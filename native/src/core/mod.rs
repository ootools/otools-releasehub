mod downloads;
mod install;
mod library;
pub mod models;
mod network;
pub mod selection;
mod store;

use models::*;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

pub type EventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct Core {
    pub(crate) store: store::Store,
    pub(crate) data_dir: PathBuf,
    portable: bool,
    events: EventSink,
    pub(crate) github: network::Github,
    pub(crate) active_downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub(crate) download_queue: Mutex<()>,
    pub(crate) installing: Mutex<std::collections::HashSet<String>>,
    pub(crate) checks: tokio::sync::Mutex<()>,
    // Keep this last so the database and other fields close before the directory lock is released.
    _data_lock: std::fs::File,
}
impl Core {
    pub fn new(data_dir: PathBuf, portable: bool, events: EventSink) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|_| "无法写入数据目录，请将便携文件夹放到可写的位置".to_string())?;
        let data_dir =
            std::fs::canonicalize(data_dir).map_err(|_| "无法读取数据目录".to_string())?;
        let lock_path = data_dir.join(".releasehub.lock");
        match std::fs::symlink_metadata(&lock_path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                return Err("资料库锁文件不能是目录或链接".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err("无法读取资料库锁文件，请检查目录权限".into())
            }
            _ => {}
        }
        let data_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| "无法打开资料库锁文件，请检查目录权限".to_string())?;
        fs2::FileExt::try_lock_exclusive(&data_lock)
            .map_err(|_| "此资料库已被另一个追新实例占用".to_string())?;
        for folder in ["downloads", "apps"] {
            let path = data_dir.join(folder);
            std::fs::create_dir_all(&path).map_err(|_| "无法创建数据文件夹".to_string())?;
            if std::fs::symlink_metadata(&path)
                .map_err(|_| "无法读取数据文件夹".to_string())?
                .file_type()
                .is_symlink()
                || !std::fs::canonicalize(&path)
                    .map_err(|_| "无法读取数据文件夹".to_string())?
                    .starts_with(&data_dir)
            {
                return Err("数据文件夹不能链接到外部位置".into());
            }
        }
        let store = store::Store::new(&data_dir.join("releasehub.sqlite3"))?;
        repair_portable_paths(&store, &data_dir)?;
        Ok(Arc::new(Self {
            store,
            data_dir,
            portable,
            events,
            github: network::Github::new()?,
            active_downloads: Mutex::new(HashMap::new()),
            download_queue: Mutex::new(()),
            installing: Mutex::new(std::collections::HashSet::new()),
            checks: tokio::sync::Mutex::new(()),
            _data_lock: data_lock,
        }))
    }
    pub fn set_github_token(&self, token: Option<String>) {
        self.github.set_token(token);
    }
    pub(crate) fn set_github_service(&self, service: Arc<crate::github::GithubService>) {
        self.github.set_service(service);
    }
    pub(crate) fn changed(&self) {
        (self.events)("releasehub:changed", json!({}));
    }
    pub(crate) fn download_event(&self, record: &DownloadRecord) {
        if let Ok(value) = serde_json::to_value(record) {
            (self.events)("releasehub:download", value);
        }
    }
    pub async fn handle(self: &Arc<Self>, action: &str, payload: Value) -> Result<Value, String> {
        if !payload.is_object() {
            return Err("请求参数须为对象".into());
        }
        if serde_json::to_vec(&payload)
            .map_err(|_| "请求参数无效".to_string())?
            .len()
            > 2 * 1024 * 1024
        {
            return Err("请求内容过大".into());
        }
        match action {
            "get_settings" => output(self.store.settings()?),
            "snapshot" => Ok(
                json!({"projects": self.store.projects()?, "downloads": self.store.downloads()?,
                "settings": self.store.settings()?,
                "runtime": {"appVersion": env!("CARGO_PKG_VERSION"), "dataDir": self.data_dir.to_string_lossy(),
                    "portable": self.portable, "plugin": true, "platform": std::env::consts::OS, "arch": std::env::consts::ARCH, "githubTokenConfigured": self.github.token_configured()}}),
            ),
            "add_project" => {
                let input: AddProjectInput =
                    decode(payload.get("input").cloned().ok_or("缺少项目信息")?)?;
                output(self.add_project(input).await?)
            }
            "update_project" => {
                let id = text(&payload, "id", 128)?;
                let changes: ProjectChanges =
                    decode(payload.get("changes").cloned().ok_or("缺少修改内容")?)?;
                output(self.update_project(&id, changes)?)
            }
            "remove_project" => {
                self.store.remove_project(&text(&payload, "id", 128)?)?;
                self.changed();
                Ok(Value::Null)
            }
            "check_updates" => self.check_updates(&payload).await,
            "download_asset" => output(self.queue_download(
                &text(&payload, "projectId", 128)?,
                integer(&payload, "assetId")?,
            )?),
            "retry_download" => output(self.retry_download(&text(&payload, "id", 128)?)?),
            "cancel_download" => {
                self.cancel_download(&text(&payload, "id", 128)?)?;
                Ok(Value::Null)
            }
            "install_download" => {
                if payload.get("confirmed").and_then(Value::as_bool) != Some(true) {
                    return Err("请先确认安装此软件".into());
                }
                self.install_download(&text(&payload, "id", 128)?).await
            }
            "list_managed_versions" => {
                output(self.list_managed_versions(&text(&payload, "projectId", 128)?)?)
            }
            "activate_managed_version" => {
                if payload.get("confirmed").and_then(Value::as_bool) != Some(true) {
                    return Err("请先确认切换当前受管版本；旧目录和配置将保持不变".into());
                }
                output(self.activate_managed_version(
                    &text(&payload, "projectId", 128)?,
                    &text(&payload, "downloadId", 128)?,
                )?)
            }
            "mark_installed" => {
                let id = text(&payload, "projectId", 128)?;
                let version = text(&payload, "version", 200)?;
                let path = optional_text(&payload, "path", 4096)?;
                let p = self.store.mutate("projects", &id, |p: &mut Project| {
                    p.installed_version = Some(version);
                    p.installed_path = path;
                    p.installed_source = Some("manual".into());
                    selection::update_status(p);
                    Ok(())
                })?;
                self.changed();
                output(p)
            }
            "save_settings" => {
                let patch = payload
                    .get("settings")
                    .and_then(Value::as_object)
                    .ok_or("设置内容无效")?;
                let mut value = output(self.store.settings()?)?;
                for (k, v) in patch {
                    value
                        .as_object_mut()
                        .ok_or("设置内容无效")?
                        .insert(k.clone(), v.clone());
                }
                let settings: Settings = decode(value)?;
                self.store.save_settings(&settings)?;
                self.pump_downloads();
                self.changed();
                output(settings)
            }
            "export_library" => Ok(Value::String(self.export_library()?)),
            "import_library" => self.import_library(&text(&payload, "contents", 1024 * 1024)?),
            _ => Err("不支持此操作".into()),
        }
    }
}
fn repair_portable_paths(store: &store::Store, data: &std::path::Path) -> Result<(), String> {
    for mut d in store.downloads()? {
        if !["completed", "installed"].contains(&d.status.as_str()) {
            continue;
        }
        if uuid::Uuid::parse_str(&d.id).is_err()
            || downloads::safe_name(&d.file_name) != d.file_name
        {
            return Err("本地下载记录路径无效".into());
        }
        let candidate = data.join("downloads").join(&d.id).join(&d.file_name);
        let safe = std::fs::symlink_metadata(&candidate)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            && std::fs::canonicalize(&candidate)
                .is_ok_and(|p| p.starts_with(data.join("downloads")));
        if safe {
            d.path = Some(candidate.to_string_lossy().into_owned());
        } else {
            d.status = "failed".into();
            d.path = None;
            d.error = Some("下载文件缺失或路径不安全，请重新下载".into());
        }
        store.put_download(&d)?;
    }
    for mut p in store.projects()? {
        if p.installed_source.as_deref() != Some("managed") {
            continue;
        }
        if uuid::Uuid::parse_str(&p.id).is_err() {
            return Err("本地受管项目编号无效".into());
        }
        let leaf = p
            .installed_path
            .as_deref()
            .and_then(|s| std::path::Path::new(s).file_name())
            .and_then(|s| s.to_str());
        if let Some(leaf) = leaf.filter(|s| downloads::safe_name(s) == *s) {
            let candidate = data.join("apps").join(&p.id).join(leaf);
            if std::fs::symlink_metadata(&candidate)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                && std::fs::canonicalize(&candidate)
                    .is_ok_and(|q| q.starts_with(data.join("apps").join(&p.id)))
            {
                p.installed_path = Some(candidate.to_string_lossy().into_owned());
                store.put_project(&p)?;
            } else {
                p.error = Some("受管程序目录缺失，请从已下载文件重新安装".into());
                store.put_project(&p)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn output<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "数据格式转换失败".into())
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "请求字段或格式无效".into())
}
pub(crate) fn text(payload: &Value, key: &str, max: usize) -> Result<String, String> {
    bounded_text(
        payload
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("缺少{}参数", key))?,
        max,
        key,
    )
}
pub(crate) fn optional_text(
    payload: &Value,
    key: &str,
    max: usize,
) -> Result<Option<String>, String> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(v)) if v.trim().is_empty() => Ok(None),
        Some(Value::String(v)) => bounded_text(v, max, key).map(Some),
        _ => Err(format!("{}格式无效", key)),
    }
}
pub(crate) fn integer(payload: &Value, key: &str) -> Result<u64, String> {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{}须为正整数", key))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queued_download(project: &Project) -> DownloadRecord {
        let time = now();
        DownloadRecord {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            asset_id: 1,
            file_name: "fixture.zip".into(),
            tag: "v1.0.0".into(),
            url: "https://github.com/fixture/project/releases/download/v1.0.0/fixture.zip".into(),
            kind: "portable".into(),
            total_bytes: 10,
            received_bytes: 0,
            status: "queued".into(),
            path: None,
            sha256: None,
            verified: false,
            error: None,
            created_at: time.clone(),
            updated_at: time,
        }
    }
    #[test]
    fn data_lock_rejects_second_core_before_recovery_and_releases_on_last_drop() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("data");
        let core = Core::new(data.clone(), true, Arc::new(|_, _| {})).unwrap();
        let mut project = test_project("fixture", "project");
        project.status = "checking".into();
        core.store.put_project(&project).unwrap();
        let download = queued_download(&project);
        core.store.put_download(&download).unwrap();
        let started = std::time::Instant::now();
        let error = Core::new(data.join("."), true, Arc::new(|_, _| {}))
            .err()
            .unwrap();
        assert!(error.contains("已被另一个追新实例占用"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(core.store.download(&download.id).unwrap().status, "queued");
        assert_eq!(core.store.project(&project.id).unwrap().status, "checking");
        let retained = core.clone();
        drop(core);
        assert!(Core::new(data.clone(), true, Arc::new(|_, _| {})).is_err());
        drop(retained);
        let reopened = Core::new(data.clone(), true, Arc::new(|_, _| {})).unwrap();
        assert_eq!(
            reopened.store.download(&download.id).unwrap().status,
            "failed"
        );
        assert_eq!(
            reopened.store.project(&project.id).unwrap().status,
            "notChecked"
        );
        drop(reopened);
        assert!(data.join(".releasehub.lock").is_file());
    }
    #[test]
    fn data_locks_allow_independent_directories() {
        let directory = tempfile::tempdir().unwrap();
        let first = Core::new(directory.path().join("first"), true, Arc::new(|_, _| {})).unwrap();
        let second = Core::new(directory.path().join("second"), true, Arc::new(|_, _| {})).unwrap();
        let mut settings = Settings::default();
        settings.theme = "dark".into();
        first.store.save_settings(&settings).unwrap();
        assert_eq!(first.store.settings().unwrap().theme, "dark");
        assert_eq!(second.store.settings().unwrap().theme, "system");
    }
    #[tokio::test]
    async fn get_settings_returns_only_preferences_without_library_or_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::new(directory.path().join("data"), true, Arc::new(|_, _| {})).unwrap();
        let project = test_project("fixture", "private-test-marker");
        core.store.put_project(&project).unwrap();
        let download = queued_download(&project);
        core.store.put_download(&download).unwrap();
        core.set_github_token(Some("local-test-token-not-a-credential".into()));
        let mut settings = Settings::default();
        settings.theme = "dark".into();
        settings.check_interval_minutes = 0;
        core.store.save_settings(&settings).unwrap();
        let result = core.handle("get_settings", json!({})).await.unwrap();
        assert_eq!(result, serde_json::to_value(&settings).unwrap());
        for field in [
            "projects",
            "downloads",
            "buildTasks",
            "runtime",
            "githubToken",
            "agentToken",
        ] {
            assert!(result.get(field).is_none(), "{field}");
        }
        assert!(!result
            .to_string()
            .contains("local-test-token-not-a-credential"));
        assert_eq!(core.store.download(&download.id).unwrap().status, "queued");
    }
}
