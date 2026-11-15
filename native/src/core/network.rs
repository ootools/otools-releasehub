use super::{models::*, selection};
use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, Semaphore};
use url::Url;

pub const MAX_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_API_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone)]
struct Cached {
    json: Value,
    etag: Option<String>,
    until: Instant,
}
pub struct Github {
    client: Client,
    token: RwLock<Option<String>>,
    service: RwLock<Option<Arc<crate::github::GithubService>>>,
    cache: Mutex<HashMap<String, Cached>>,
    request_slots: Semaphore,
    next_request: Mutex<Instant>,
    authorization_changed: Notify,
}
pub fn trusted_download_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "下载地址格式无效".to_string())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.as_str().split("://").nth(1).is_some_and(|part| {
            part.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        || url.port().is_some_and(|p| p != 443)
        || url.fragment().is_some()
    {
        return Err("仅允许无凭据的 GitHub HTTPS 下载地址".into());
    }
    let trusted = match url.host_str() {
        Some("github.com") => {
            let parts: Vec<_> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
            parts.len() >= 6
                && parts[2] == "releases"
                && parts[3] == "download"
                && url.query().is_none()
        }
        Some(
            "release-assets.githubusercontent.com"
            | "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com",
        ) => true,
        _ => false,
    };
    if !trusted {
        return Err("下载地址不在可信 GitHub 发行域名内".into());
    }
    Ok(url)
}
fn validated_result_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "编译结果地址无效".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.as_str().split("://").nth(1).is_some_and(|part| {
            part.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        || url.port().is_some_and(|p| p != 443)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("结果须为无凭据、无查询参数的 GitHub HTTPS 发行或 Actions 地址".into());
    }
    let parts: Vec<_> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
    if parts.len() < 5
        || repository(&format!("{}/{}", parts[0], parts[1])).is_err()
        || parts.iter().any(|p| p.is_empty())
        || !((parts[2] == "releases"
            && (parts[3] == "tag" || (parts[3] == "download" && parts.len() >= 6)))
            || (parts[2] == "actions"
                && parts[3] == "runs"
                && parts[4].bytes().all(|b| b.is_ascii_digit())))
    {
        return Err("结果须指向 GitHub 发行页、发行附件或 Actions 构建".into());
    }
    Ok(url)
}
#[cfg(test)]
pub fn github_build_result_url(value: &str) -> Result<(), String> {
    validated_result_url(value).map(|_| ())
}
pub fn github_result_url(value: &str, owner: &str, repo: &str) -> Result<(), String> {
    let url = validated_result_url(value)?;
    let parts: Vec<_> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
    if !parts[0].eq_ignore_ascii_case(owner) || !parts[1].eq_ignore_ascii_case(repo) {
        return Err("正式发行须指向当前项目的 GitHub 仓库".into());
    }
    Ok(())
}
pub fn repository(value: &str) -> Result<(String, String), String> {
    let trimmed = value.trim().trim_end_matches('/');
    let source = if trimmed.starts_with("https://") {
        let u = Url::parse(trimmed).map_err(|_| "项目地址格式无效".to_string())?;
        if u.host_str() != Some("github.com")
            || !u.username().is_empty()
            || u.as_str().split("://").nth(1).is_some_and(|part| {
                part.split('/')
                    .next()
                    .is_some_and(|authority| authority.contains('@'))
            })
            || u.port().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err("请填写公开 GitHub 项目地址".into());
        }
        u.path().trim_matches('/').to_owned()
    } else {
        trimmed.to_owned()
    };
    let parts: Vec<_> = source.split('/').collect();
    if parts.len() != 2 {
        return Err("请填写 owner/repo 或 GitHub 项目主页地址".into());
    }
    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    if owner.is_empty()
        || owner.len() > 100
        || repo.is_empty()
        || repo.len() > 100
        || !owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || [b'-', b'_', b'.'].contains(&b))
        || [".", ".."].contains(&repo)
    {
        return Err("GitHub 项目名称无效".into());
    }
    Ok((owner.into(), repo.into()))
}
pub fn request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "网络请求超时，请稍后重试"
    } else if error.is_connect() {
        "无法连接 GitHub，请检查网络或代理"
    } else if error.is_redirect() {
        "下载被重定向到不可信地址，已停止"
    } else {
        "GitHub 网络请求失败，请稍后重试"
    }
    .into()
}
fn api_resource_available(status: StatusCode) -> Result<bool, String> {
    match status {
        StatusCode::NOT_FOUND => Ok(false),
        StatusCode::UNAUTHORIZED => Err("GitHub 访问令牌无效，请重新配置".into()),
        StatusCode::FORBIDDEN => Err("GitHub 拒绝访问，请检查网络或令牌权限".into()),
        _ if status.is_success() => Ok(true),
        _ => Err(if status.is_redirection() {
            "项目地址已变更，请重新添加项目"
        } else {
            "GitHub 暂时无法提供数据，请稍后重试"
        }
        .into()),
    }
}
impl Github {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!(
                "ReleaseHub/",
                env!("CARGO_PKG_VERSION"),
                " public-release-manager"
            ))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|_| "网络服务初始化失败".to_string())?;
        Ok(Self {
            client,
            token: RwLock::new(None),
            service: RwLock::new(None),
            cache: Mutex::new(HashMap::new()),
            request_slots: Semaphore::new(3),
            next_request: Mutex::new(Instant::now()),
            authorization_changed: Notify::new(),
        })
    }
    pub(crate) fn set_service(&self, service: Arc<crate::github::GithubService>) {
        if let Ok(mut current) = self.service.write() {
            *current = Some(service);
        }
    }
    pub fn set_token(&self, token: Option<String>) {
        let token = token.filter(|t| {
            !t.trim().is_empty() && t.len() <= 4096 && t.bytes().all(|b| b.is_ascii_graphic())
        });
        let Ok(mut current) = self.token.write() else {
            return;
        };
        if *current == token {
            return;
        }
        *current = token;
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        if let Ok(mut next) = self.next_request.lock() {
            *next = Instant::now();
        }
        drop(current);
        self.authorization_changed.notify_waiters();
    }
    pub fn token_configured(&self) -> bool {
        self.token.read().map(|t| t.is_some()).unwrap_or(false)
    }
    async fn wait_for_request(&self) -> Result<(), String> {
        loop {
            let changed = self.authorization_changed.notified();
            let wait = {
                let mut next = self
                    .next_request
                    .lock()
                    .map_err(|_| "网络队列不可用".to_string())?;
                let now = Instant::now();
                let wait = next.saturating_duration_since(now);
                if wait > Duration::from_secs(60) {
                    return Err("GitHub 请求额度已用完，请稍后重试，或到 GitHub 页面登录".into());
                }
                if wait.is_zero() {
                    *next = now + Duration::from_millis(350);
                    return Ok(());
                }
                wait
            };
            // No lock is held while waiting; changing credentials wakes the old cooldown immediately.
            tokio::select! {_=tokio::time::sleep(wait)=>{},_=changed=>{}}
        }
    }
    fn defer_requests(&self, token: &Option<String>, seconds: u64) -> Result<bool, String> {
        let current = self.token.read().map_err(|_| "凭据读取失败".to_string())?;
        if *current != *token {
            return Ok(false);
        }
        *self
            .next_request
            .lock()
            .map_err(|_| "网络队列不可用".to_string())? =
            Instant::now() + Duration::from_secs(seconds);
        Ok(true)
    }
    pub async fn api(
        &self,
        owner: &str,
        repo: &str,
        tail: &[&str],
        page: Option<u32>,
    ) -> Result<Value, String> {
        self.api_optional(owner, repo, tail, page)
            .await?
            .ok_or_else(|| "公开项目或指定版本不存在".into())
    }
    async fn api_optional(
        &self,
        owner: &str,
        repo: &str,
        tail: &[&str],
        page: Option<u32>,
    ) -> Result<Option<Value>, String> {
        let service = self
            .service
            .read()
            .map_err(|_| "GitHub 连接暂时不可用".to_string())?
            .clone();
        if let Some(service) = service {
            let _slot = self
                .request_slots
                .acquire()
                .await
                .map_err(|_| "网络队列已关闭".to_string())?;
            // A connected account bypasses old anonymous cooldown/cache. Never copy its credential.
            if let Some(result) = service.repository_api(owner, repo, tail, page).await {
                return result;
            }
        }
        let mut url =
            Url::parse("https://api.github.com").map_err(|_| "GitHub 地址无效".to_string())?;
        {
            let mut p = url
                .path_segments_mut()
                .map_err(|_| "GitHub 地址无效".to_string())?;
            p.extend(["repos", owner, repo]);
            p.extend(tail.iter().copied());
        }
        if let Some(n) = page {
            url.query_pairs_mut()
                .append_pair("per_page", "100")
                .append_pair("page", &n.to_string());
        }
        let key = url.to_string();
        let cached = self
            .cache
            .lock()
            .map_err(|_| "网络缓存读取失败".to_string())?
            .get(&key)
            .cloned();
        if let Some(c) = &cached {
            if c.until > Instant::now() {
                return Ok(Some(c.json.clone()));
            }
        }
        let _slot = self
            .request_slots
            .acquire()
            .await
            .map_err(|_| "网络队列已关闭".to_string())?;
        for attempt in 0..3 {
            self.wait_for_request().await?;
            let token = self
                .token
                .read()
                .map_err(|_| "凭据读取失败".to_string())?
                .clone();
            let mut request = self
                .client
                .get(url.clone())
                .header(header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(t) = &token {
                request = request.bearer_auth(t);
            }
            if let Some(etag) = cached.as_ref().and_then(|c| c.etag.as_ref()) {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            let response = request.send().await.map_err(|e| request_error(&e))?;
            let status = response.status();
            if status == StatusCode::NOT_MODIFIED {
                if let Some(mut c) = cached.clone() {
                    c.until = Instant::now() + Duration::from_secs(60);
                    let current = self.token.read().map_err(|_| "凭据读取失败".to_string())?;
                    if *current == token {
                        self.cache
                            .lock()
                            .map_err(|_| "网络缓存写入失败".to_string())?
                            .insert(key.clone(), c.clone());
                    }
                    return Ok(Some(c.json));
                }
                return Err("GitHub 缓存响应无效，请重试".into());
            }
            let exhausted = response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|h| h.to_str().ok())
                == Some("0");
            if status == StatusCode::TOO_MANY_REQUESTS
                || (status == StatusCode::FORBIDDEN && exhausted)
            {
                let retry = response
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());
                let reset = response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<i64>().ok());
                let seconds = retry
                    .or_else(|| reset.map(|s| (s - chrono::Utc::now().timestamp()).max(1) as u64))
                    .unwrap_or(60)
                    .min(3600);
                if !self.defer_requests(&token, seconds)? {
                    continue;
                }
                return Err("GitHub 请求额度已用完，请稍后重试，或到 GitHub 页面登录".into());
            }
            if status.is_server_error() && attempt < 2 {
                tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                continue;
            }
            if !api_resource_available(status)? {
                return Ok(None);
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_API_BYTES as u64)
            {
                return Err("GitHub 返回的数据过大".into());
            }
            let etag = response
                .headers()
                .get(header::ETAG)
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned);
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| request_error(&e))?;
                if bytes.len() + chunk.len() > MAX_API_BYTES {
                    return Err("GitHub 返回的数据过大".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let json: Value =
                serde_json::from_slice(&bytes).map_err(|_| "GitHub 数据格式无效".to_string())?;
            let current = self.token.read().map_err(|_| "凭据读取失败".to_string())?;
            if *current == token {
                let mut cache = self
                    .cache
                    .lock()
                    .map_err(|_| "网络缓存写入失败".to_string())?;
                if cache.len() >= 128 {
                    cache.clear();
                }
                cache.insert(
                    key.clone(),
                    Cached {
                        json: json.clone(),
                        etag,
                        until: Instant::now() + Duration::from_secs(60),
                    },
                );
            }
            return Ok(Some(json));
        }
        Err("GitHub 暂时不可用".into())
    }
    pub async fn head(&self, owner: &str, repo: &str, reference: &str) -> Result<String, String> {
        let j = self.api(owner, repo, &["commits", reference], None).await?;
        let sha = j
            .get("sha")
            .and_then(Value::as_str)
            .ok_or("无法确定源码版本")?;
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("GitHub 源码版本格式无效".into());
        }
        Ok(sha.to_ascii_lowercase())
    }
    pub async fn release(&self, p: &Project) -> Result<Option<Release>, String> {
        let selected = if p.channel == "stable" {
            // GitHub latest is the author's stable choice, not the most recently backported release.
            self.api_optional(&p.owner, &p.repo, &["releases", "latest"], None)
                .await?
        } else {
            let mut eligible = Vec::new();
            for page in 1..=3 {
                let j = self
                    .api(&p.owner, &p.repo, &["releases"], Some(page))
                    .await?;
                let list = j.as_array().ok_or("GitHub 发行列表格式无效")?;
                for raw in list {
                    if raw.get("draft").and_then(Value::as_bool) != Some(false) {
                        continue;
                    }
                    if raw
                        .get("published_at")
                        .and_then(Value::as_str)
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .is_some()
                    {
                        eligible.push(raw.clone());
                    }
                }
                if list.len() < 100 {
                    break;
                }
                if page == 3 && eligible.is_empty() {
                    return Err("近期发行列表中没有符合通道的版本，请检查项目发行页".into());
                }
            }
            latest_preview_release(&eligible).cloned()
        };
        let Some(raw) = selected.as_ref() else {
            return Ok(None);
        };
        if raw.get("draft").and_then(Value::as_bool) != Some(false)
            || (p.channel == "stable"
                && raw.get("prerelease").and_then(Value::as_bool) != Some(false))
        {
            return Err("GitHub 返回的发行不符合所选通道".into());
        }
        if raw
            .get("published_at")
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none()
        {
            return Err("GitHub 发行日期格式无效".into());
        }
        let tag = field(raw, "tag_name", 200)?;
        let release_url = field(raw, "html_url", 4096)?;
        github_result_url(&release_url, &p.owner, &p.repo)?;
        let mut assets = Vec::new();
        for a in raw
            .get("assets")
            .and_then(Value::as_array)
            .ok_or("GitHub 附件列表格式无效")?
        {
            let mut asset = ReleaseAsset {
                id: a.get("id").and_then(Value::as_u64).ok_or("附件编号无效")?,
                name: field(a, "name", 512)?,
                size: a
                    .get("size")
                    .and_then(Value::as_u64)
                    .ok_or("附件大小无效")?,
                url: field(a, "browser_download_url", 4096)?,
                content_type: field(a, "content_type", 200)?,
                digest: a
                    .get("digest")
                    .and_then(Value::as_str)
                    .filter(|s| s.len() <= 200)
                    .map(str::to_owned),
                kind: "other".into(),
                compatible: false,
                score: 0,
                reason: String::new(),
            };
            selection::classify_for_project(
                &mut asset,
                &p.preference,
                p.asset_pattern.as_deref(),
                &p.owner,
                &p.repo,
            );
            assets.push(asset);
        }
        Ok(Some(Release {
            id: raw
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("发行编号无效")?,
            tag,
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(300)
                .collect(),
            body: raw
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(128 * 1024)
                .collect(),
            published_at: field(raw, "published_at", 100)?,
            url: release_url,
            prerelease: raw
                .get("prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            assets,
        }))
    }
}
fn latest_preview_release(values: &[Value]) -> Option<&Value> {
    values
        .iter()
        .filter(|v| v.get("draft").and_then(Value::as_bool) == Some(false))
        .filter_map(|v| {
            v.get("published_at")
                .and_then(Value::as_str)
                .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                .map(|date| (date, v))
        })
        .max_by_key(|(date, _)| *date)
        .map(|(_, v)| v)
}
pub fn field(raw: &Value, key: &str, max: usize) -> Result<String, String> {
    bounded_text(
        raw.get(key)
            .and_then(Value::as_str)
            .ok_or("GitHub 必要字段缺失")?,
        max,
        "GitHub 字段",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_credential_and_redirect_traps() {
        for bad in [
            "http://github.com/a/b/releases/download/v/a.zip",
            "https://github.com.evil.test/a.zip",
            "https://user@github.com/a/b/releases/download/v/a.zip",
            "https://github.com:444/a/b/releases/download/v/a.zip",
            "https://api.github.com/repos/a/b",
            "https://github.com/a/b/archive/main.zip",
        ] {
            assert!(trusted_download_url(bad).is_err(), "{bad}");
        }
        assert!(trusted_download_url("https://release-assets.githubusercontent.com/github-production-release-asset/a?sig=public").is_ok());
    }
    #[test]
    fn preview_follows_published_releases_not_source_or_tag_order() {
        let values = vec![
            serde_json::json!({"tag_name":"v9.0.0","draft":false,"prerelease":false,"published_at":"2026-01-01T00:00:00Z"}),
            serde_json::json!({"tag_name":"v2.0.0","draft":false,"prerelease":false,"published_at":"2026-02-01T00:00:00Z"}),
            serde_json::json!({"tag_name":"v2.1.0-beta.1","draft":false,"prerelease":true,"published_at":"2026-03-01T00:00:00Z"}),
            serde_json::json!({"tag_name":"v99.0.0","draft":true,"prerelease":false,"published_at":"2026-04-01T00:00:00Z"}),
        ];
        assert_eq!(
            latest_preview_release(&values).unwrap()["tag_name"],
            "v2.1.0-beta.1"
        );
        assert!(latest_preview_release(&[]).is_none());
    }
    #[test]
    fn parse_public_repository() {
        assert_eq!(
            repository("https://github.com/Predidit/Kazumi").unwrap(),
            ("Predidit".into(), "Kazumi".into())
        );
        assert!(repository("https://private.example/a/b").is_err());
        assert!(
            github_result_url("https://github.com/other/project/actions/runs/12", "a", "b")
                .is_err()
        );
    }
    #[test]
    fn fork_results_are_allowed_without_loosening_official_release_urls() {
        for value in [
            "https://github.com/my-account/my-builds/actions/runs/12",
            "https://github.com/my-account/app/releases/tag/v1",
            "https://github.com/my-account/app/releases/download/v1/app.zip",
        ] {
            assert!(github_build_result_url(value).is_ok(), "{value}");
            assert!(github_result_url(value, "upstream", "app").is_err());
        }
        for value in [
            "http://github.com/my-account/app/releases/tag/v1",
            "https://github.com.evil.test/a/b/releases/tag/v1",
            "https://user@github.com/a/b/releases/tag/v1",
            "https://github.com/a/b/releases/tag/v1?token=anything",
            "https://github.com/a/b/releases/download/v1",
            "https://github.com/a/b/actions/runs/not-a-number",
            "https://github.com/a/b/issues/1",
        ] {
            assert!(github_build_result_url(value).is_err(), "{value}");
        }
        assert!(github_result_url(
            "https://github.com/upstream/app/releases/tag/v1",
            "upstream",
            "app"
        )
        .is_ok());
    }
    #[test]
    fn latest_absence_is_distinct_from_api_errors() {
        assert!(!api_resource_available(StatusCode::NOT_FOUND).unwrap());
        assert!(api_resource_available(StatusCode::OK).unwrap());
        for status in [
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::FOUND,
        ] {
            assert!(api_resource_available(status).is_err(), "{status}");
        }
    }
    #[tokio::test]
    async fn changed_authorization_wakes_wait_and_rejects_stale_cooldown() {
        let github = Github::new().unwrap();
        github.cache.lock().unwrap().insert(
            "test".into(),
            Cached {
                json: serde_json::json!({}),
                etag: None,
                until: Instant::now() + Duration::from_secs(60),
            },
        );
        assert!(github.defer_requests(&None, 30).unwrap());
        let waiting = github.wait_for_request();
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), waiting.as_mut())
                .await
                .is_err()
        );
        github.set_token(Some("synthetic-test-credential".into()));
        assert!(github.cache.lock().unwrap().is_empty());
        assert!(!github.defer_requests(&None, 3600).unwrap());
        tokio::time::timeout(Duration::from_secs(1), waiting.as_mut())
            .await
            .unwrap()
            .unwrap();
        let credential = github.token.read().unwrap().clone();
        assert!(github.defer_requests(&credential, 3600).unwrap());
        github.set_token(None);
        tokio::time::timeout(Duration::from_secs(1), github.wait_for_request())
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn stable_uses_author_latest_while_preview_uses_publication_time() {
        let github = Github::new().unwrap();
        let mut project = test_project("owner", "app");
        let release = |id: u64, tag: &str, date: &str, prerelease: bool| {
            serde_json::json!({"id":id,"tag_name":tag,"name":tag,"draft":false,
            "prerelease":prerelease,"published_at":date,"html_url":format!("https://github.com/owner/app/releases/tag/{tag}"),"assets":[]})
        };
        let latest = release(1, "v2.0.0", "2026-01-01T00:00:00Z", false);
        let maintenance = release(2, "v1.9.9", "2026-02-01T00:00:00Z", false);
        let preview = release(3, "v3.0.0-beta.1", "2026-03-01T00:00:00Z", true);
        let mut draft = release(4, "v99.0.0", "2026-04-01T00:00:00Z", false);
        draft["draft"] = Value::Bool(true);
        {
            let mut cache = github.cache.lock().unwrap();
            let until = Instant::now() + Duration::from_secs(60);
            cache.insert(
                "https://api.github.com/repos/owner/app/releases/latest".into(),
                Cached {
                    json: latest.clone(),
                    etag: None,
                    until,
                },
            );
            cache.insert(
                "https://api.github.com/repos/owner/app/releases?per_page=100&page=1".into(),
                Cached {
                    json: serde_json::json!([latest, maintenance, preview, draft]),
                    etag: None,
                    until,
                },
            );
        }
        assert_eq!(
            github.release(&project).await.unwrap().unwrap().tag,
            "v2.0.0"
        );
        project.channel = "preview".into();
        assert_eq!(
            github.release(&project).await.unwrap().unwrap().tag,
            "v3.0.0-beta.1"
        );
    }
}
