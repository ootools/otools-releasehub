use super::{models::*, network, Core};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

pub fn safe_name(value: &str) -> String {
    let mut name: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(180)
        .collect();
    name = name.trim_matches([' ', '.']).to_owned();
    if name.is_empty() {
        name = "download.bin".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        name.insert(0, '_');
    }
    name
}
pub fn expected_sha(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(hash) = value.strip_prefix("sha256:") else {
        return Ok(None);
    };
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("上游 SHA-256 校验值格式无效".into());
    }
    Ok(Some(hash.to_ascii_lowercase()))
}
pub fn directory(data: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "下载记录编号无效".to_string())?;
    let root = data.join("downloads");
    let resolved = std::fs::canonicalize(&root).map_err(|_| "无法读取下载目录".to_string())?;
    if !resolved.starts_with(data)
        || std::fs::symlink_metadata(&root)
            .map_err(|_| "无法读取下载目录".to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("下载目录不能链接到外部位置".into());
    }
    let folder = root.join(id);
    match std::fs::symlink_metadata(&folder) {
        Ok(meta) => {
            if !meta.is_dir()
                || meta.file_type().is_symlink()
                || !std::fs::canonicalize(&folder)
                    .map_err(|_| "下载目录无效".to_string())?
                    .starts_with(&resolved)
            {
                return Err("下载记录目录不安全".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&folder).map_err(|_| "无法创建下载目录".to_string())?;
        }
        Err(_) => return Err("无法读取下载目录".into()),
    }
    Ok(folder)
}
impl Core {
    pub(crate) fn queue_download(
        self: &Arc<Self>,
        project_id: &str,
        asset_id: u64,
    ) -> Result<DownloadRecord, String> {
        let queued = {
            let _guard = self
                .download_queue
                .lock()
                .map_err(|_| "下载队列不可用".to_string())?;
            let p = self.store.project(project_id)?;
            let release = p.release.as_ref().ok_or("请先检查项目发行版本")?;
            let a = release
                .assets
                .iter()
                .find(|a| a.id == asset_id)
                .ok_or("找不到此发行附件")?;
            if !a.compatible {
                return Err(format!("无法下载：{}", a.reason));
            }
            network::trusted_download_url(&a.url)?;
            let digest = expected_sha(a.digest.as_deref())?;
            if let Some(existing) = self.store.downloads()?.into_iter().find(|d| {
                d.project_id == p.id
                    && d.asset_id == asset_id
                    && d.tag == release.tag
                    && !["failed", "cancelled"].contains(&d.status.as_str())
            }) {
                return Ok(existing);
            }
            let time = now();
            let record = DownloadRecord {
                id: uuid::Uuid::new_v4().to_string(),
                project_id: p.id,
                project_name: p.name,
                asset_id,
                file_name: safe_name(&a.name),
                tag: release.tag.clone(),
                url: a.url.clone(),
                kind: a.kind.clone(),
                total_bytes: a.size,
                received_bytes: 0,
                status: "queued".into(),
                path: None,
                sha256: None,
                verified: false,
                error: None,
                created_at: time.clone(),
                updated_at: time,
            };
            self.store.set_digest(&record.id, digest.as_deref())?;
            self.store.put_download(&record)?;
            record
        };
        self.download_event(&queued);
        self.changed();
        self.pump_downloads();
        Ok(queued)
    }
    pub(crate) fn retry_download(self: &Arc<Self>, id: &str) -> Result<DownloadRecord, String> {
        let record = {
            let _guard = self
                .download_queue
                .lock()
                .map_err(|_| "下载队列不可用".to_string())?;
            if self
                .active_downloads
                .lock()
                .map_err(|_| "下载队列不可用".to_string())?
                .contains_key(id)
            {
                return Err("下载仍在停止，请稍后重试".into());
            }
            self.store
                .mutate("downloads", id, |d: &mut DownloadRecord| {
                    if !["failed", "cancelled"].contains(&d.status.as_str()) {
                        return Err("只能重试失败或已取消的下载".into());
                    }
                    network::trusted_download_url(&d.url)?;
                    d.status = "queued".into();
                    d.received_bytes = 0;
                    d.sha256 = None;
                    d.verified = false;
                    d.path = None;
                    d.error = None;
                    d.updated_at = now();
                    Ok(())
                })?
        };
        self.download_event(&record);
        self.changed();
        self.pump_downloads();
        Ok(record)
    }
    pub(crate) fn cancel_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
        // Set the durable state first; a worker checks cancellation again before publishing a complete file.
        let record = self
            .store
            .mutate("downloads", id, |d: &mut DownloadRecord| {
                if !["queued", "downloading"].contains(&d.status.as_str()) {
                    return Err("此下载已结束，无法取消".into());
                }
                d.status = "cancelled".into();
                d.error = Some("下载已取消，可重试".into());
                d.updated_at = now();
                Ok(())
            })?;
        if let Some(flag) = self
            .active_downloads
            .lock()
            .map_err(|_| "下载队列不可用".to_string())?
            .get(id)
        {
            flag.store(true, Ordering::Release);
        }
        self.download_event(&record);
        self.changed();
        self.pump_downloads();
        Ok(())
    }
    pub(crate) fn pump_downloads(self: &Arc<Self>) {
        let Ok(settings) = self.store.settings() else {
            return;
        };
        let Ok(mut records) = self.store.downloads() else {
            return;
        };
        records.reverse();
        let Ok(mut active) = self.active_downloads.lock() else {
            return;
        };
        for record in records.into_iter().filter(|d| d.status == "queued") {
            if active.len() >= settings.max_concurrent_downloads {
                break;
            }
            if active.contains_key(&record.id) {
                continue;
            }
            let cancel = Arc::new(AtomicBool::new(false));
            active.insert(record.id.clone(), cancel.clone());
            let core = self.clone();
            tokio::spawn(async move {
                let result = core.download_worker(&record.id, cancel.clone()).await;
                if let Err(error) = result {
                    if let Ok(d) =
                        core.store
                            .mutate("downloads", &record.id, |d: &mut DownloadRecord| {
                                if d.status == "cancelled" || cancel.load(Ordering::Acquire) {
                                    d.status = "cancelled".into();
                                    d.error = Some("下载已取消，可重试".into());
                                } else {
                                    d.status = "failed".into();
                                    d.error = Some(error);
                                }
                                d.updated_at = now();
                                Ok(())
                            })
                    {
                        core.download_event(&d);
                    }
                }
                if let Ok(mut active) = core.active_downloads.lock() {
                    active.remove(&record.id);
                }
                core.changed();
                core.pump_downloads();
            });
        }
    }
    async fn download_worker(&self, id: &str, cancel: Arc<AtomicBool>) -> Result<(), String> {
        let mut record = self
            .store
            .mutate("downloads", id, |d: &mut DownloadRecord| {
                if d.status != "queued" {
                    return Err("下载已取消".into());
                }
                d.status = "downloading".into();
                d.updated_at = now();
                Ok(())
            })?;
        self.download_event(&record);
        let folder = directory(&self.data_dir, id)?;
        if safe_name(&record.file_name) != record.file_name {
            return Err("下载文件名无效".into());
        }
        let partial = folder.join(format!("{}.part", record.file_name));
        let destination = folder.join(&record.file_name);
        if destination.exists() {
            return Err("完整下载文件已存在，请勿覆盖".into());
        }
        if let Ok(meta) = std::fs::symlink_metadata(&partial) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("临时下载文件不安全".into());
            }
            tokio::fs::remove_file(&partial)
                .await
                .map_err(|_| "无法清理中断的下载".to_string())?;
        }
        let result = self
            .stream_download(&mut record, &partial, &destination, &cancel)
            .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
        result
    }
    async fn stream_download(
        &self,
        record: &mut DownloadRecord,
        partial: &Path,
        destination: &Path,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("下载已取消".into());
        }
        let url = network::trusted_download_url(&record.url)?;
        // This client intentionally has no API token, default authorization header, or cookie store.
        let client = reqwest::Client::builder()
            .user_agent("ReleaseHub/0.1 artifact-download")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(3600))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5
                    || network::trusted_download_url(attempt.url().as_str()).is_err()
                {
                    attempt.error("不可信下载跳转")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| "下载服务初始化失败".to_string())?;
        let response = tokio::select! {
            response=client.get(url).send()=>response.map_err(|e|network::request_error(&e))?,
            _=wait_cancel(cancel)=>return Err("下载已取消".into()),
        };
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                404 => "发行附件已删除，请重新检查更新",
                403 | 429 => "GitHub 暂时限制下载，请稍后重试",
                _ => "附件下载失败，请稍后重试",
            }
            .into());
        }
        network::trusted_download_url(response.url().as_str())?;
        let expected_size = record.total_bytes;
        if expected_size == 0 || expected_size > network::MAX_DOWNLOAD_BYTES {
            return Err("发行附件大小无效或超过4GB限制".into());
        }
        if response
            .content_length()
            .is_some_and(|n| n != expected_size || n > network::MAX_DOWNLOAD_BYTES)
        {
            return Err("下载大小与发行附件记录不符，请重新检查更新".into());
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(partial)
            .await
            .map_err(|_| "无法创建下载文件，请检查空间和目录权限".to_string())?;
        let mut stream = response.bytes_stream();
        let mut hash = Sha256::new();
        let mut last = Instant::now();
        loop {
            let chunk = tokio::select! {
                chunk=tokio::time::timeout(Duration::from_secs(60),stream.next())=>chunk.map_err(|_|"下载连接长时间没有响应".to_string())?,
                _=wait_cancel(cancel)=>return Err("下载已取消".into()),
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|e| network::request_error(&e))?;
            if cancel.load(Ordering::Acquire) {
                return Err("下载已取消".into());
            }
            record.received_bytes = record
                .received_bytes
                .checked_add(chunk.len() as u64)
                .ok_or("下载大小超限")?;
            if record.received_bytes > expected_size
                || record.received_bytes > network::MAX_DOWNLOAD_BYTES
            {
                return Err("下载内容超过附件声明大小".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| "下载写入失败，请检查磁盘空间".to_string())?;
            hash.update(&chunk);
            if last.elapsed() >= Duration::from_millis(400) {
                record.updated_at = now();
                let progress = record.received_bytes;
                let current =
                    self.store
                        .mutate("downloads", &record.id, |d: &mut DownloadRecord| {
                            if d.status == "cancelled" {
                                return Err("下载已取消".into());
                            }
                            d.received_bytes = progress;
                            d.updated_at = now();
                            Ok(())
                        })?;
                self.download_event(&current);
                last = Instant::now();
            }
        }
        if record.received_bytes != expected_size {
            return Err("下载不完整，请重试".into());
        }
        file.flush()
            .await
            .map_err(|_| "无法保存下载文件".to_string())?;
        file.sync_all()
            .await
            .map_err(|_| "无法保存下载文件".to_string())?;
        drop(file);
        let digest = format!("{:x}", hash.finalize());
        let upstream = self.store.digest(&record.id)?;
        if upstream.as_ref().is_some_and(|h| h != &digest) {
            return Err("SHA-256 校验不符，已拒绝此文件".into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("下载已取消".into());
        }
        // Hard-link publication is atomic and refuses to overwrite an existing completed file on all supported hosts.
        tokio::fs::hard_link(partial, destination)
            .await
            .map_err(|_| "无法保存完整文件，目标可能已存在".to_string())?;
        let completed = self
            .store
            .mutate("downloads", &record.id, |d: &mut DownloadRecord| {
                if d.status == "cancelled" || cancel.load(Ordering::Acquire) {
                    return Err("下载已取消".into());
                }
                d.status = "completed".into();
                d.received_bytes = record.received_bytes;
                d.path = Some(destination.to_string_lossy().into_owned());
                d.sha256 = Some(digest);
                d.verified = upstream.is_some();
                d.error = None;
                d.updated_at = now();
                Ok(())
            });
        match completed {
            Ok(d) => {
                let _ = tokio::fs::remove_file(partial).await;
                self.download_event(&d);
                Ok(())
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(destination).await;
                Err(e)
            }
        }
    }
}
async fn wait_cancel(cancel: &AtomicBool) {
    loop {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_names_cannot_escape() {
        assert_eq!(safe_name("../CON.exe"), "_CON.exe");
        assert!(!safe_name("a\\b:evil.zip").contains(['\\', ':', '/']));
    }
    #[test]
    fn publisher_digest_is_separate_from_computed_hash() {
        assert_eq!(expected_sha(None).unwrap(), None);
        assert!(expected_sha(Some("sha256:bad")).is_err());
        assert!(expected_sha(Some(&format!("sha256:{}", "a".repeat(64))))
            .unwrap()
            .is_some());
    }
    fn paused_queue() -> (tempfile::TempDir, Arc<Core>, Project) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().into(), true, Arc::new(|_, _| {})).unwrap();
        let mut settings = core.store.settings().unwrap();
        settings.max_concurrent_downloads = 1;
        core.store.save_settings(&settings).unwrap();
        // Occupy the only slot: these lifecycle tests must never start a network request.
        core.active_downloads
            .lock()
            .unwrap()
            .insert("held-test-slot".into(), Arc::new(AtomicBool::new(false)));
        let mut project = test_project("owner", "app");
        project.release = Some(Release {
            id: 1,
            tag: "v1.0.0".into(),
            name: "test".into(),
            body: String::new(),
            published_at: now(),
            url: "https://github.com/owner/app/releases/tag/v1.0.0".into(),
            prerelease: false,
            assets: vec![ReleaseAsset {
                id: 7,
                name: "app-win-x64.zip".into(),
                size: 10,
                url: "https://github.com/owner/app/releases/download/v1.0.0/app-win-x64.zip".into(),
                content_type: "application/zip".into(),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                kind: "portable".into(),
                compatible: true,
                score: 80,
                reason: "test".into(),
            }],
        });
        core.store.put_project(&project).unwrap();
        (dir, core, project)
    }
    #[test]
    fn queue_cancel_and_retry_preserve_identity_and_publisher_digest() {
        let (_dir, core, project) = paused_queue();
        let queued = core.queue_download(&project.id, 7).unwrap();
        assert_eq!(queued.status, "queued");
        assert_eq!(core.queue_download(&project.id, 7).unwrap().id, queued.id);
        assert_eq!(core.store.downloads().unwrap().len(), 1);
        let flag = Arc::new(AtomicBool::new(false));
        core.active_downloads
            .lock()
            .unwrap()
            .insert(queued.id.clone(), flag.clone());
        core.store
            .mutate("downloads", &queued.id, |d: &mut DownloadRecord| {
                d.status = "downloading".into();
                d.received_bytes = 4;
                Ok(())
            })
            .unwrap();
        core.cancel_download(&queued.id).unwrap();
        assert!(flag.load(Ordering::Acquire));
        assert_eq!(core.store.download(&queued.id).unwrap().status, "cancelled");
        assert!(core.retry_download(&queued.id).is_err());
        core.active_downloads.lock().unwrap().remove(&queued.id);
        let retried = core.retry_download(&queued.id).unwrap();
        assert_eq!(retried.id, queued.id);
        assert_eq!(retried.status, "queued");
        assert_eq!(retried.received_bytes, 0);
        assert!(retried.path.is_none());
        assert!(retried.sha256.is_none());
        assert!(!retried.verified);
        assert!(retried.error.is_none());
        assert_eq!(core.store.digest(&queued.id).unwrap(), Some("a".repeat(64)));
        assert!(core.retry_download(&queued.id).is_err());
        core.store
            .mutate("downloads", &queued.id, |d: &mut DownloadRecord| {
                d.status = "failed".into();
                d.error = Some("interrupted".into());
                Ok(())
            })
            .unwrap();
        assert_eq!(core.retry_download(&queued.id).unwrap().status, "queued");
    }
    #[tokio::test]
    async fn cancelled_worker_never_requests_or_publishes_a_file() {
        let (_dir, core, project) = paused_queue();
        let queued = core.queue_download(&project.id, 7).unwrap();
        let result = core
            .download_worker(&queued.id, Arc::new(AtomicBool::new(true)))
            .await;
        assert_eq!(result.unwrap_err(), "下载已取消");
        let folder = directory(&core.data_dir, &queued.id).unwrap();
        assert!(!folder.join(&queued.file_name).exists());
        assert!(!folder.join(format!("{}.part", queued.file_name)).exists());
        core.cancel_download(&queued.id).unwrap();
        assert_eq!(core.store.download(&queued.id).unwrap().status, "cancelled");
    }
}
