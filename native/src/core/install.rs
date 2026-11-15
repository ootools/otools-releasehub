use super::{downloads, models::*, Core};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const MAX_ENTRIES: usize = 10000;
const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
fn io_error(_: impl std::fmt::Display) -> String {
    "安装文件读写失败，请检查权限和磁盘空间".into()
}
fn child_directory(root: &Path, name: &str) -> Result<PathBuf, String> {
    let child = root.join(name);
    if !child.exists() {
        fs::create_dir(&child).map_err(io_error)?;
    }
    let meta = fs::symlink_metadata(&child).map_err(io_error)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("受管安装目录不能包含链接".into());
    }
    let resolved = fs::canonicalize(&child).map_err(io_error)?;
    if resolved.parent() != Some(root) {
        return Err("受管安装目录不能跳转到其他位置".into());
    }
    Ok(resolved)
}
fn managed_version_name(d: &DownloadRecord) -> String {
    format!("{}--{}", downloads::safe_name(&d.tag), d.id)
}
fn existing_managed_directory(root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let path = root.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e)),
    };
    let linked = meta.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || meta.file_attributes() & 0x400 != 0 // Also reject Windows directory junctions/reparse points.
    };
    if !meta.is_dir() || linked {
        return Err("受管版本目录不能是文件或链接".into());
    }
    let resolved = fs::canonicalize(&path).map_err(io_error)?;
    if resolved.parent() != Some(root) {
        return Err("受管版本目录不能跳转到其他位置".into());
    }
    Ok(Some(resolved))
}
fn managed_version_path(data: &Path, d: &DownloadRecord) -> Result<Option<PathBuf>, String> {
    uuid::Uuid::parse_str(&d.project_id).map_err(|_| "项目编号无效".to_string())?;
    uuid::Uuid::parse_str(&d.id).map_err(|_| "下载记录编号无效".to_string())?;
    let Some(root) = existing_managed_directory(data, "apps")? else {
        return Ok(None);
    };
    let Some(project) = existing_managed_directory(&root, &d.project_id)? else {
        return Ok(None);
    };
    existing_managed_directory(&project, &managed_version_name(d))
}
pub fn zip_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.len() > 1024
        || name.contains(['\\', ':', '\0'])
        || name.starts_with('/')
    {
        return Err("ZIP 包包含越界或危险路径，已停止安装".into());
    }
    let path = Path::new(name);
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_str().ok_or("ZIP 文件名编码无效")?;
                if text.is_empty()
                    || text.ends_with(['.', ' '])
                    || downloads::safe_name(text) != text
                {
                    return Err("ZIP 包含 Windows 不安全文件名".into());
                }
            }
            _ => return Err("ZIP 包包含越界路径，已停止安装".into()),
        }
    }
    if path.as_os_str().is_empty() {
        return Err("ZIP 路径为空".into());
    }
    Ok(path.to_owned())
}
fn verify_download(data: &Path, d: &DownloadRecord) -> Result<PathBuf, String> {
    if downloads::safe_name(&d.file_name) != d.file_name {
        return Err("下载记录文件名无效".into());
    }
    let folder = downloads::directory(data, &d.id)?;
    let expected = folder.join(&d.file_name);
    let recorded = d.path.as_deref().ok_or("下载文件路径缺失")?;
    let recorded = Path::new(recorded);
    if recorded != expected {
        return Err("下载记录路径不属于受管目录".into());
    }
    let meta =
        fs::symlink_metadata(&expected).map_err(|_| "下载文件不存在，请重试下载".to_string())?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || fs::canonicalize(&expected).map_err(io_error)?.parent()
            != Some(fs::canonicalize(folder).map_err(io_error)?.as_path())
    {
        return Err("下载文件被替换为链接，已拒绝安装".into());
    }
    if meta.len() != d.total_bytes {
        return Err("下载文件大小已改变，请重新下载".into());
    }
    let mut file = fs::File::open(&expected).map_err(io_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest = format!("{:x}", hash.finalize());
    if d.sha256.as_deref() != Some(digest.as_str()) {
        return Err("下载文件内容已改变，请重新下载".into());
    }
    Ok(expected)
}
fn extract_zip(source: &Path, stage: &Path) -> Result<(), String> {
    let file = fs::File::open(source).map_err(io_error)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "ZIP 文件损坏或格式不支持".to_string())?;
    if zip.is_empty() || zip.len() > MAX_ENTRIES {
        return Err("ZIP 文件为空或条目超过10000个限制".into());
    }
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    let mut regular = 0;
    // Validate all names and declared sizes before creating any extracted content.
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|_| "ZIP 文件目录损坏".to_string())?;
        let path = zip_path(entry.name())?;
        if entry.enclosed_name().is_none() {
            return Err("ZIP 包含越界路径".into());
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if ![0, 0o100000, 0o040000].contains(&kind) {
                return Err("ZIP 包含符号链接或特殊文件，已拒绝安装".into());
            }
        }
        let name = path.to_string_lossy().to_lowercase();
        if !seen.insert(name) {
            return Err("ZIP 包含重复文件名，已拒绝覆盖".into());
        }
        total = total.checked_add(entry.size()).ok_or("ZIP 展开大小超限")?;
        if total > MAX_EXPANDED || entry.size() > super::network::MAX_DOWNLOAD_BYTES {
            return Err("ZIP 展开内容超过安全大小限制".into());
        }
        if !entry.is_dir() {
            regular += 1;
        }
    }
    if regular == 0 {
        return Err("ZIP 没有可安装文件".into());
    }
    let mut actual = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| "ZIP 文件损坏".to_string())?;
        let path = stage.join(zip_path(entry.name())?);
        if entry.is_dir() {
            fs::create_dir_all(&path).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "ZIP 路径冲突或无法创建文件".to_string())?;
        let size = entry.size();
        let mut copied = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = entry
                .read(&mut buffer)
                .map_err(|_| "ZIP 文件内容校验失败".to_string())?;
            if n == 0 {
                break;
            }
            copied = copied.checked_add(n as u64).ok_or("ZIP 展开大小超限")?;
            actual = actual.checked_add(n as u64).ok_or("ZIP 展开大小超限")?;
            if copied > size || actual > MAX_EXPANDED {
                return Err("ZIP 实际大小超限，已停止安装".into());
            }
            output.write_all(&buffer[..n]).map_err(io_error)?;
        }
        if copied != size {
            return Err("ZIP 文件内容不完整".into());
        }
        output.sync_all().map_err(io_error)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            // Preserve only ordinary executable/read/write bits, never setuid/setgid.
            fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))
                .map_err(io_error)?;
        }
    }
    Ok(())
}
#[cfg(windows)]
fn launch_installer(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["exe", "msi"].contains(&extension.as_str()) {
        return Err("仅支持 EXE/MSI 安装器".into());
    }
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    // Native ShellExecute opens exactly the verified local installer with no arguments or scripts.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result <= 32 {
        return Err("安装器未启动或用户取消了权限确认，请手动打开下载文件".into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn launch_installer(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    let launcher = {
        if !["dmg", "pkg"].contains(&extension.as_str()) {
            return Err("请选择 macOS DMG/PKG 安装器".into());
        }
        "open"
    };
    #[cfg(target_os = "linux")]
    let launcher = {
        if !["deb", "rpm"].contains(&extension.as_str()) {
            return Err("请选择 DEB/RPM 安装包".into());
        }
        "xdg-open"
    };
    std::process::Command::new(launcher)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "无法打开系统安装器，请从下载目录手动安装".to_string())?;
    Ok(())
}
impl Core {
    pub(crate) fn list_managed_versions(
        &self,
        project_id: &str,
    ) -> Result<Vec<ManagedVersion>, String> {
        let project = self.store.project(project_id)?;
        let mut versions = Vec::new();
        for d in self.store.downloads()?.into_iter().filter(|d| {
            d.project_id == project.id && d.status == "installed" && d.kind == "portable"
        }) {
            let Some(path) = managed_version_path(&self.data_dir, &d)? else {
                continue;
            };
            let active = project.installed_source.as_deref() == Some("managed")
                && project.installed_version.as_deref() == Some(d.tag.as_str())
                && project.installed_path.as_deref().map(Path::new) == Some(path.as_path());
            versions.push(ManagedVersion {
                download_id: d.id,
                version: d.tag,
                path: path.to_string_lossy().into_owned(),
                active,
            });
        }
        Ok(versions)
    }
    pub(crate) fn activate_managed_version(
        &self,
        project_id: &str,
        download_id: &str,
    ) -> Result<Project, String> {
        let installing = self
            .installing
            .lock()
            .map_err(|_| "安装状态不可用".to_string())?;
        if installing.contains(project_id) {
            return Err("此项目正在安装，请稍后再切换版本".into());
        }
        let record = self.store.download(download_id)?;
        if record.project_id != project_id
            || record.status != "installed"
            || record.kind != "portable"
        {
            return Err("只能切换此项目已准备好的受管便携版本".into());
        }
        let path =
            managed_version_path(&self.data_dir, &record)?.ok_or("受管版本目录已丢失，无法切换")?;
        let project = self
            .store
            .mutate("projects", project_id, |p: &mut Project| {
                if p.id != project_id
                    || p.installed_source.as_deref() != Some("managed")
                    || p.installed_path
                        .as_deref()
                        .and_then(|s| Path::new(s).parent())
                        != path.parent()
                {
                    return Err("只能切换当前受管项目，手动记录的外部安装不会修改".into());
                }
                p.installed_version = Some(record.tag.clone());
                p.installed_path = Some(path.to_string_lossy().into_owned());
                p.error = None;
                super::selection::update_status(p);
                Ok(())
            })?;
        drop(installing);
        self.changed();
        Ok(project)
    }
    pub(crate) async fn install_download(self: &Arc<Self>, id: &str) -> Result<Value, String> {
        let record = self.store.download(id)?;
        if !["completed", "installed"].contains(&record.status.as_str()) {
            return Err("请先完成下载后再安装".into());
        }
        let project = self.store.project(&record.project_id)?;
        {
            let mut installing = self
                .installing
                .lock()
                .map_err(|_| "安装状态不可用".to_string())?;
            if !installing.insert(project.id.clone()) {
                return Err("此项目正在安装，请稍后再试".into());
            }
        }
        let transitioned = self
            .store
            .mutate("downloads", id, |d: &mut DownloadRecord| {
                if !["completed", "installed"].contains(&d.status.as_str()) {
                    return Err("下载状态已改变，请重试".into());
                }
                d.status = "installing".into();
                d.error = None;
                d.updated_at = now();
                Ok(())
            });
        let transitioned = match transitioned {
            Ok(d) => d,
            Err(e) => {
                if let Ok(mut state) = self.installing.lock() {
                    state.remove(&project.id);
                }
                return Err(e);
            }
        };
        self.download_event(&transitioned);
        self.changed();
        let core = self.clone();
        let task_record = record.clone();
        let task_project = project.clone();
        let result =
            tokio::task::spawn_blocking(move || core.install_blocking(&task_record, &task_project))
                .await
                .map_err(|_| "安装任务被中断，请重新尝试".to_string())
                .and_then(|r| r);
        if let Ok(mut state) = self.installing.lock() {
            state.remove(&project.id);
        }
        if let Err(e) = &result {
            let d = self
                .store
                .mutate("downloads", id, |d: &mut DownloadRecord| {
                    d.status = record.status.clone();
                    d.error = Some(e.clone());
                    d.updated_at = now();
                    Ok(())
                })?;
            self.download_event(&d);
        }
        self.changed();
        result
    }
    fn install_blocking(&self, d: &DownloadRecord, p: &Project) -> Result<Value, String> {
        let source = verify_download(&self.data_dir, d)?;
        if d.kind == "installer" {
            launch_installer(&source)?;
            let updated = self
                .store
                .mutate("downloads", &d.id, |d: &mut DownloadRecord| {
                    d.status = "completed".into();
                    d.error = None;
                    d.updated_at = now();
                    Ok(())
                })?;
            self.download_event(&updated);
            return Ok(
                json!({"kind":"installer","message":"已打开官方安装器。完成后请手动记录已安装版本。","path":source.to_string_lossy(),"installed":false}),
            );
        }
        let standalone = p.owner.eq_ignore_ascii_case("Lake1059")
            && p.repo.eq_ignore_ascii_case("FFmpegFreeUI")
            && d.file_name.eq_ignore_ascii_case("FFmpegFreeUI.x64.exe");
        let archive = d.file_name.to_ascii_lowercase().ends_with(".zip");
        if d.kind != "portable" || (!archive && !standalone) {
            return Err("此文件不支持受管便携安装".into());
        }
        uuid::Uuid::parse_str(&p.id).map_err(|_| "项目编号无效".to_string())?;
        let root = fs::canonicalize(self.data_dir.join("apps")).map_err(io_error)?;
        if root.parent() != Some(self.data_dir.as_path()) {
            return Err("受管安装根目录不能跳转".into());
        }
        let project_dir = child_directory(&root, &p.id)?;
        let stage_name = format!(".staging-{}", uuid::Uuid::new_v4());
        let stage = project_dir.join(&stage_name);
        fs::create_dir(&stage).map_err(io_error)?;
        let prepared = if archive {
            extract_zip(&source, &stage)
        } else {
            (|| {
                let mut input = fs::File::open(&source).map_err(io_error)?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(stage.join(&d.file_name))
                    .map_err(io_error)?;
                let copied = std::io::copy(&mut input, &mut output).map_err(io_error)?;
                if copied != d.total_bytes {
                    return Err("便携主程序复制不完整".to_string());
                }
                output.sync_all().map_err(io_error)?;
                Ok(())
            })()
        };
        if let Err(error) = prepared {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
        let destination = project_dir.join(managed_version_name(d));
        if destination.exists() {
            let _ = fs::remove_dir_all(&stage);
            return Err("此版本已经安装，旧文件和配置保持不变".into());
        }
        if let Err(e) = fs::rename(&stage, &destination) {
            let _ = fs::remove_dir_all(&stage);
            return Err(io_error(e));
        }
        let path = destination.to_string_lossy().into_owned();
        self.store.mutate("projects", &p.id, |p: &mut Project| {
            p.installed_version = Some(d.tag.clone());
            p.installed_path = Some(path.clone());
            p.installed_source = Some("managed".into());
            super::selection::update_status(p);
            Ok(())
        })?;
        let updated = self
            .store
            .mutate("downloads", &d.id, |d: &mut DownloadRecord| {
                d.status = "installed".into();
                d.error = None;
                d.updated_at = now();
                Ok(())
            })?;
        self.download_event(&updated);
        let message = if p.installed_path.is_some() {
            "新版本已安装到独立受管目录；旧版本和配置未改动，请按软件说明迁移设置。"
        } else {
            "已准备好受管便携程序，不会自动运行软件。"
        };
        Ok(json!({"kind":"portable","message":message,"path":path,"installed":true}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prepared_version(core: &Core, project: &Project, tag: &str) -> (DownloadRecord, PathBuf) {
        let id = uuid::Uuid::new_v4().to_string();
        let folder = downloads::directory(&core.data_dir, &id).unwrap();
        let source = folder.join("FFmpegFreeUI.x64.exe");
        let bytes = format!("local test data for {tag}; not a program");
        fs::write(&source, bytes.as_bytes()).unwrap();
        let time = now();
        let download=DownloadRecord{id,project_id:project.id.clone(),project_name:project.name.clone(),asset_id:1,
            file_name:"FFmpegFreeUI.x64.exe".into(),tag:tag.into(),url:format!("https://github.com/Lake1059/FFmpegFreeUI/releases/download/{tag}/FFmpegFreeUI.x64.exe"),kind:"portable".into(),
            total_bytes:bytes.len()as u64,received_bytes:bytes.len()as u64,status:"completed".into(),path:Some(source.to_string_lossy().into_owned()),
            sha256:Some(format!("{:x}",Sha256::digest(bytes.as_bytes()))),verified:false,error:None,created_at:time.clone(),updated_at:time};
        core.store.put_download(&download).unwrap();
        let result = core.install_blocking(&download, project).unwrap();
        let path = PathBuf::from(result["path"].as_str().unwrap());
        assert_eq!(result["installed"], true);
        (core.store.download(&download.id).unwrap(), path)
    }
    #[tokio::test]
    async fn managed_versions_switch_after_confirmation_and_preserve_both_directories() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let changes = Arc::new(AtomicUsize::new(0));
        let observed = changes.clone();
        let core = Core::new(
            dir.path().join("data"),
            true,
            Arc::new(move |event, _| {
                if event == "releasehub:changed" {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            }),
        )
        .unwrap();
        let mut project = test_project("Lake1059", "FFmpegFreeUI");
        project.release=Some(Release{id:2,tag:"v2.0.0".into(),name:"2.0.0".into(),body:String::new(),published_at:now(),
            url:"https://github.com/Lake1059/FFmpegFreeUI/releases/tag/v2.0.0".into(),prerelease:false,
            assets:vec![ReleaseAsset{id:1,name:"FFmpegFreeUI.x64.exe".into(),size:32,url:"https://github.com/Lake1059/FFmpegFreeUI/releases/download/v2.0.0/FFmpegFreeUI.x64.exe".into(),
                content_type:"application/octet-stream".into(),digest:None,kind:"portable".into(),compatible:true,score:100,reason:String::new()}]});
        core.store.put_project(&project).unwrap();
        let (old, old_path) = prepared_version(&core, &project, "v1.0.0");
        fs::write(old_path.join("settings.json"), b"old user settings").unwrap();
        let (new, new_path) = prepared_version(&core, &project, "v2.0.0");
        fs::write(new_path.join("settings.json"), b"new user settings").unwrap();
        let downloads_before = serde_json::to_value(core.store.downloads().unwrap()).unwrap();
        let listed = core
            .handle("list_managed_versions", json!({"projectId":project.id}))
            .await
            .unwrap();
        let listed = listed.as_array().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].as_object().unwrap().len(), 4);
        assert_eq!(
            listed.iter().find(|v| v["downloadId"] == new.id).unwrap()["active"],
            true
        );
        assert_eq!(
            listed.iter().find(|v| v["downloadId"] == old.id).unwrap()["active"],
            false
        );
        let mut request = json!({"projectId":project.id,"downloadId":old.id});
        assert!(core
            .handle("activate_managed_version", request.clone())
            .await
            .is_err());
        for confirmation in [Value::Null, json!(false), json!("true")] {
            request["confirmed"] = confirmation;
            assert!(core
                .handle("activate_managed_version", request.clone())
                .await
                .is_err());
        }
        assert_eq!(changes.load(Ordering::SeqCst), 0);
        assert_eq!(
            core.store
                .project(&project.id)
                .unwrap()
                .installed_version
                .as_deref(),
            Some("v2.0.0")
        );
        request["confirmed"] = json!(true);
        let activated = core
            .handle("activate_managed_version", request.clone())
            .await
            .unwrap();
        assert_eq!(activated["installedVersion"], old.tag);
        assert_eq!(
            activated["installedPath"],
            old_path.to_string_lossy().as_ref()
        );
        assert_eq!(activated["installedSource"], "managed");
        assert_eq!(activated["status"], "updateAvailable");
        assert_eq!(
            core.handle("activate_managed_version", request)
                .await
                .unwrap(),
            activated
        );
        let listed = core.list_managed_versions(&project.id).unwrap();
        assert_eq!(listed.iter().filter(|v| v.active).count(), 1);
        assert!(
            listed
                .iter()
                .find(|v| v.download_id == old.id)
                .unwrap()
                .active
        );
        let restored = core
            .handle(
                "activate_managed_version",
                json!({"projectId":project.id,"downloadId":new.id,"confirmed":true}),
            )
            .await
            .unwrap();
        assert_eq!(restored["installedVersion"], new.tag);
        assert_eq!(restored["status"], "upToDate");
        assert_eq!(changes.load(Ordering::SeqCst), 3);
        assert_eq!(
            fs::read(old_path.join("settings.json")).unwrap(),
            b"old user settings"
        );
        assert_eq!(
            fs::read(new_path.join("settings.json")).unwrap(),
            b"new user settings"
        );
        assert_eq!(
            serde_json::to_value(core.store.downloads().unwrap()).unwrap(),
            downloads_before
        );
    }
    #[tokio::test]
    async fn managed_versions_reject_external_cross_project_missing_and_unprepared_targets() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().join("data"), true, Arc::new(|_, _| {})).unwrap();
        let project = test_project("Lake1059", "FFmpegFreeUI");
        core.store.put_project(&project).unwrap();
        let (installed, path) = prepared_version(&core, &project, "v1.0.0");
        let current = core.store.project(&project.id).unwrap();
        let request = json!({"projectId":project.id,"downloadId":installed.id,"confirmed":true});
        core.installing.lock().unwrap().insert(project.id.clone());
        assert!(core
            .handle("activate_managed_version", request.clone())
            .await
            .is_err());
        core.installing.lock().unwrap().remove(&project.id);
        let other = test_project("other-owner", "other-project");
        core.store.put_project(&other).unwrap();
        assert!(core
            .handle(
                "activate_managed_version",
                json!({"projectId":other.id,"downloadId":installed.id,"confirmed":true})
            )
            .await
            .is_err());
        assert!(core.list_managed_versions(&other.id).unwrap().is_empty());
        assert!(!core.data_dir.join("apps").join(&other.id).exists());
        let external = dir.path().join("external-app");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("settings.json"), b"external settings").unwrap();
        for source in ["manual", "managed"] {
            let mut manual = current.clone();
            manual.installed_source = Some(source.into());
            manual.installed_path = Some(external.to_string_lossy().into_owned());
            core.store.put_project(&manual).unwrap();
            assert!(core
                .handle("activate_managed_version", request.clone())
                .await
                .is_err());
            assert_eq!(
                serde_json::to_value(core.store.project(&project.id).unwrap()).unwrap(),
                serde_json::to_value(manual).unwrap()
            );
            assert!(core
                .list_managed_versions(&project.id)
                .unwrap()
                .iter()
                .all(|v| !v.active));
        }
        core.store.put_project(&current).unwrap();
        assert_eq!(
            fs::read(external.join("settings.json")).unwrap(),
            b"external settings"
        );
        let mut missing = installed.clone();
        missing.id = uuid::Uuid::new_v4().to_string();
        missing.tag = "v0.9.0".into();
        core.store.put_download(&missing).unwrap();
        assert!(core
            .handle(
                "activate_managed_version",
                json!({"projectId":project.id,"downloadId":missing.id,"confirmed":true})
            )
            .await
            .is_err());
        for (kind, status) in [("portable", "completed"), ("installer", "installed")] {
            let mut unprepared = installed.clone();
            unprepared.id = uuid::Uuid::new_v4().to_string();
            unprepared.kind = kind.into();
            unprepared.status = status.into();
            core.store.put_download(&unprepared).unwrap();
            fs::create_dir(
                path.parent()
                    .unwrap()
                    .join(managed_version_name(&unprepared)),
            )
            .unwrap();
            assert!(core
                .handle(
                    "activate_managed_version",
                    json!({"projectId":project.id,"downloadId":unprepared.id,"confirmed":true})
                )
                .await
                .is_err());
        }
        fs::create_dir(path.parent().unwrap().join("unrecorded-folder")).unwrap();
        let listed = core.list_managed_versions(&project.id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].download_id, installed.id);
        assert!(core
            .handle(
                "activate_managed_version",
                json!({"projectId":project.id,"downloadId":"../outside","confirmed":true})
            )
            .await
            .is_err());
        assert_eq!(
            serde_json::to_value(core.store.project(&project.id).unwrap()).unwrap(),
            serde_json::to_value(current).unwrap()
        );
    }
    #[cfg(any(windows, unix))]
    #[tokio::test]
    async fn managed_versions_reject_links_at_every_managed_directory_level() {
        for level in ["apps", "project", "version"] {
            let dir = tempfile::tempdir().unwrap();
            let core = Core::new(dir.path().join("data"), true, Arc::new(|_, _| {})).unwrap();
            let project = test_project("Lake1059", "FFmpegFreeUI");
            core.store.put_project(&project).unwrap();
            let (installed, version) = prepared_version(&core, &project, "v1.0.0");
            let before = serde_json::to_value(core.store.project(&project.id).unwrap()).unwrap();
            let path = match level {
                "apps" => core.data_dir.join("apps"),
                "project" => core.data_dir.join("apps").join(&project.id),
                _ => version,
            };
            let moved = path.with_file_name(format!("moved-{level}"));
            assert!(path.starts_with(&core.data_dir));
            assert!(!moved.exists());
            fs::rename(&path, &moved).unwrap();
            #[cfg(windows)]
            {
                let result = std::process::Command::new("cmd.exe")
                    .args(["/d", "/c", "mklink", "/J"])
                    .arg(&path)
                    .arg(&moved)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&moved, &path).unwrap();
            assert!(core.list_managed_versions(&project.id).is_err(), "{level}");
            assert!(
                core.handle(
                    "activate_managed_version",
                    json!({"projectId":project.id,"downloadId":installed.id,"confirmed":true})
                )
                .await
                .is_err(),
                "{level}"
            );
            assert_eq!(
                serde_json::to_value(core.store.project(&project.id).unwrap()).unwrap(),
                before
            );
        }
    }
    #[test]
    fn zip_rejects_traversal_and_windows_aliases() {
        for bad in [
            "../escape.exe",
            "C:/escape.exe",
            "a\\..\\escape.exe",
            "/root/file",
            "a/NUL.txt",
            "a/file. ",
            "a:file",
        ] {
            assert!(zip_path(bad).is_err(), "{bad}");
        }
        assert!(zip_path("App/config/settings.json").is_ok());
    }
    #[test]
    fn standalone_upgrade_keeps_old_config_and_rebases_after_move() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let moved = temp.path().join("moved");
        let core = Core::new(original.clone(), true, Arc::new(|_, _| {})).unwrap();
        let mut project = test_project("Lake1059", "FFmpegFreeUI");
        core.store.put_project(&project).unwrap();
        let bytes = b"local-test-main";
        let mut old_config = PathBuf::new();
        for (index, tag) in ["6.5.0", "6.6.0"].iter().enumerate() {
            let id = uuid::Uuid::new_v4().to_string();
            let folder = downloads::directory(&core.data_dir, &id).unwrap();
            let file = folder.join("FFmpegFreeUI.x64.exe");
            fs::write(&file, bytes).unwrap();
            let time = now();
            let download=DownloadRecord{id,project_id:project.id.clone(),project_name:project.name.clone(),asset_id:index as u64+1,
                file_name:"FFmpegFreeUI.x64.exe".into(),tag:(*tag).into(),url:format!("https://github.com/Lake1059/FFmpegFreeUI/releases/download/{tag}/FFmpegFreeUI.x64.exe"),kind:"portable".into(),
                total_bytes:bytes.len()as u64,received_bytes:bytes.len()as u64,status:"completed".into(),path:Some(file.to_string_lossy().into_owned()),
                sha256:Some(format!("{:x}",Sha256::digest(bytes))),verified:false,error:None,created_at:time.clone(),updated_at:time};
            core.store.put_download(&download).unwrap();
            let result = core.install_blocking(&download, &project).unwrap();
            assert_eq!(result["installed"], true);
            project = core.store.project(&project.id).unwrap();
            let installed = PathBuf::from(project.installed_path.as_ref().unwrap());
            assert_eq!(
                fs::read(installed.join("FFmpegFreeUI.x64.exe"))
                    .unwrap()
                    .as_slice(),
                bytes.as_slice()
            );
            if index == 0 {
                old_config = installed.join("settings.json");
                fs::write(&old_config, b"user-settings").unwrap();
            }
        }
        assert_eq!(
            fs::read(&old_config).unwrap().as_slice(),
            b"user-settings".as_slice()
        );
        let project_id = project.id.clone();
        drop(core);
        fs::rename(&original, &moved).unwrap();
        let restored = Core::new(moved.clone(), true, Arc::new(|_, _| {})).unwrap();
        let project = restored.store.project(&project_id).unwrap();
        assert!(Path::new(project.installed_path.as_ref().unwrap())
            .starts_with(fs::canonicalize(&moved).unwrap()));
        for download in restored.store.downloads().unwrap() {
            assert!(Path::new(download.path.as_ref().unwrap()).exists());
        }
    }
    #[tokio::test]
    async fn zip_install_requires_confirmation_and_leaves_manual_software_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().join("data"), true, Arc::new(|_, _| {})).unwrap();
        let manual = dir.path().join("existing-app");
        fs::create_dir(&manual).unwrap();
        fs::write(manual.join("settings.json"), b"keep-manual-settings").unwrap();
        let mut project = test_project("RoutineMiku", "apkstart");
        project.installed_version = Some("v0.3.1".into());
        project.installed_path = Some(manual.to_string_lossy().into_owned());
        project.installed_source = Some("manual".into());
        core.store.put_project(&project).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let folder = downloads::directory(&core.data_dir, &id).unwrap();
        let source = folder.join("ApkStart-win64-portable.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&source).unwrap());
        writer
            .start_file(
                "ApkStart/readme.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"local test data only").unwrap();
        writer.finish().unwrap();
        let bytes = fs::read(&source).unwrap();
        let time = now();
        let download=DownloadRecord{id,project_id:project.id.clone(),project_name:project.name.clone(),asset_id:1,
            file_name:"ApkStart-win64-portable.zip".into(),tag:"v0.3.2".into(),url:"https://github.com/RoutineMiku/apkstart/releases/download/v0.3.2/ApkStart-win64-portable.zip".into(),kind:"portable".into(),
            total_bytes:bytes.len()as u64,received_bytes:bytes.len()as u64,status:"completed".into(),path:Some(source.to_string_lossy().into_owned()),
            sha256:Some(format!("{:x}",Sha256::digest(&bytes))),verified:false,error:None,created_at:time.clone(),updated_at:time};
        core.store.put_download(&download).unwrap();
        assert!(core
            .handle("install_download", json!({"id":download.id}))
            .await
            .is_err());
        assert_eq!(fs::read_dir(core.data_dir.join("apps")).unwrap().count(), 0);
        let result = core
            .handle(
                "install_download",
                json!({"id":download.id,"confirmed":true}),
            )
            .await
            .unwrap();
        assert_eq!(result["installed"], true);
        let installed = PathBuf::from(result["path"].as_str().unwrap());
        assert!(installed.starts_with(core.data_dir.join("apps").join(&project.id)));
        assert_eq!(
            fs::read(installed.join("ApkStart/readme.txt")).unwrap(),
            b"local test data only"
        );
        assert_eq!(
            fs::read(manual.join("settings.json")).unwrap(),
            b"keep-manual-settings"
        );
        assert_eq!(
            core.store.download(&download.id).unwrap().status,
            "installed"
        );
    }
    #[test]
    fn malicious_zip_never_writes_outside_stage() {
        let dir = tempfile::tempdir().unwrap();
        let zip_pathname = dir.path().join("bad.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip_pathname).unwrap());
        writer
            .start_file("../outside.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        writer.finish().unwrap();
        let stage = dir.path().join("stage");
        fs::create_dir(&stage).unwrap();
        assert!(extract_zip(&zip_pathname, &stage).is_err());
        assert!(!dir.path().join("outside.txt").exists());
    }
}

#[cfg(all(test, unix))]
mod unix_archive_tests {
    use super::*;
    #[test]
    fn zip_preserves_executable_permission_without_setuid() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("app.zip");
        let stage = temp.path().join("stage");
        fs::create_dir(&stage).unwrap();
        let file = fs::File::create(&source).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "app/bin/main",
                zip::write::SimpleFileOptions::default().unix_permissions(0o4755),
            )
            .unwrap();
        archive.write_all(b"fixture; not executed").unwrap();
        archive.finish().unwrap();
        extract_zip(&source, &stage).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(stage.join("app/bin/main"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
    }
}
