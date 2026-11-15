use crate::{core::Core, credential, events::Events, github::GithubService};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Session {
    root: PathBuf,
    core: Arc<Core>,
    github: Arc<GithubService>,
    events: Arc<Events>,
    stop: Arc<AtomicBool>,
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
    credential_error: Mutex<Option<String>>,
}
pub struct PluginRuntime {
    runtime: Mutex<Option<Arc<tokio::runtime::Runtime>>>,
    session: Mutex<Option<Arc<Session>>>,
}
impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}
impl PluginRuntime {
    pub fn new() -> Self {
        Self {
            runtime: Mutex::new(None),
            session: Mutex::new(None),
        }
    }
    pub fn invoke(&self, envelope: Value) -> Result<Value, String> {
        let method = envelope["method"].as_str().ok_or("method is required")?;
        if method == "__otools_health__" {
            return Ok(
                json!({"id":"otools-releasehub","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"capabilities":["dispatch","poll_events"]}),
            );
        }
        if method == "__otools_shutdown__" {
            self.shutdown()?;
            return Ok(json!({"stopped":true}));
        }
        if method == "poll_events" {
            return Ok(self
                .session
                .lock()
                .map_err(|_| "Runtime unavailable")?
                .as_ref()
                .map(|session| session.events.drain())
                .unwrap_or_else(|| json!({"events":[]})));
        }
        if method != "dispatch" {
            return Err(format!("Unknown method: {method}"));
        }
        let raw = envelope["pluginDataDir"]
            .as_str()
            .ok_or("Host-selected pluginDataDir is required")?;
        if raw.len() > 8192 || !Path::new(raw).is_absolute() {
            return Err("Plugin data path must be absolute".into());
        }
        std::fs::create_dir_all(raw).map_err(|_| "无法创建插件数据目录")?;
        let root = std::fs::canonicalize(raw).map_err(|_| "无法读取插件数据目录")?;
        let mut runtime = self.runtime.lock().map_err(|_| "Runtime unavailable")?;
        if runtime.is_none() {
            *runtime = Some(Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(3)
                    .enable_all()
                    .build()
                    .map_err(|_| "Unable to start plugin runtime")?,
            ));
        }
        let executor = runtime.as_ref().ok_or("Runtime unavailable")?.clone();
        drop(runtime);
        let runtime = executor;
        let mut session = self.session.lock().map_err(|_| "Runtime unavailable")?;
        if session.as_ref().is_some_and(|session| session.root != root) {
            return Err("Plugin data root cannot change while running".into());
        }
        if session.is_none() {
            let _enter = runtime.enter();
            let events = Arc::new(Events::default());
            let sink = events.clone();
            let core = Core::new(
                root.clone(),
                false,
                Arc::new(move |topic, payload| sink.push(topic, payload)),
            )?;
            // Do not read keyring until this plugin has previously stored its own token.
            let token = if root.join("token-login.enabled").is_file() {
                credential::load(&root)
            } else {
                Ok(None)
            };
            let credential_error = token.as_ref().err().cloned();
            let token = token.unwrap_or(None);
            core.set_github_token(token.clone());
            let plugin_dir = envelope["pluginDir"].as_str().map(PathBuf::from);
            let cli = plugin_dir
                .as_ref()
                .map(|dir| {
                    dir.join("tools")
                        .join(if cfg!(windows) { "gh.exe" } else { "gh" })
                })
                .filter(|path| path.is_absolute() && path.is_file());
            #[cfg(unix)]
            if let Some(path) = &cli {
                use std::os::unix::fs::PermissionsExt;
                let metadata = std::fs::symlink_metadata(path).map_err(|_| "登录组件不可用")?;
                if metadata.file_type().is_symlink() {
                    return Err("登录组件不能是符号链接".into());
                }
                // Host package extractors may omit executable bits; repair only the
                // bundled exact-name component, never arbitrary system binaries.
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|_| "无法准备登录组件权限")?;
            }
            let github = GithubService::with_token(credential::auth_directory(&root)?, cli, token);
            core.set_github_service(github.clone());
            let stop = Arc::new(AtomicBool::new(false));
            let task_stop = stop.clone();
            let task_core = core.clone();
            let task_events = events.clone();
            let scheduler = runtime.spawn(async move {
                let mut last = Instant::now();
                while !task_stop.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    if task_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(settings) = task_core.handle("get_settings", json!({})).await else {
                        continue;
                    };
                    let minutes = settings["checkIntervalMinutes"].as_u64().unwrap_or(0);
                    if minutes == 0 {
                        last = Instant::now();
                        continue;
                    }
                    if last.elapsed().as_secs() < minutes * 60 {
                        continue;
                    }
                    last = Instant::now();
                    if let Ok(result) = task_core.handle("check_updates", json!({})).await {
                        if settings["notifications"] == true
                            && result["updates"].as_u64().unwrap_or(0) > 0
                        {
                            task_events.push(
                                "releasehub:notification",
                                json!({"updates":result["updates"]}),
                            );
                        }
                    }
                }
            });
            *session = Some(Arc::new(Session {
                root: root.clone(),
                core,
                github,
                events,
                stop,
                scheduler: Mutex::new(Some(scheduler)),
                credential_error: Mutex::new(credential_error),
            }));
        }
        let context = session.as_ref().ok_or("Runtime unavailable")?.clone();
        drop(session);
        let session = context;
        let action = envelope
            .pointer("/payload/action")
            .and_then(Value::as_str)
            .ok_or("action is required")?;
        let payload = envelope
            .pointer("/payload/payload")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !payload.is_object()
            || serde_json::to_vec(&payload)
                .map_err(|_| "Invalid payload")?
                .len()
                > 2 * 1024 * 1024
        {
            return Err("Invalid request payload".into());
        }
        match action {
            "snapshot" => {
                let mut value = runtime.block_on(session.core.handle(action, payload))?;
                if let Some(error) = &*session
                    .credential_error
                    .lock()
                    .map_err(|_| "Credential state unavailable")?
                {
                    value["runtime"]["credentialError"] = json!(error);
                }
                Ok(value)
            }
            "set_github_token" => {
                let token = payload["token"].as_str().ok_or("缺少令牌")?.trim();
                credential::validate_token(token)?;
                let marker = root.join("token-login.enabled");
                let had_marker = marker.is_file();
                std::fs::write(&marker, b"1").map_err(|_| "无法保存账号连接标记")?;
                let status = match runtime.block_on(
                    session
                        .github
                        .connect_token(token.into(), |value| credential::save(&root, value)),
                ) {
                    Ok(status) => status,
                    Err(error) => {
                        if !had_marker {
                            let _ = std::fs::remove_file(&marker);
                        }
                        return Err(error);
                    }
                };
                session.core.set_github_token(Some(token.into()));
                *session
                    .credential_error
                    .lock()
                    .map_err(|_| "Credential state unavailable")? = None;
                session.events.push("releasehub:changed", json!({}));
                Ok(status)
            }
            "github_logout" => {
                if root.join("token-login.enabled").is_file() {
                    credential::clear(&root)?;
                    std::fs::remove_file(root.join("token-login.enabled"))
                        .map_err(|_| "无法清理账号连接标记")?;
                }
                session.core.set_github_token(None);
                runtime.block_on(session.github.handle(action, payload))
            }
            "read_text_file" => {
                let path = payload["path"].as_str().ok_or("缺少文件路径")?;
                let meta = std::fs::metadata(path).map_err(|_| "无法读取文件")?;
                if meta.len() > 1024 * 1024 {
                    return Err("订阅文件超过 1 MiB".into());
                }
                Ok(json!(
                    std::fs::read_to_string(path).map_err(|_| "文件不是有效 UTF-8 文本")?
                ))
            }
            "write_export_file" => {
                let path = payload["path"].as_str().ok_or("缺少文件路径")?;
                if !Path::new(path).is_absolute() {
                    return Err("文件路径须为绝对路径".into());
                }
                let contents =
                    runtime.block_on(session.core.handle("export_library", json!({})))?;
                std::fs::write(path, contents.as_str().ok_or("导出失败")?)
                    .map_err(|_| "无法写入导出文件")?;
                Ok(json!({"path":path}))
            }
            "download_path" => {
                let snapshot = runtime.block_on(session.core.handle("snapshot", json!({})))?;
                let id = payload["id"].as_str().ok_or("缺少下载编号")?;
                let record = snapshot["downloads"]
                    .as_array()
                    .and_then(|items| items.iter().find(|value| value["id"] == id))
                    .ok_or("下载不存在")?;
                Ok(json!(record["path"].as_str().ok_or("下载文件尚未完成")?))
            }
            "managed_path" => {
                let versions = runtime.block_on(session.core.handle(
                    "list_managed_versions",
                    json!({"projectId":payload["projectId"]}),
                ))?;
                let item = versions
                    .as_array()
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|value| value["downloadId"] == payload["downloadId"])
                    })
                    .ok_or("受管版本不存在")?;
                Ok(item["path"].clone())
            }
            value if value.starts_with("github_") => {
                runtime.block_on(session.github.handle(value, payload))
            }
            _ => runtime.block_on(session.core.handle(action, payload)),
        }
    }
    pub fn shutdown(&self) -> Result<(), String> {
        let mut runtime = self.runtime.lock().map_err(|_| "Runtime unavailable")?;
        if runtime
            .as_ref()
            .is_some_and(|runtime| Arc::strong_count(runtime) > 1)
        {
            return Err("Plugin requests are still running".into());
        }
        let mut session = self.session.lock().map_err(|_| "Runtime unavailable")?;
        if let Some(old) = session.take() {
            old.stop.store(true, Ordering::Release);
            if let Some(task) = old
                .scheduler
                .lock()
                .map_err(|_| "Scheduler unavailable")?
                .take()
            {
                task.abort();
            }
            if let Ok(downloads) = old.core.active_downloads.lock() {
                for cancel in downloads.values() {
                    cancel.store(true, Ordering::Release);
                }
            }
            if let Some(runtime) = runtime.as_ref() {
                runtime.block_on(old.github.shutdown());
            }
        }
        if let Some(executor) = runtime.take() {
            match Arc::try_unwrap(executor) {
                Ok(executor) => executor.shutdown_timeout(Duration::from_secs(5)),
                Err(active) => {
                    *runtime = Some(active);
                    return Err("Plugin runtime is still busy".into());
                }
            }
        }
        Ok(())
    }
}
impl Drop for PluginRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod concurrent_tests {
    use super::*;
    #[test]
    fn update_check_waiting_for_network_does_not_block_snapshot_or_cancel() {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(PluginRuntime::new());
        let request = |action: &str| json!({"method":"dispatch","pluginDataDir":root.path(),"pluginDir":root.path(),"payload":{"action":action,"payload":{}}});
        runtime.invoke(request("snapshot")).unwrap();
        let core = runtime
            .session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .core
            .clone();
        let gate = core.checks.blocking_lock();
        let rt = runtime.clone();
        let check = request("check_updates");
        let worker = std::thread::spawn(move || rt.invoke(check));
        std::thread::sleep(Duration::from_millis(40));
        let (sender, receiver) = std::sync::mpsc::channel();
        let rt = runtime.clone();
        let snapshot = request("snapshot");
        let reader = std::thread::spawn(move || {
            sender.send(rt.invoke(snapshot)).unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_millis(200));
        drop(gate);
        worker.join().unwrap().unwrap();
        reader.join().unwrap();
        assert!(
            result.is_ok(),
            "an unrelated snapshot was blocked by a check"
        );
        runtime.shutdown().unwrap();
    }
}
