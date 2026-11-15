use serde::{Deserialize, Serialize};

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub url: String,
    pub content_type: String,
    pub digest: Option<String>,
    pub kind: String,
    pub compatible: bool,
    pub score: i32,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub id: u64,
    pub tag: String,
    pub name: String,
    pub body: String,
    pub published_at: String,
    pub url: String,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub avatar_url: Option<String>,
    pub channel: String,
    pub preference: String,
    pub asset_pattern: Option<String>,
    pub installed_version: Option<String>,
    pub installed_path: Option<String>,
    pub installed_source: Option<String>,
    pub release: Option<Release>,
    pub selected_asset_id: Option<u64>,
    pub status: String,
    pub checked_at: Option<String>,
    pub error: Option<String>,
    pub default_branch: String,
    pub source_head_sha: Option<String>,
    pub source_ahead: Option<bool>,
    pub created_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub asset_id: u64,
    pub file_name: String,
    pub tag: String,
    pub url: String,
    pub kind: String,
    pub total_bytes: u64,
    pub received_bytes: u64,
    pub status: String,
    pub path: Option<String>,
    pub sha256: Option<String>,
    pub verified: bool,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedVersion {
    pub download_id: String,
    pub version: String,
    pub path: String,
    pub active: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub theme: String,
    pub check_interval_minutes: u32,
    pub notifications: bool,
    pub auto_download: bool,
    pub max_concurrent_downloads: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            check_interval_minutes: 60,
            notifications: true,
            auto_download: false,
            max_concurrent_downloads: 2,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err("主题设置无效".into());
        }
        if self.check_interval_minutes != 0 && !(5..=10080).contains(&self.check_interval_minutes) {
            return Err("检查间隔须为0（仅手动检查）或5至10080分钟".into());
        }
        if !(1..=4).contains(&self.max_concurrent_downloads) {
            return Err("同时下载数量须为1至4".into());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddProjectInput {
    pub repository: String,
    pub channel: String,
    pub preference: String,
    pub installed_version: Option<String>,
    pub installed_path: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChanges {
    pub channel: Option<String>,
    pub preference: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub asset_pattern: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub installed_version: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub installed_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub selected_asset_id: Option<Option<u64>>,
}
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub fn validate_choices(channel: &str, preference: &str) -> Result<(), String> {
    if !["stable", "preview"].contains(&channel) {
        return Err("版本通道无效".into());
    }
    if !["auto", "portable", "installer"].contains(&preference) {
        return Err("安装包偏好无效".into());
    }
    Ok(())
}

#[cfg(test)]
pub fn test_project(owner: &str, repo: &str) -> Project {
    Project {
        id: uuid::Uuid::new_v4().to_string(),
        owner: owner.into(),
        repo: repo.into(),
        name: repo.into(),
        description: String::new(),
        url: format!("https://github.com/{owner}/{repo}"),
        avatar_url: None,
        channel: "stable".into(),
        preference: "auto".into(),
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
        created_at: now(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_checks_and_bounded_intervals_are_valid() {
        let mut settings = Settings::default();
        for minutes in [0, 5, 60, 10080] {
            settings.check_interval_minutes = minutes;
            assert!(settings.validate().is_ok(), "{minutes}");
        }
        for minutes in [1, 4, 10081, u32::MAX] {
            settings.check_interval_minutes = minutes;
            assert!(settings.validate().is_err(), "{minutes}");
        }
    }
    #[test]
    fn optional_updates_preserve_omission_and_allow_explicit_clear() {
        let absent: ProjectChanges = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(absent.installed_version.is_none());
        let cleared: ProjectChanges =
            serde_json::from_value(serde_json::json!({"installedVersion":null})).unwrap();
        assert_eq!(cleared.installed_version, Some(None));
        let value = serde_json::to_value(test_project("owner", "repo")).unwrap();
        assert!(value.get("installedVersion").is_some());
        assert!(value.get("installed_version").is_none());
    }
}
pub fn bounded_text(value: &str, max: usize, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.chars().any(|c| c == '\0') {
        return Err(format!("{}为空或过长", label));
    }
    Ok(value.to_owned())
}
