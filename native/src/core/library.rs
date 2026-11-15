use super::{models::*, network, selection, Core};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Subscription {
    repository: String,
    channel: String,
    preference: String,
    asset_pattern: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Library {
    version: u32,
    projects: Vec<Subscription>,
    settings: LibrarySettings,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibrarySettings {
    theme: String,
    check_interval_minutes: u32,
    notifications: bool,
    auto_download: bool,
    max_concurrent_downloads: usize,
}
fn empty_project(owner: String, repo: String, channel: String, preference: String) -> Project {
    let time = now();
    Project {
        id: uuid::Uuid::new_v4().to_string(),
        name: repo.clone(),
        url: format!("https://github.com/{owner}/{repo}"),
        owner,
        repo,
        description: String::new(),
        avatar_url: None,
        channel,
        preference,
        asset_pattern: None,
        installed_version: None,
        installed_path: None,
        installed_source: None,
        release: None,
        selected_asset_id: None,
        status: "notChecked".into(),
        checked_at: None,
        error: None,
        default_branch: "main".into(),
        source_head_sha: None,
        source_ahead: None,
        created_at: time,
    }
}
fn metadata(p: &mut Project, raw: &Value) -> Result<(), String> {
    if raw.get("private").and_then(Value::as_bool) != Some(false) {
        return Err("只支持公开 GitHub 项目".into());
    }
    p.name = network::field(raw, "name", 200)?;
    p.default_branch = network::field(raw, "default_branch", 200)?;
    p.description = raw
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    p.avatar_url = raw
        .pointer("/owner/avatar_url")
        .and_then(Value::as_str)
        .filter(|s| {
            url::Url::parse(s).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("avatars.githubusercontent.com")
                    && u.username().is_empty()
                    && u.password().is_none()
            })
        })
        .map(str::to_owned);
    Ok(())
}
impl Core {
    pub(crate) async fn add_project(&self, input: AddProjectInput) -> Result<Project, String> {
        validate_choices(&input.channel, &input.preference)?;
        let (owner, repo) = network::repository(&input.repository)?;
        let mut p = empty_project(owner, repo, input.channel, input.preference);
        let raw = self.github.api(&p.owner, &p.repo, &[], None).await?;
        metadata(&mut p, &raw)?;
        p.installed_version = input
            .installed_version
            .map(|s| bounded_text(&s, 200, "已安装版本"))
            .transpose()?;
        p.installed_path = input
            .installed_path
            .map(|s| bounded_text(&s, 4096, "已安装路径"))
            .transpose()?;
        if p.installed_version.is_some() {
            p.installed_source = Some("manual".into());
        }
        if !self.store.insert_project_unique(&p)? {
            return Err("项目已在列表中".into());
        }
        self.changed();
        Ok(p)
    }
    pub(crate) fn update_project(
        &self,
        id: &str,
        changes: ProjectChanges,
    ) -> Result<Project, String> {
        let p = self.store.mutate("projects", id, |p: &mut Project| {
            if let Some(c) = changes.channel {
                p.channel = c;
                p.release = None;
                p.selected_asset_id = None;
                p.status = "notChecked".into();
            }
            if let Some(v) = changes.preference {
                p.preference = v;
            }
            validate_choices(&p.channel, &p.preference)?;
            if let Some(pattern) = changes.asset_pattern {
                p.asset_pattern = pattern
                    .map(|s| bounded_text(&s, 200, "文件名规则"))
                    .transpose()?;
            }
            if let Some(v) = changes.installed_version {
                p.installed_version = v.map(|s| bounded_text(&s, 200, "已安装版本")).transpose()?;
                p.installed_source = p.installed_version.as_ref().map(|_| "manual".into());
            }
            if let Some(path) = changes.installed_path {
                p.installed_path = path
                    .map(|s| bounded_text(&s, 4096, "已安装路径"))
                    .transpose()?;
            }
            if let Some(r) = &mut p.release {
                for a in &mut r.assets {
                    selection::classify_for_project(
                        a,
                        &p.preference,
                        p.asset_pattern.as_deref(),
                        &p.owner,
                        &p.repo,
                    );
                }
                p.selected_asset_id = selection::choose(&r.assets);
            }
            if let Some(selected) = changes.selected_asset_id {
                if let Some(id) = selected {
                    if !p
                        .release
                        .as_ref()
                        .is_some_and(|r| r.assets.iter().any(|a| a.id == id && a.compatible))
                    {
                        return Err("附件不适用于当前项目或系统架构".into());
                    }
                }
                p.selected_asset_id = selected;
            }
            if p.release.is_some() {
                selection::update_status(p);
            }
            p.error = None;
            Ok(())
        })?;
        self.changed();
        Ok(p)
    }
    pub(crate) async fn check_updates(self: &Arc<Self>, payload: &Value) -> Result<Value, String> {
        let _guard = self.checks.lock().await;
        let allow_auto = match payload.get("allowAutoDownload") {
            None => true,
            Some(Value::Bool(v)) => *v,
            _ => return Err("自动下载参数无效".into()),
        };
        let ids: Option<Vec<String>> = match payload.get("projectIds") {
            None => None,
            Some(v) => Some(super::decode(v.clone())?),
        };
        if ids.as_ref().is_some_and(|v| v.len() > 1000) {
            return Err("一次最多检查1000个项目".into());
        }
        let projects = self.store.projects()?;
        if let Some(ids) = &ids {
            for id in ids {
                if !projects.iter().any(|p| &p.id == id) {
                    return Err("要检查的项目不存在".into());
                }
            }
        }
        let mut checked = 0;
        let mut updates = 0;
        let mut errors = 0;
        for mut p in projects
            .into_iter()
            .filter(|p| ids.as_ref().is_none_or(|v| v.contains(&p.id)))
        {
            let id = p.id.clone();
            self.store
                .mutate("projects", &id, |current: &mut Project| {
                    current.status = "checking".into();
                    current.error = None;
                    Ok(())
                })?;
            self.changed();
            let fetched = async {
                let raw = self.github.api(&p.owner, &p.repo, &[], None).await?;
                metadata(&mut p, &raw)?;
                let release = self.github.release(&p).await?;
                let head = self
                    .github
                    .head(&p.owner, &p.repo, &p.default_branch)
                    .await
                    .ok();
                let tag_commit = if let Some(r) = &release {
                    self.github.head(&p.owner, &p.repo, &r.tag).await.ok()
                } else {
                    None
                };
                Ok::<_, String>((release, head, tag_commit))
            }
            .await;
            checked += 1;
            let updated = self.store.mutate("projects", &id, |current: &mut Project| {
                current.checked_at = Some(now());
                match fetched {
                    Ok((mut release, head, tag_commit)) => {
                        // A concurrent user change of channel must not receive data from the previous channel.
                        if current.channel != p.channel {
                            current.status = "notChecked".into();
                            return Ok(());
                        }
                        current.name = p.name;
                        current.description = p.description;
                        current.avatar_url = p.avatar_url;
                        current.default_branch = p.default_branch;
                        if let Some(r) = &mut release {
                            for a in &mut r.assets {
                                selection::classify_for_project(
                                    a,
                                    &current.preference,
                                    current.asset_pattern.as_deref(),
                                    &current.owner,
                                    &current.repo,
                                );
                            }
                        }
                        let previous = current.selected_asset_id;
                        current.selected_asset_id = release.as_ref().and_then(|r| {
                            previous
                                .filter(|id| r.assets.iter().any(|a| a.id == *id && a.compatible))
                                .or_else(|| selection::choose(&r.assets))
                        });
                        current.release = release;
                        current.source_ahead =
                            head.as_ref().zip(tag_commit.as_ref()).map(|(a, b)| a != b);
                        current.source_head_sha = head;
                        current.error = None;
                        selection::update_status(current);
                    }
                    Err(error) => {
                        current.status = "error".into();
                        current.error = Some(error);
                        errors += 1;
                    }
                }
                Ok(())
            });
            let updated = match updated {
                Ok(p) => p,
                Err(e) if self.store.project(&id).is_err() => {
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if updated.status == "updateAvailable" {
                updates += 1;
                if allow_auto && self.store.settings()?.auto_download {
                    if let (Some(asset), Some(r)) =
                        (updated.selected_asset_id, updated.release.as_ref())
                    {
                        let exists = self.store.downloads()?.iter().any(|d| {
                            d.project_id == updated.id && d.asset_id == asset && d.tag == r.tag
                        });
                        if !exists {
                            if let Err(e) = self.queue_download(&updated.id, asset) {
                                self.store.mutate("projects", &id, |p: &mut Project| {
                                    p.error = Some(format!("自动下载未开始：{e}"));
                                    Ok(())
                                })?;
                            }
                        }
                    }
                }
            }
            self.changed();
        }
        Ok(json!({"checked":checked,"updates":updates,"errors":errors}))
    }
    pub(crate) fn export_library(&self) -> Result<String, String> {
        let s = self.store.settings()?;
        let library = Library {
            version: 1,
            projects: self
                .store
                .projects()?
                .into_iter()
                .map(|p| Subscription {
                    repository: format!("{}/{}", p.owner, p.repo),
                    channel: p.channel,
                    preference: p.preference,
                    asset_pattern: p.asset_pattern,
                })
                .collect(),
            settings: LibrarySettings {
                theme: s.theme,
                check_interval_minutes: s.check_interval_minutes,
                notifications: s.notifications,
                auto_download: s.auto_download,
                max_concurrent_downloads: s.max_concurrent_downloads,
            },
        };
        serde_json::to_string_pretty(&library).map_err(|_| "订阅导出失败".into())
    }
    pub(crate) fn import_library(&self, contents: &str) -> Result<Value, String> {
        if contents.len() > 1024 * 1024 {
            return Err("订阅文件超过1MB限制".into());
        }
        let library: Library = serde_json::from_str(contents)
            .map_err(|_| "订阅文件格式无效或含不支持的字段".to_string())?;
        if library.version != 1 || library.projects.len() > 1000 {
            return Err("订阅文件版本无效或项目过多".into());
        }
        let mut settings = self.store.settings()?;
        settings.theme = library.settings.theme;
        settings.check_interval_minutes = library.settings.check_interval_minutes;
        settings.notifications = library.settings.notifications;
        settings.auto_download = library.settings.auto_download;
        settings.max_concurrent_downloads = library.settings.max_concurrent_downloads;
        settings.validate()?;
        let mut ready = Vec::new();
        for input in library.projects {
            validate_choices(&input.channel, &input.preference)?;
            let (owner, repo) = network::repository(&input.repository)?;
            let mut p = empty_project(owner, repo, input.channel, input.preference);
            p.asset_pattern = input
                .asset_pattern
                .map(|s| bounded_text(&s, 200, "文件名规则"))
                .transpose()?;
            ready.push(p);
        }
        let (mut imported, mut skipped) = (0, 0);
        for p in ready {
            if self.store.insert_project_unique(&p)? {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        self.store.save_settings(&settings)?;
        self.changed();
        Ok(json!({"imported":imported,"skipped":skipped}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_only_subscriptions() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().into(), true, Arc::new(|_, _| {})).unwrap();
        let mut p = empty_project(
            "owner".into(),
            "repo".into(),
            "stable".into(),
            "portable".into(),
        );
        p.installed_path = Some("C:\\private\\app".into());
        p.installed_version = Some("1.2.0".into());
        core.store.put_project(&p).unwrap();
        let mut settings = core.store.settings().unwrap();
        settings.auto_download = true;
        core.store.save_settings(&settings).unwrap();
        core.set_github_token(Some("private-test-token".into()));
        let export = core.export_library().unwrap();
        assert!(!export.contains("private-test-token"));
        assert!(!export.contains("installedPath"));
        assert!(!export.contains("installedVersion"));
        assert!(!export.contains("buildTasks"));
        assert!(!export.contains("agentPort"));
        assert!(!export.contains("agentEnabled"));
        assert_eq!(core.import_library(&export).unwrap()["skipped"], 1);
        let imported =
            Core::new(dir.path().join("fresh-import"), true, Arc::new(|_, _| {})).unwrap();
        assert_eq!(imported.import_library(&export).unwrap()["imported"], 1);
        assert!(imported.store.settings().unwrap().auto_download);
        assert!(!imported.github.token_configured());
        assert!(imported.store.downloads().unwrap().is_empty());
        let restored = &imported.store.projects().unwrap()[0];
        assert!(restored.installed_path.is_none());
        assert!(restored.installed_version.is_none());
    }
    #[test]
    fn import_rejects_secrets_and_unknown_schema() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().into(), true, Arc::new(|_, _| {})).unwrap();
        let mut v: Value = serde_json::from_str(&core.export_library().unwrap()).unwrap();
        v["token"] = json!("not-a-real-secret");
        assert!(core.import_library(&v.to_string()).is_err());
    }
}
