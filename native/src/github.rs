//! The native GitHub page talks only to these fixed operations, never to a shell.
//!
//! `auth_root` is a host-selected, machine-local directory, NOT portable library data.
//! Only gh's configuration is isolated: its `gh:github.com` keyring entries are still
//! shared with the official CLI for the same account. Disconnecting here never runs
//! `gh auth logout`, reads a token, or removes a shared keyring entry.
//!
//! Anonymous HTTP disables redirects. Authenticated HTTP is delegated to official
//! gh: use a constructed absolute API URL (bypassing gh's api_host mapping), never
//! --paginate, and rely on gh's cross-host credential protection. gh has no switch
//! to disable every redirect; do not describe that transport as strictly no-follow.

use reqwest::{header, Client};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, MutexGuard, RwLock,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::{oneshot, Mutex},
    task::JoinHandle,
    time::{timeout, timeout_at, Instant},
};
use url::Url;

const API_ORIGIN: &str = "https://api.github.com";
const DEVICE_URI: &str = "https://github.com/login/device";
const API_VERSION: &str = "2022-11-28";
const PAGE_SIZE: usize = 30;
const MAX_PAGE: u8 = 10;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_PROCESS_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_README_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const ACCOUNT_CHANGED: &str = "GitHub 账号已变更，请重新连接，再重新确认操作。";
const UNSAFE_STORAGE: &str =
    "GitHub 授权未能保存在系统凭据库，本次连接已停止；请修复系统安全存储后重试。";
const LOGIN_WAITING: &str =
    "请在 GitHub 官方页面输入短码。系统凭据与官方 GitHub CLI 共用同名账号。";
const COMPONENT_MISSING: &str = "未找到官方 GitHub CLI 登录组件；仍可浏览公开仓库。";

/// A service with an explicitly consented, machine-local GitHub session.
/// No subprocess, process output, credential, or arbitrary endpoint is exposed.
pub struct GithubService {
    backend: Arc<Backend>,
    session: Arc<StdMutex<Session>>,
    operation: Mutex<()>,
    login_task: StdMutex<Option<LoginTask>>,
    shutting_down: AtomicBool,
}

struct Backend {
    auth_root: PathBuf,
    config_dir: PathBuf,
    gh_path: Option<PathBuf>,
    client: Option<Client>,
    configuration_error: Option<String>,
    token: RwLock<Option<String>>,
}

#[derive(Default)]
struct Session {
    consent: bool,
    account: Option<Account>,
    login: Option<LoginView>,
    error: Option<String>,
    generation: u64,
}

#[derive(Clone)]
struct Account {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

impl Account {
    fn value(&self) -> Value {
        json!({
            "login": self.login,
            "name": self.name,
            "avatarUrl": self.avatar_url,
            "url": format!("https://github.com/{}", self.login),
        })
    }
}

struct LoginView {
    state: &'static str,
    user_code: Option<String>,
    message: String,
}

impl LoginView {
    fn value(&self) -> Value {
        json!({
            "state": self.state,
            "userCode": self.user_code,
            "verificationUri": DEVICE_URI,
            "message": self.message,
        })
    }
}

struct LoginTask {
    cancel: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepositoryName {
    owner: String,
    name: String,
}

impl RepositoryName {
    fn parse(value: &str) -> Result<Self, String> {
        let (owner, name) = value
            .split_once('/')
            .ok_or("请输入 owner/repo 格式的仓库名称。")?;
        if !valid_login(owner)
            || name.is_empty()
            || name.len() > 100
            || matches!(name, "." | "..")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("仓库名称格式不正确，请使用 owner/repo。".into());
        }
        Ok(Self {
            owner: owner.into(),
            name: name.into(),
        })
    }

    fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn web_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }
}

#[derive(Clone)]
enum Endpoint {
    Viewer,
    Search {
        query: String,
        page: u8,
    },
    Starred {
        page: u8,
    },
    Owned {
        page: u8,
    },
    Repository(RepositoryName),
    Readme(RepositoryName),
    Star(RepositoryName),
    Activity {
        login: String,
        page: u8,
    },
    // Rust-only subscription bridge: never reachable via a frontend endpoint.
    RepositoryRead {
        repository: RepositoryName,
        tail: Vec<String>,
        page: Option<u32>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Put,
    Delete,
    Patch,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }
}

impl Endpoint {
    fn url(&self) -> Result<Url, String> {
        let path = match self {
            Self::Viewer => "/user".into(),
            Self::Search { .. } => "/search/repositories".into(),
            Self::Starred { .. } => "/user/starred".into(),
            Self::Owned { .. } => "/user/repos".into(),
            Self::Repository(repo) => format!("/repos/{}", repo.full_name()),
            Self::Readme(repo) => format!("/repos/{}/readme", repo.full_name()),
            Self::Star(repo) => format!("/user/starred/{}", repo.full_name()),
            Self::RepositoryRead { repository, .. } => format!("/repos/{}", repository.full_name()),
            Self::Activity { login, .. } => {
                if !valid_login(login) {
                    return Err("GitHub 账号信息不正确，请重新连接。".into());
                }
                format!("/users/{login}/received_events")
            }
        };
        let mut url =
            Url::parse(&format!("{API_ORIGIN}{path}")).map_err(|_| "无法准备 GitHub 请求。")?;
        match self {
            Self::Search { query, page } => {
                check_page(*page)?;
                url.query_pairs_mut()
                    .append_pair("q", if query.is_empty() { "stars:>0" } else { query })
                    .append_pair("sort", "stars")
                    .append_pair("order", "desc")
                    .append_pair("per_page", "30")
                    .append_pair("page", &page.to_string());
            }
            Self::Starred { page } | Self::Owned { page } => {
                check_page(*page)?;
                let mut pairs = url.query_pairs_mut();
                pairs
                    .append_pair("sort", "updated")
                    .append_pair("direction", "desc")
                    .append_pair("per_page", "30")
                    .append_pair("page", &page.to_string());
                if matches!(self, Self::Owned { .. }) {
                    pairs.append_pair("affiliation", "owner");
                }
            }
            Self::Activity { page, .. } => {
                check_page(*page)?;
                url.query_pairs_mut()
                    .append_pair("per_page", "30")
                    .append_pair("page", &page.to_string());
            }
            Self::RepositoryRead { tail, page, .. } => {
                url.path_segments_mut()
                    .map_err(|_| "GitHub 请求地址不正确。")?
                    .extend(tail.iter().map(String::as_str));
                if let Some(page) = page {
                    if !(1..=10).contains(page) {
                        return Err("页码应在 1 到 10 之间。".into());
                    }
                    url.query_pairs_mut()
                        .append_pair("per_page", "100")
                        .append_pair("page", &page.to_string());
                }
            }
            _ => {}
        }
        if url.scheme() != "https"
            || url.host_str() != Some("api.github.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("GitHub 请求地址不正确。".into());
        }
        Ok(url)
    }

    fn allows(&self, method: Method) -> bool {
        match method {
            Method::Get => true,
            Method::Put | Method::Delete => matches!(self, Self::Star(_)),
            Method::Patch => matches!(self, Self::Repository(_)),
        }
    }

    fn is_public_read(&self) -> bool {
        matches!(
            self,
            Self::Search { .. } | Self::Repository(_) | Self::Readme(_)
        )
    }

    fn accept(&self) -> &'static str {
        if matches!(self, Self::Readme(_)) {
            "application/vnd.github.raw+json"
        } else {
            "application/vnd.github+json"
        }
    }
}

struct ApiResponse {
    status: u16,
    body: Vec<u8>,
    has_next: bool,
}

impl ApiResponse {
    fn json(&self) -> Result<Value, String> {
        if !(200..300).contains(&self.status) {
            return Err(http_error(self.status));
        }
        serde_json::from_slice(&self.body).map_err(|_| "GitHub 返回的内容格式不正确。".into())
    }
}

struct CapturedProcess {
    status: ExitStatus,
    stdout: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum AuthStatusError {
    Disconnected,
    UnsafeStorage,
    Invalid,
}

impl GithubService {
    /// `auth_root` must be an absolute host-selected local authorization directory.
    /// `gh_path` is the host-resolved official CLI executable; never search PATH here.
    #[cfg(test)]
    pub fn new(auth_root: PathBuf, gh_path: Option<PathBuf>) -> Arc<Self> {
        Self::with_token(auth_root, gh_path, None)
    }

    pub fn with_token(
        auth_root: PathBuf,
        gh_path: Option<PathBuf>,
        token: Option<String>,
    ) -> Arc<Self> {
        let configuration_error = validate_auth_root(&auth_root)
            .and_then(|_| reject_link_ancestors(&auth_root))
            .err();
        let (consent, marker_error) = if configuration_error.is_none() {
            match read_connected(&auth_root) {
                Ok(value) => (value, None),
                Err(error) => (false, Some(error)),
            }
        } else {
            (false, None)
        };
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("ReleaseHub/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok();
        let config_dir = auth_root.join("github-cli");
        Arc::new(Self {
            backend: Arc::new(Backend {
                auth_root,
                config_dir,
                gh_path,
                client,
                configuration_error,
                token: RwLock::new(token.clone()),
            }),
            session: Arc::new(StdMutex::new(Session {
                consent: consent || token.is_some(),
                error: marker_error,
                ..Session::default()
            })),
            operation: Mutex::new(()),
            login_task: StdMutex::new(None),
            shutting_down: AtomicBool::new(false),
        })
    }

    pub async fn handle(self: &Arc<Self>, action: &str, payload: Value) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("应用正在退出，请稍后重新打开。".into());
        }
        match action {
            "github_status" => {
                fields(&payload, &[])?;
                if self.has_consent() {
                    let _ = self.authenticated_account().await;
                }
                Ok(self.status_value())
            }
            "github_login" => {
                fields(&payload, &[])?;
                if self
                    .backend
                    .token
                    .read()
                    .map_err(|_| "授权状态不可用")?
                    .is_some()
                {
                    return Err("请先断开令牌登录，再使用设备授权".into());
                }
                self.start_login().await;
                Ok(self.status_value())
            }
            "github_cancel_login" => {
                fields(&payload, &[])?;
                let was_waiting = session_lock(&self.session)
                    .login
                    .as_ref()
                    .is_some_and(|login| login.state == "waiting");
                self.stop_login().await;
                if was_waiting {
                    let cleanup = self
                        .backend
                        .remove_app_hosts()
                        .and_then(|_| self.backend.save_connected(false));
                    let mut session = session_lock(&self.session);
                    session.consent = false;
                    session.account = None;
                    session.error = cleanup.err();
                }
                session_lock(&self.session).login = None;
                Ok(self.status_value())
            }
            "github_logout" => {
                fields(&payload, &[])?;
                self.stop_login().await;
                if let Ok(mut token) = self.backend.token.write() {
                    *token = None;
                }
                let error = self.backend.save_connected(false).err();
                let mut session = session_lock(&self.session);
                session.consent = false;
                session.account = None;
                session.login = None;
                session.error = error;
                drop(session);
                Ok(self.status_value())
            }
            "github_repositories" => self.repositories(&payload).await,
            "github_repository" => self.repository(&payload).await,
            "github_set_star" => self.set_star(&payload).await,
            "github_update_repository" => self.update_repository(&payload).await,
            "github_activity" => self.activity(&payload).await,
            _ => Err("不支持这个 GitHub 操作。".into()),
        }
    }

    pub async fn connect_token(
        &self,
        token: String,
        persist: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.stop_login().await;
        let previous = self
            .backend
            .token
            .write()
            .map_err(|_| "授权状态不可用")?
            .replace(token.clone());
        let account = match self.backend.authenticated_account().await {
            Ok(account) => account,
            Err(error) => {
                *self.backend.token.write().map_err(|_| "授权状态不可用")? = previous;
                return Err(error);
            }
        };
        self.commit_token(previous, account, &token, persist)
    }

    fn commit_token(
        &self,
        previous: Option<String>,
        account: Account,
        token: &str,
        persist: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<Value, String> {
        if let Err(error) = persist(token) {
            *self.backend.token.write().map_err(|_| "授权状态不可用")? = previous;
            return Err(error);
        }
        {
            let mut session = session_lock(&self.session);
            session.consent = true;
            session.account = Some(account);
            session.error = None;
            session.login = None;
        }
        Ok(self.status_value())
    }

    /// Internal GET-only bridge for subscribed public software. Raw API JSON is
    /// consumed by Rust core's existing projection, never forwarded as a page or
    /// credential. Outer None means no app consent; inner None means HTTP 404.
    pub(crate) async fn repository_api(
        &self,
        owner: &str,
        repo: &str,
        tail: &[&str],
        page: Option<u32>,
    ) -> Option<Result<Option<Value>, String>> {
        let _operation = self.operation.lock().await;
        if !self.has_consent() {
            return None;
        }
        Some(
            async {
                let endpoint = repository_read_endpoint(owner, repo, tail, page)?;
                self.authenticated_account().await?;
                let response = self
                    .backend
                    .api(&endpoint, Method::Get, true, false, None)
                    .await?;
                if response.status == 404 {
                    return Ok(None);
                }
                response.json().map(Some)
            }
            .await,
        )
    }

    /// The host must await this on application exit. A completed connection stays
    /// remembered, but our own pending login child is killed and reaped.
    pub async fn shutdown(&self) {
        // Never queue application exit behind an unrelated 30-second API call.
        // The flag and login_task mutex also prevent a late login spawn racing
        // with this cancellation; stop_login only waits for our login child.
        self.shutting_down.store(true, Ordering::Release);
        self.stop_login().await;
    }

    fn has_consent(&self) -> bool {
        session_lock(&self.session).consent
    }

    fn status_value(&self) -> Value {
        let session = session_lock(&self.session);
        let available = self.backend.available()
            || self.backend.token.read().is_ok_and(|token| token.is_some());
        let error = session
            .error
            .clone()
            .or_else(|| self.backend.configuration_error.clone())
            .or_else(|| {
                if available {
                    None
                } else {
                    Some(COMPONENT_MISSING.into())
                }
            });
        json!({
            "available": available || self.backend.token.read().is_ok_and(|token| token.is_some()),
            "tokenSupported": true,
            "connected": session.consent && session.account.is_some(),
            "account": if session.consent { session.account.as_ref().map(Account::value) } else { None },
            "login": session.login.as_ref().map(LoginView::value),
            "error": error,
            "provider": if self.backend.token.read().is_ok_and(|token| token.is_some()) { "token" } else { "github-cli" },
        })
    }

    fn disconnect_with_error(&self, error: String) {
        let marker_error = self.backend.save_connected(false).err();
        let mut session = session_lock(&self.session);
        session.consent = false;
        session.account = None;
        session.login = None;
        session.error = Some(marker_error.unwrap_or(error));
    }

    async fn authenticated_account(&self) -> Result<Account, String> {
        let previous = {
            let session = session_lock(&self.session);
            if !session.consent {
                return Err("请先连接 GitHub 账号，再使用这个功能。".into());
            }
            session.account.clone()
        };
        let account = match self.backend.authenticated_account().await {
            Ok(account) => account,
            Err(error) => {
                self.disconnect_with_error(error.clone());
                return Err(error);
            }
        };
        if previous
            .as_ref()
            .is_some_and(|old| !old.login.eq_ignore_ascii_case(&account.login))
        {
            self.disconnect_with_error(ACCOUNT_CHANGED.into());
            return Err(ACCOUNT_CHANGED.into());
        }
        let mut session = session_lock(&self.session);
        session.account = Some(account.clone());
        session.error = None;
        Ok(account)
    }

    async fn start_login(&self) {
        let running = self
            .login_task
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|task| !task.task.is_finished());
        if running {
            return;
        }
        self.stop_login().await;
        if self.has_consent() && self.authenticated_account().await.is_ok() {
            return;
        }
        let prepared = if !self.backend.available() {
            Err(COMPONENT_MISSING.into())
        } else {
            self.backend
                .ensure_layout()
                .and_then(|_| self.backend.save_connected(false))
                .and_then(|_| self.backend.remove_app_hosts())
        };
        if let Err(error) = prepared {
            let mut session = session_lock(&self.session);
            session.consent = false;
            session.account = None;
            session.error = Some(error.clone());
            session.login = Some(LoginView {
                state: "error",
                user_code: None,
                message: error,
            });
            return;
        }
        let generation = {
            let mut session = session_lock(&self.session);
            session.generation = session.generation.wrapping_add(1);
            session.consent = false;
            session.account = None;
            session.error = None;
            session.login = Some(LoginView {
                state: "waiting",
                user_code: None,
                message: LOGIN_WAITING.into(),
            });
            session.generation
        };
        let backend = self.backend.clone();
        let session = self.session.clone();
        let (cancel, receiver) = oneshot::channel();
        let mut slot = self
            .login_task
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let task = tokio::spawn(async move {
            login_worker(backend, session, generation, receiver).await;
        });
        *slot = Some(LoginTask { cancel, task });
    }

    async fn stop_login(&self) {
        let task = self
            .login_task
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(task) = task {
            let _ = task.cancel.send(());
            let _ = task.task.await;
        }
    }

    async fn repositories(&self, payload: &Value) -> Result<Value, String> {
        fields(payload, &["tab", "query", "page"])?;
        let tab = string_field(payload, "tab")?;
        let query = query_field(payload)?;
        let page = page_field(payload)?;
        let endpoint = match tab {
            "explore" => Endpoint::Search {
                query: query.clone(),
                page,
            },
            "starred" => Endpoint::Starred { page },
            "owned" => Endpoint::Owned { page },
            _ => return Err("请选择探索、已 Star 或自己的仓库。".into()),
        };
        let authenticated = if tab != "explore" || self.has_consent() {
            self.authenticated_account().await?;
            true
        } else {
            false
        };
        let response = self
            .backend
            .api(&endpoint, Method::Get, authenticated, false, None)
            .await?;
        let value = response.json()?;
        let items = if tab == "explore" {
            value["items"].as_array()
        } else {
            value.as_array()
        }
        .ok_or("GitHub 仓库列表格式不正确。")?;
        let total_count = if tab == "explore" {
            value["total_count"]
                .as_u64()
                .filter(|count| *count <= MAX_SAFE_INTEGER)
        } else {
            None
        };
        let has_more = page < MAX_PAGE
            && (response.has_next
                || (items.len() >= PAGE_SIZE
                    && total_count
                        .is_none_or(|total| u64::from(page) * (PAGE_SIZE as u64) < total)));
        let mut projected = Vec::new();
        for item in items.iter().take(PAGE_SIZE) {
            let repo = project_repository(item, if tab == "starred" { Some(true) } else { None })?;
            // Stars and owned queries intentionally filter ONLY this returned page.
            // The UI must label this "筛选本页"; totalCount remains null and hasMore
            // is computed before filtering so an empty match page can still advance.
            if tab == "explore" || repository_matches(&repo, &query) {
                projected.push(repo);
            }
        }
        Ok(
            json!({ "items": projected, "page": page, "hasMore": has_more, "totalCount": total_count }),
        )
    }

    async fn repository(&self, payload: &Value) -> Result<Value, String> {
        fields(payload, &["repository"])?;
        let requested = RepositoryName::parse(string_field(payload, "repository")?)?;
        let authenticated = if self.has_consent() {
            self.authenticated_account().await?;
            true
        } else {
            false
        };
        let response = self
            .backend
            .api(
                &Endpoint::Repository(requested),
                Method::Get,
                authenticated,
                false,
                None,
            )
            .await?;
        let mut repository = project_repository(&response.json()?, None)?;
        let canonical =
            RepositoryName::parse(repository["fullName"].as_str().ok_or("仓库信息不完整。")?)?;
        if authenticated {
            repository["starred"] = json!(self.star_state(&canonical).await?);
        }
        let readme_response = self
            .backend
            .api(
                &Endpoint::Readme(canonical),
                Method::Get,
                authenticated,
                false,
                None,
            )
            .await?;
        let readme = match readme_response.status {
            200 => readme_text(&readme_response.body),
            404 | 204 => String::new(),
            status => return Err(http_error(status)),
        };
        Ok(json!({ "repository": repository, "readme": readme }))
    }

    async fn star_state(&self, repository: &RepositoryName) -> Result<bool, String> {
        let response = self
            .backend
            .api(
                &Endpoint::Star(repository.clone()),
                Method::Get,
                true,
                false,
                None,
            )
            .await?;
        match response.status {
            204 => Ok(true),
            404 => Ok(false),
            status => Err(http_error(status)),
        }
    }

    async fn set_star(&self, payload: &Value) -> Result<Value, String> {
        fields(payload, &["repository", "starred", "confirmed"])?;
        let confirmed = require_confirmed(payload)?;
        let repository = RepositoryName::parse(string_field(payload, "repository")?)?;
        let starred = payload["starred"]
            .as_bool()
            .ok_or("请选择 Star 或取消 Star。")?;
        // Revalidate the active keyring account immediately before a mutation.
        self.authenticated_account().await?;
        let method = if starred { Method::Put } else { Method::Delete };
        let response = self
            .backend
            .api(
                &Endpoint::Star(repository.clone()),
                method,
                true,
                confirmed,
                None,
            )
            .await?;
        if response.status != 204 {
            return Err(http_error(response.status));
        }
        // A redirected PUT/DELETE can become GET in gh's HTTP transport. Verify
        // the desired state, not just process exit 0 or a generic 2xx response.
        if self.star_state(&repository).await? != starred {
            return Err("GitHub 的 Star 状态与本次操作不一致，请刷新后重新确认。".into());
        }
        Ok(json!({ "starred": starred }))
    }

    async fn update_repository(&self, payload: &Value) -> Result<Value, String> {
        fields(
            payload,
            &["repository", "description", "homepage", "confirmed"],
        )?;
        let confirmed = require_confirmed(payload)?;
        let repository = RepositoryName::parse(string_field(payload, "repository")?)?;
        let description = description_field(payload)?;
        let homepage = homepage_field(payload)?;
        self.authenticated_account().await?;
        let endpoint = Endpoint::Repository(repository.clone());
        let before = self
            .backend
            .api(&endpoint, Method::Get, true, false, None)
            .await?
            .json()?;
        let before_projected = project_repository(&before, None)?;
        if before_projected["canAdmin"].as_bool() != Some(true) {
            return Err("只有拥有管理权限的账号才能修改这个仓库。".into());
        }
        if before_projected["archived"].as_bool() == Some(true) {
            return Err("已归档仓库是只读的，无法在这里修改。".into());
        }
        if !before_projected["fullName"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(&repository.full_name()))
        {
            return Err("仓库名称已变更，请重新打开仓库详情后再确认。".into());
        }
        self.authenticated_account().await?;
        // Only these two fields are serialized; no name/privacy/archive changes.
        // JSON travels over stdin, never through a shell or -f/-F arguments.
        let body = json!({ "description": description, "homepage": homepage });
        let response = self
            .backend
            .api(&endpoint, Method::Patch, true, confirmed, Some(&body))
            .await?;
        if response.status != 200 {
            return Err(http_error(response.status));
        }
        let after = response.json()?;
        if !update_matches(&before, &after, &description, &homepage) {
            return Err("GitHub 返回的信息与本次修改不一致，请刷新后重新确认。".into());
        }
        project_repository(&after, None)
    }

    async fn activity(&self, payload: &Value) -> Result<Value, String> {
        fields(payload, &["page"])?;
        let page = page_field(payload)?;
        let account = self.authenticated_account().await?;
        let response = self
            .backend
            .api(
                &Endpoint::Activity {
                    login: account.login,
                    page,
                },
                Method::Get,
                true,
                false,
                None,
            )
            .await?;
        let value = response.json()?;
        let events = value.as_array().ok_or("GitHub 动态列表格式不正确。")?;
        let has_more = page < MAX_PAGE && (response.has_next || events.len() >= PAGE_SIZE);
        let items: Vec<Value> = events
            .iter()
            .take(PAGE_SIZE)
            .filter_map(project_activity)
            .collect();
        // This is GitHub's received_events feed, not real-time/all-star release news.
        Ok(json!({ "items": items, "page": page, "hasMore": has_more }))
    }
}

impl Drop for GithubService {
    fn drop(&mut self) {
        if let Some(task) = self
            .login_task
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = task.cancel.send(());
            // There is no strong service reference in the worker. Aborting drops
            // its kill_on_drop child; shutdown() is preferred for awaited reaping.
            task.task.abort();
        }
    }
}

impl Backend {
    fn available(&self) -> bool {
        self.gh_path.as_ref().is_some_and(|path| {
            path.is_absolute()
                && !path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
                && path.is_file()
        })
    }

    fn ensure_layout(&self) -> Result<(), String> {
        if let Some(error) = &self.configuration_error {
            return Err(error.clone());
        }
        validate_auth_root(&self.auth_root)?;
        reject_link_ancestors(&self.auth_root)?;
        fs::create_dir_all(&self.auth_root).map_err(|_| "无法创建本机 GitHub 授权目录。")?;
        reject_link_ancestors(&self.config_dir)?;
        fs::create_dir_all(&self.config_dir).map_err(|_| "无法创建 GitHub 登录配置目录。")?;
        reject_link_ancestors(&self.config_dir)
    }

    fn save_connected(&self, connected: bool) -> Result<(), String> {
        validate_auth_root(&self.auth_root)?;
        reject_link_ancestors(&self.auth_root)?;
        if !connected && !self.auth_root.exists() {
            return Ok(());
        }
        if connected {
            self.ensure_layout()?;
        }
        let path = self.auth_root.join("connected.json");
        check_regular_file(&path)?;
        let write = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            file.write_all(if connected { b"true\n" } else { b"false\n" })?;
            file.sync_all()
        })();
        if write.is_err() {
            // A failed disconnect must not leave a previous true marker active.
            if !connected && remove_exact_file(&path).is_ok() {
                return Ok(());
            }
            return Err("无法保存本机 GitHub 连接状态，请检查授权目录是否可写。".into());
        }
        Ok(())
    }

    fn remove_app_hosts(&self) -> Result<(), String> {
        validate_auth_root(&self.auth_root)?;
        reject_link_ancestors(&self.config_dir)?;
        // gh writes both host and per-user plaintext fallback oauth_token fields
        // into this exact app-only file. Remove it without reading any token. Do
        // not touch config outside this directory or any shared keyring entry.
        remove_exact_file(&self.config_dir.join("hosts.yml"))
    }

    fn command(&self, args: &[&str], input: bool) -> Result<Command, String> {
        if !self.available() {
            return Err(COMPONENT_MISSING.into());
        }
        self.ensure_layout()?;
        let executable = self.gh_path.as_ref().ok_or(COMPONENT_MISSING)?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(&self.config_dir)
            .stdin(if input { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Clear only named settings; never inspect, copy or log the environment.
        for key in [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GH_ENTERPRISE_TOKEN",
            "GITHUB_ENTERPRISE_TOKEN",
            "GH_DEBUG",
            "GH_BROWSER",
            "BROWSER",
            "GH_HOST",
            "GH_REPO",
            "GH_HTTP_UNIX_SOCKET",
            "GH_PAGER",
            "PAGER",
            "LESS",
            "GH_EDITOR",
            "GIT_EDITOR",
            "VISUAL",
            "EDITOR",
            "GIT_ASKPASS",
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "GH_FORCE_TTY",
            "CLICOLOR_FORCE",
            "FORCE_COLOR",
            "GH_FORCE_COLOR",
            "GH_ACCESSIBLE_PROMPTER",
            "GH_ACCESSIBLE_COLORS",
            "GLAMOUR_STYLE",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ] {
            command.env_remove(key);
        }
        command
            .env("GH_CONFIG_DIR", &self.config_dir)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("TERM", "dumb")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1");
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        Ok(command)
    }

    async fn authenticated_account(&self) -> Result<Account, String> {
        if self.token.read().map_err(|_| "授权状态不可用")?.is_some() {
            let response = self
                .api(&Endpoint::Viewer, Method::Get, true, false, None)
                .await?;
            return project_account(&response.json()?);
        }

        let command = self.command(
            &[
                "auth",
                "status",
                "--active",
                "--hostname",
                "github.com",
                "--json",
                "hosts",
            ],
            false,
        )?;
        let output = capture(command, None, MAX_PROCESS_MESSAGE_BYTES).await?;
        let login = match parse_auth_status(&output.stdout) {
            Ok(login) if output.status.success() => login,
            Err(AuthStatusError::UnsafeStorage) => {
                self.remove_app_hosts()?;
                return Err(UNSAFE_STORAGE.into());
            }
            _ => return Err("GitHub 登录未生效或已失效，请重新连接。".into()),
        };
        let value = self
            .api(&Endpoint::Viewer, Method::Get, true, false, None)
            .await?
            .json()?;
        let account = project_account(&value)?;
        if !account.login.eq_ignore_ascii_case(&login) {
            return Err(ACCOUNT_CHANGED.into());
        }
        Ok(account)
    }

    async fn api(
        &self,
        endpoint: &Endpoint,
        method: Method,
        authenticated: bool,
        confirmed: bool,
        body: Option<&Value>,
    ) -> Result<ApiResponse, String> {
        if !endpoint.allows(method) || (method != Method::Get && (!authenticated || !confirmed)) {
            return Err("这个 GitHub 操作未获确认。".into());
        }
        if (method == Method::Patch) != body.is_some() {
            return Err("GitHub 请求内容不正确。".into());
        }
        let target = endpoint.url()?;
        let token = if authenticated {
            self.token.read().map_err(|_| "授权状态不可用")?.clone()
        } else {
            None
        };
        if !authenticated || token.is_some() {
            if !authenticated && (method != Method::Get || !endpoint.is_public_read()) {
                return Err("请先连接 GitHub 账号。".into());
            }
            let client = self.client.as_ref().ok_or("无法准备 GitHub 网络连接。")?;
            let http_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
                .map_err(|_| "请求方法无效")?;
            let mut request = client
                .request(http_method, target)
                .header(header::ACCEPT, endpoint.accept())
                .header("X-GitHub-Api-Version", API_VERSION);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            if let Some(body) = body {
                request = request.json(body);
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| "暂时无法连接 GitHub，请检查网络后重试。")?;
            let status = response.status().as_u16();
            if (300..400).contains(&status) {
                return Err("GitHub 返回了跳转地址，请使用仓库的当前完整名称。".into());
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
            {
                return Err("GitHub 返回的内容过大，请到官网查看。".into());
            }
            let has_next = response
                .headers()
                .get(header::LINK)
                .and_then(|value| value.to_str().ok())
                .is_some_and(has_next_link);
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "读取 GitHub 内容失败，请重试。")?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err("GitHub 返回的内容过大，请到官网查看。".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(ApiResponse {
                status,
                body: bytes,
                has_next,
            });
        }
        let accept = format!("Accept: {}", endpoint.accept());
        let version = format!("X-GitHub-Api-Version: {API_VERSION}");
        let mut args = vec![
            "api",
            "--hostname",
            "github.com",
            "--method",
            method.as_str(),
            "--include",
            "--header",
            accept.as_str(),
            "--header",
            version.as_str(),
        ];
        if body.is_some() {
            args.extend(["--header", "Content-Type: application/json", "--input", "-"]);
        }
        // Absolute, constructed URL: --hostname alone does not bypass api_host.
        args.push(target.as_str());
        let input = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| "无法准备仓库修改内容。")?;
        let output = capture(
            self.command(&args, input.is_some())?,
            input,
            MAX_RESPONSE_BYTES + MAX_HEADER_BYTES,
        )
        .await?;
        let response = parse_cli_response(&output.stdout)?;
        if !output.status.success() && (200..400).contains(&response.status) {
            return Err("GitHub 请求没有正常完成，请刷新后重试。".into());
        }
        if (300..400).contains(&response.status) {
            return Err("GitHub 返回了跳转地址，请刷新仓库信息后重试。".into());
        }
        Ok(response)
    }
}

fn repository_read_endpoint(
    owner: &str,
    repo: &str,
    tail: &[&str],
    page: Option<u32>,
) -> Result<Endpoint, String> {
    let repository = RepositoryName::parse(&format!("{owner}/{repo}"))?;
    let allowed = match tail {
        [] | ["releases"] | ["releases", "latest"] => true,
        ["commits", reference] => valid_git_reference(reference),
        ["compare", comparison] => comparison.split_once("...").is_some_and(|(base, head)| {
            valid_compare_reference(base) && valid_compare_reference(head)
        }),
        _ => false,
    };
    if !allowed || page.is_some_and(|value| !(1..=10).contains(&value) || tail != ["releases"]) {
        return Err("不支持这个仓库读取请求。".into());
    }
    Ok(Endpoint::RepositoryRead {
        repository,
        tail: tail.iter().map(|part| (*part).to_owned()).collect(),
        page,
    })
}

fn valid_git_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 512
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference.chars().any(|ch| {
            ch.is_control()
                || ch.is_whitespace()
                || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && reference.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.ends_with('.')
                && !part.ends_with(".lock")
        })
}

fn valid_compare_reference(reference: &str) -> bool {
    match reference.split_once(':') {
        Some((owner, reference)) => valid_login(owner) && valid_git_reference(reference),
        None => valid_git_reference(reference),
    }
}

fn session_lock(session: &StdMutex<Session>) -> MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|error| error.into_inner())
}

async fn capture(
    mut command: Command,
    input: Option<Vec<u8>>,
    max_stdout: usize,
) -> Result<CapturedProcess, String> {
    let mut child = command
        .spawn()
        .map_err(|_| "无法启动官方 GitHub CLI，请检查登录组件。")?;
    let stdout = child.stdout.take().ok_or("无法读取 GitHub 组件响应。")?;
    let stderr = child.stderr.take().ok_or("无法读取 GitHub 组件状态。")?;
    let stdin = child.stdin.take();
    let result = timeout(REQUEST_TIMEOUT, async {
        let writer = async move {
            if let Some(bytes) = input {
                let mut stdin = stdin.ok_or("无法向 GitHub 组件传送修改内容。")?;
                stdin
                    .write_all(&bytes)
                    .await
                    .map_err(|_| "无法向 GitHub 组件传送修改内容。")?;
                stdin
                    .shutdown()
                    .await
                    .map_err(|_| "无法结束 GitHub 请求输入。")?;
            }
            Ok::<(), String>(())
        };
        let wait = async {
            child
                .wait()
                .await
                .map_err(|_| "GitHub 组件没有正常结束。".to_owned())
        };
        // Both pipes are drained concurrently to avoid blocking the child. Stderr
        // is bounded and discarded; no raw CLI error reaches the frontend/logs.
        let (status, stdout, _, _) = tokio::try_join!(
            wait,
            read_bounded(stdout, max_stdout),
            drain_bounded(stderr, MAX_PROCESS_MESSAGE_BYTES),
            writer
        )?;
        Ok::<CapturedProcess, String>(CapturedProcess { status, stdout })
    })
    .await;
    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => {
            kill_child(&mut child).await;
            Err(error)
        }
        Err(_) => {
            kill_child(&mut child).await;
            Err("GitHub 请求超时，请稍后重试。".into())
        }
    }
}

async fn kill_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    maximum: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|_| "读取 GitHub 组件响应失败。")?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > maximum {
            return Err("GitHub 返回的内容过大，请到官网查看。".into());
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

async fn drain_bounded<R: AsyncRead + Unpin>(mut reader: R, maximum: usize) -> Result<(), String> {
    let mut count = 0usize;
    let mut chunk = [0u8; 4096];
    loop {
        let size = reader
            .read(&mut chunk)
            .await
            .map_err(|_| "读取 GitHub 组件状态失败。")?;
        if size == 0 {
            return Ok(());
        }
        count = count.saturating_add(size);
        if count > maximum {
            return Err("GitHub 组件输出异常，本次操作已停止。".into());
        }
    }
}

async fn login_stderr<R: AsyncRead + Unpin>(
    mut reader: R,
    session: &StdMutex<Session>,
    generation: u64,
) -> Result<(), String> {
    let mut total = 0usize;
    let mut line = Vec::new();
    let mut discard_line = false;
    let mut chunk = [0u8; 1024];
    loop {
        let size = reader
            .read(&mut chunk)
            .await
            .map_err(|_| "读取 GitHub 登录状态失败。")?;
        if size == 0 {
            return Ok(());
        }
        total = total.saturating_add(size);
        if total > MAX_PROCESS_MESSAGE_BYTES {
            return Err("GitHub 登录组件输出异常，本次登录已停止。".into());
        }
        for byte in &chunk[..size] {
            if *byte == b'\n' {
                if !discard_line {
                    if let Some(code) = parse_device_code(&line) {
                        let mut state = session_lock(session);
                        if state.generation == generation {
                            if let Some(login) = &mut state.login {
                                if login.state == "waiting" {
                                    login.user_code = Some(code);
                                }
                            }
                        }
                    }
                }
                line.clear();
                discard_line = false;
            } else if !discard_line {
                if line.len() < 512 {
                    line.push(*byte);
                } else {
                    line.clear();
                    discard_line = true;
                }
            }
        }
    }
}

async fn login_worker(
    backend: Arc<Backend>,
    session: Arc<StdMutex<Session>>,
    generation: u64,
    mut cancel: oneshot::Receiver<()>,
) {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let result = async {
        // Piped stdout/stderr and null stdin make this non-interactive. Official
        // gh only prints its device URL in this mode; the host opens DEVICE_URI.
        let mut command = backend.command(&["auth", "login", "--hostname", "github.com", "--web", "--skip-ssh-key"], false)?;
        let mut child = command.spawn().map_err(|_| "无法启动官方 GitHub CLI 登录组件。")?;
        let stdout = child.stdout.take().ok_or("无法读取 GitHub 登录组件响应。")?;
        let stderr = child.stderr.take().ok_or("无法读取 GitHub 登录组件状态。")?;
        let process_result = {
            let process = async {
                let wait = async { child.wait().await.map_err(|_| "GitHub 登录组件没有正常结束。".to_owned()) };
                let (status, _, _) = tokio::try_join!(wait, drain_bounded(stdout, MAX_PROCESS_MESSAGE_BYTES), login_stderr(stderr, &session, generation))?;
                if status.success() { Ok(()) } else { Err("GitHub 登录没有完成，请重新发起登录。".into()) }
            };
            tokio::select! {
                _ = &mut cancel => Err("已取消本次 GitHub 登录。".to_owned()),
                value = timeout_at(deadline, process) => value.unwrap_or_else(|_| Err("GitHub 登录已超时，请重新获取短码。".into())),
            }
        };
        if process_result.is_err() { kill_child(&mut child).await; }
        process_result?;
        // Success text/exit code is not sufficient: require keyring storage and
        // verify /user agrees with the active account in --json hosts.
        tokio::select! {
            _ = &mut cancel => Err("已取消本次 GitHub 登录。".to_owned()),
            value = timeout_at(deadline, backend.authenticated_account()) => value.unwrap_or_else(|_| Err("GitHub 登录已超时，请重新获取短码。".into())),
        }
    }.await;
    let result = result.and_then(|account| {
        backend.save_connected(true)?;
        Ok(account)
    });
    match result {
        Ok(account) => {
            let mut state = session_lock(&session);
            if state.generation != generation {
                return;
            }
            state.consent = true;
            state.account = Some(account);
            state.error = None;
            state.login = Some(LoginView {
                state: "complete",
                user_code: None,
                message: "已安全连接。断开只影响本应用，不会退出其他 GitHub 工具。".into(),
            });
        }
        Err(error) => {
            // Also clean up on cancel/timeout: gh may have saved a fallback just
            // before the cancellation arrived. Never read or copy its contents.
            let cleanup_error = backend.remove_app_hosts().err();
            let marker_error = backend.save_connected(false).err();
            let error = cleanup_error.or(marker_error).unwrap_or(error);
            let mut state = session_lock(&session);
            if state.generation != generation {
                return;
            }
            state.consent = false;
            state.account = None;
            state.error = Some(error.clone());
            state.login = Some(LoginView {
                state: "error",
                user_code: None,
                message: error,
            });
        }
    }
}

fn parse_device_code(line: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(line).ok()?.trim();
    let code = text.strip_prefix("! First copy your one-time code: ")?;
    let bytes = code.as_bytes();
    if bytes.len() == 9
        && bytes[4] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Some(code.to_owned())
    } else {
        None
    }
}

fn parse_auth_status(bytes: &[u8]) -> Result<String, AuthStatusError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| AuthStatusError::Invalid)?;
    let hosts = value
        .get("hosts")
        .and_then(Value::as_object)
        .ok_or(AuthStatusError::Invalid)?;
    let entries = hosts
        .get("github.com")
        .and_then(Value::as_array)
        .ok_or(AuthStatusError::Disconnected)?;
    let mut active = entries
        .iter()
        .filter(|entry| entry["active"].as_bool() == Some(true));
    let entry = active.next().ok_or(AuthStatusError::Disconnected)?;
    if active.next().is_some() {
        return Err(AuthStatusError::Invalid);
    }
    if entry["host"].as_str() != Some("github.com") || entry["state"].as_str() != Some("success") {
        return Err(AuthStatusError::Disconnected);
    }
    if entry["tokenSource"].as_str() != Some("keyring") {
        return Err(AuthStatusError::UnsafeStorage);
    }
    let login = entry["login"]
        .as_str()
        .filter(|login| valid_login(login))
        .ok_or(AuthStatusError::Invalid)?;
    Ok(login.to_owned())
}

fn parse_cli_response(bytes: &[u8]) -> Result<ApiResponse, String> {
    let prefix = &bytes[..bytes.len().min(MAX_HEADER_BYTES + 4)];
    let crlf = prefix
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .map(|index| (index, 4));
    let lf = prefix
        .windows(2)
        .position(|part| part == b"\n\n")
        .map(|index| (index, 2));
    // gh's headers can use LF even when the raw README uses CRLF. The first
    // separator wins; never reinterpret an empty line in the body as the header.
    let (header_end, separator_len) = match (crlf, lf) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => return Err("GitHub 组件未返回有效的请求状态。".into()),
    };
    if header_end > MAX_HEADER_BYTES
        || bytes.len() - header_end - separator_len > MAX_RESPONSE_BYTES
    {
        return Err("GitHub 返回的内容过大，请到官网查看。".into());
    }
    let headers =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| "GitHub 请求状态格式不正确。")?;
    let mut lines = headers.lines();
    let mut status_line = lines.next().unwrap_or("").split_whitespace();
    if !status_line.next().is_some_and(|version| {
        matches!(
            version,
            "HTTP/1.0" | "HTTP/1.1" | "HTTP/2.0" | "HTTP/2" | "HTTP/3.0"
        )
    }) {
        return Err("GitHub 请求状态格式不正确。".into());
    }
    let status = status_line
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (100..=599).contains(value))
        .ok_or("GitHub 请求状态格式不正确。")?;
    let has_next = lines
        .filter_map(|line| line.split_once(':'))
        .any(|(key, value)| key.eq_ignore_ascii_case("link") && has_next_link(value));
    Ok(ApiResponse {
        status,
        body: bytes[header_end + separator_len..].to_vec(),
        has_next,
    })
}

fn has_next_link(value: &str) -> bool {
    // This is only a display hint. No returned Link URL is ever executed.
    value.split(',').any(|link| {
        link.split(';')
            .skip(1)
            .any(|part| part.trim() == "rel=\"next\"")
    })
}

fn fields(payload: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = payload.as_object().ok_or("GitHub 请求格式不正确。")?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("请求包含不支持的信息。".into());
    }
    Ok(())
}

fn string_field<'a>(payload: &'a Value, name: &str) -> Result<&'a str, String> {
    payload[name]
        .as_str()
        .ok_or_else(|| "GitHub 请求缺少必要文字信息。".into())
}

fn check_page(page: u8) -> Result<(), String> {
    if (1..=MAX_PAGE).contains(&page) {
        Ok(())
    } else {
        Err("页码应在 1 到 10 之间。".into())
    }
}

fn page_field(payload: &Value) -> Result<u8, String> {
    let page = payload["page"]
        .as_u64()
        .filter(|page| (1..=u64::from(MAX_PAGE)).contains(page))
        .ok_or("页码应在 1 到 10 之间。")?;
    Ok(page as u8)
}

fn query_field(payload: &Value) -> Result<String, String> {
    let query = string_field(payload, "query")?;
    if query.chars().count() > 256 || query.chars().any(char::is_control) {
        return Err("搜索内容过长或包含不能使用的字符。".into());
    }
    Ok(query.trim().into())
}

fn require_confirmed(payload: &Value) -> Result<bool, String> {
    if payload["confirmed"].as_bool() != Some(true) {
        return Err("请先确认这次 GitHub 修改。".into());
    }
    Ok(true)
}

fn description_field(payload: &Value) -> Result<String, String> {
    let description = string_field(payload, "description")?;
    if description.chars().count() > 350 || description.chars().any(char::is_control) {
        return Err("仓库说明最多 350 个字，且不能包含换行或控制字符。".into());
    }
    Ok(description.into())
}

fn homepage_field(payload: &Value) -> Result<String, String> {
    let value = string_field(payload, "homepage")?;
    if value.is_empty() {
        return Ok(String::new());
    }
    safe_https(value).ok_or_else(|| "主页请留空，或填写不含账号密码的完整 HTTPS 链接。".into())
}

fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && login
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && login
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn safe_https(value: &str) -> Option<String> {
    if value.len() > 2048
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.to_string())
}

fn avatar(value: &Value) -> Option<String> {
    let value = safe_https(value.as_str()?)?;
    let url = Url::parse(&value).ok()?;
    if url.host_str() != Some("avatars.githubusercontent.com") || url.port().is_some() {
        return None;
    }
    Some(value)
}

fn text(value: &Value, maximum: usize) -> String {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|ch| !ch.is_control())
        .take(maximum)
        .collect()
}

fn optional_text(value: &Value, maximum: usize) -> Option<String> {
    if !value.is_string() {
        return None;
    }
    let value = text(value, maximum);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn count(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0).min(MAX_SAFE_INTEGER)
}

fn project_account(value: &Value) -> Result<Account, String> {
    let login = value["login"]
        .as_str()
        .filter(|login| valid_login(login))
        .ok_or("GitHub 账号信息不完整，请重新连接。")?;
    Ok(Account {
        login: login.into(),
        name: optional_text(&value["name"], 100),
        avatar_url: avatar(&value["avatar_url"]),
    })
}

fn project_repository(value: &Value, starred: Option<bool>) -> Result<Value, String> {
    let name = RepositoryName::parse(
        value["full_name"]
            .as_str()
            .ok_or("GitHub 仓库信息不完整。")?,
    )?;
    let id = value["id"]
        .as_u64()
        .filter(|id| *id > 0 && *id <= MAX_SAFE_INTEGER)
        .ok_or("GitHub 仓库编号不正确。")?;
    let topics: Vec<String> = value["topics"]
        .as_array()
        .map(|topics| {
            topics
                .iter()
                .filter_map(|topic| optional_text(topic, 50))
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "id": id,
        "name": name.name,
        "fullName": name.full_name(),
        "owner": name.owner,
        "description": optional_text(&value["description"], 350),
        "url": name.web_url(),
        "private": value["private"].as_bool().unwrap_or(false),
        "archived": value["archived"].as_bool().unwrap_or(false),
        "fork": value["fork"].as_bool().unwrap_or(false),
        "language": optional_text(&value["language"], 80),
        "stars": count(&value["stargazers_count"]),
        "forks": count(&value["forks_count"]),
        "openIssues": count(&value["open_issues_count"]),
        "updatedAt": text(&value["updated_at"], 64),
        "defaultBranch": text(&value["default_branch"], 255),
        "homepage": value["homepage"].as_str().and_then(safe_https),
        "topics": topics,
        "canAdmin": value["permissions"]["admin"].as_bool() == Some(true),
        "starred": starred,
    }))
}

fn repository_matches(repository: &Value, query: &str) -> bool {
    let query = query.to_lowercase();
    if query.is_empty() {
        return true;
    }
    ["name", "fullName", "description", "language"]
        .iter()
        .any(|key| {
            repository[*key]
                .as_str()
                .is_some_and(|value| value.to_lowercase().contains(&query))
        })
        || repository["topics"].as_array().is_some_and(|topics| {
            topics.iter().any(|topic| {
                topic
                    .as_str()
                    .is_some_and(|value| value.to_lowercase().contains(&query))
            })
        })
}

fn update_matches(before: &Value, after: &Value, description: &str, homepage: &str) -> bool {
    let expected_id = before["id"].as_u64().filter(|id| *id > 0);
    expected_id.is_some()
        && after["id"].as_u64() == expected_id
        && before["full_name"] == after["full_name"]
        && before["private"] == after["private"]
        && before["archived"] == after["archived"]
        && after["description"].as_str().unwrap_or("") == description
        && after["homepage"].as_str().unwrap_or("") == homepage
}

fn readme_text(bytes: &[u8]) -> String {
    let truncated = bytes.len() > MAX_README_BYTES;
    let bytes = &bytes[..bytes.len().min(MAX_README_BYTES)];
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if truncated {
        text.push_str("\n\n---\nREADME 较长，这里只显示前 256 KiB；完整内容请到 GitHub 查看。\n");
    }
    text
}

fn project_activity(value: &Value) -> Option<Value> {
    let repository = RepositoryName::parse(value["repo"]["name"].as_str()?).ok()?;
    let id = value["id"].as_str()?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let actor = text(&value["actor"]["login"], 100);
    if actor.is_empty() {
        return None;
    }
    let kind = text(&value["type"], 64);
    let summary = match kind.as_str() {
        "WatchEvent" => "Star 了仓库",
        "ForkEvent" => "派生了仓库",
        "PushEvent" => "推送了代码",
        "CreateEvent" => match value["payload"]["ref_type"].as_str() {
            Some("branch") => "创建了分支",
            Some("tag") => "创建了标签",
            _ => "创建了仓库内容",
        },
        "DeleteEvent" => "删除了分支或标签",
        "IssuesEvent" => "更新了议题",
        "IssueCommentEvent" => "评论了议题或拉取请求",
        "PullRequestEvent" => "更新了拉取请求",
        "PullRequestReviewEvent" => "评审了拉取请求",
        "PullRequestReviewCommentEvent" => "评论了代码评审",
        "ReleaseEvent" => "发布了版本",
        "PublicEvent" => "公开了仓库",
        "MemberEvent" => "更新了协作者",
        "GollumEvent" => "更新了 Wiki",
        "CommitCommentEvent" => "评论了提交",
        _ => "更新了仓库",
    };
    Some(json!({
        "id": id, "actor": actor, "avatarUrl": avatar(&value["actor"]["avatar_url"]),
        "repository": repository.full_name(), "type": kind, "summary": summary,
        "url": repository.web_url(), "createdAt": text(&value["created_at"], 64),
    }))
}

fn http_error(status: u16) -> String {
    match status {
        401 => "GitHub 登录已失效，请重新连接。",
        403 => "GitHub 暂时拒绝请求，可能权限不足或请求次数已用完。",
        404 => "未找到仓库，或当前账号没有访问权限。",
        409 => "仓库当前状态不允许这个操作，请刷新后重试。",
        422 => "GitHub 没有接受这些信息，请检查后重试。",
        429 => "GitHub 请求次数较多，请稍后再试。",
        500..=599 => "GitHub 暂时不可用，请稍后再试。",
        _ => "GitHub 没有返回预期结果，请刷新后重试。",
    }
    .into()
}

fn validate_auth_root(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("本机 GitHub 授权目录不正确，请重新启动应用。".into());
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err("GitHub 授权配置必须保存在本机磁盘。".into());
        }
    }
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        } // reparse point/junction
    }
    false
}

fn reject_link_ancestors(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => {
                return Err("GitHub 授权目录不能是链接或普通文件。".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("无法检查本机 GitHub 授权目录。".into()),
        }
    }
    Ok(())
}

fn check_regular_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        reject_link_ancestors(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => {
            Err("GitHub 本机配置不是安全的普通文件。".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法检查 GitHub 本机配置。".into()),
    }
}

fn read_connected(auth_root: &Path) -> Result<bool, String> {
    let path = auth_root.join("connected.json");
    check_regular_file(&path)?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("无法读取本机 GitHub 连接状态。".into()),
    };
    let mut bytes = Vec::new();
    file.take(33)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取本机 GitHub 连接状态。")?;
    if bytes.len() > 32 {
        return Ok(false);
    }
    Ok(serde_json::from_slice::<bool>(&bytes).unwrap_or(false))
}

fn remove_exact_file(path: &Path) -> Result<(), String> {
    check_regular_file(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法清理本应用的 GitHub 登录配置，请检查授权目录是否可写。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_repo() -> Value {
        json!({
            "id": 42, "name": "ignored-name", "full_name": "octocat/Hello-World",
            "owner": { "login": "untrusted-owner" }, "description": "示例说明",
            "html_url": "https://untrusted.example/redirect", "private": false,
            "archived": false, "fork": false, "language": "Rust", "stargazers_count": 12,
            "forks_count": 3, "open_issues_count": 2, "updated_at": "2026-01-01T00:00:00Z",
            "default_branch": "main", "homepage": "https://example.com/", "topics": ["desktop"],
            "permissions": { "admin": true }, "unexpected_secret": "not-a-credential",
        })
    }

    fn status_entry() -> Value {
        json!({ "hosts": { "github.com": [{ "active": true, "host": "github.com", "state": "success", "tokenSource": "keyring", "login": "octocat" }] } })
    }

    #[test]
    fn repository_projection_has_exact_contract_and_no_raw_fields() {
        let projected = project_repository(&sample_repo(), Some(true)).unwrap();
        assert_eq!(projected.as_object().unwrap().len(), 19);
        assert_eq!(projected["name"], "Hello-World");
        assert_eq!(projected["owner"], "octocat");
        assert_eq!(projected["url"], "https://github.com/octocat/Hello-World");
        assert_eq!(projected["starred"], true);
        assert_eq!(projected["canAdmin"], true);
        assert!(projected.get("unexpected_secret").is_none());
        assert!(projected.get("permissions").is_none());
        assert!(project_repository(&sample_repo(), None).unwrap()["starred"].is_null());
    }

    #[test]
    fn projection_rejects_bad_names_and_unsafe_urls() {
        let mut repo = sample_repo();
        repo["homepage"] = json!("javascript:alert(1)");
        repo["permissions"]["admin"] = json!("true");
        assert!(project_repository(&repo, None).unwrap()["homepage"].is_null());
        assert_eq!(project_repository(&repo, None).unwrap()["canAdmin"], false);
        repo["full_name"] = json!("octocat/../private");
        assert!(project_repository(&repo, None).is_err());
        assert!(avatar(&json!("https://untrusted.example/avatar.png")).is_none());
        assert!(avatar(&json!(
            "https://user:password@avatars.githubusercontent.com/u/1"
        ))
        .is_none());
        assert!(avatar(&json!("https://avatars.githubusercontent.com/u/1?v=4")).is_some());
    }

    #[test]
    fn repository_input_cannot_escape_a_fixed_endpoint() {
        for invalid in [
            "",
            "owner",
            "owner/repo/extra",
            "owner/..",
            "../repo",
            "-owner/repo",
            "owner-/repo",
            "owner/repo?x=1",
            "owner/repo#x",
            "owner/re%2Fpo",
            "owner/re\\po",
            "https://github.com/o/r",
            "owner/repo\n",
            "owner/仓库",
        ] {
            assert!(
                RepositoryName::parse(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(RepositoryName::parse("octocat/.github").is_ok());
        assert!(RepositoryName::parse("org-name/repo_name-1.2").is_ok());
    }

    #[test]
    fn search_is_encoded_not_used_as_an_endpoint_or_extra_query() {
        let query = "x&per_page=500 https://untrusted.example/#fragment";
        let url = Endpoint::Search {
            query: query.into(),
            page: 2,
        }
        .url()
        .unwrap();
        assert_eq!(url.origin().ascii_serialization(), API_ORIGIN);
        assert_eq!(url.path(), "/search/repositories");
        assert!(url.fragment().is_none());
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs.iter().find(|(key, _)| key == "q").unwrap().1, query);
        assert_eq!(pairs.iter().filter(|(key, _)| key == "per_page").count(), 1);
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "per_page" && value == "30"));
        assert!(Endpoint::Starred { page: 0 }.url().is_err());
        assert!(Endpoint::Activity {
            login: "../user".into(),
            page: 1
        }
        .url()
        .is_err());
        assert!(!Endpoint::Viewer.allows(Method::Patch));
        assert!(!Endpoint::Owned { page: 1 }.is_public_read());
    }

    #[test]
    fn page_confirmation_and_update_fields_are_strict() {
        for page in [
            json!(0),
            json!(11),
            json!(-1),
            json!(1.5),
            json!("1"),
            Value::Null,
        ] {
            assert!(page_field(&json!({ "page": page })).is_err());
        }
        assert_eq!(page_field(&json!({ "page": 10 })).unwrap(), 10);
        assert!(require_confirmed(&json!({ "confirmed": true })).is_ok());
        for confirmed in [Value::Null, json!(false), json!("true"), json!(1)] {
            assert!(require_confirmed(&json!({ "confirmed": confirmed })).is_err());
        }
        assert!(fields(
            &json!({ "description": "x", "private": false }),
            &["description", "homepage", "confirmed", "repository"]
        )
        .is_err());
        assert!(description_field(&json!({ "description": "字".repeat(350) })).is_ok());
        assert!(description_field(&json!({ "description": "字".repeat(351) })).is_err());
        assert!(description_field(&json!({ "description": "hello\nworld" })).is_err());
        assert!(query_field(&json!({ "query": "a".repeat(257) })).is_err());
    }

    #[test]
    fn homepage_allows_only_explicit_https_without_credentials() {
        for invalid in [
            "http://example.com",
            "javascript:alert(1)",
            "https://user@example.com",
            "https://user:pass@example.com",
            "https://example.com\n",
            "//example.com",
            "example.com",
        ] {
            assert!(
                homepage_field(&json!({ "homepage": invalid })).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert_eq!(homepage_field(&json!({ "homepage": "" })).unwrap(), "");
        assert_eq!(
            homepage_field(&json!({ "homepage": "https://example.com" })).unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn auth_status_requires_active_success_keyring_not_exit_zero() {
        assert_eq!(
            parse_auth_status(&serde_json::to_vec(&status_entry()).unwrap()).unwrap(),
            "octocat"
        );
        assert_eq!(
            parse_auth_status(br#"{"hosts":{}}"#),
            Err(AuthStatusError::Disconnected)
        );
        let mut status = status_entry();
        status["hosts"]["github.com"][0]["active"] = json!(false);
        assert_eq!(
            parse_auth_status(&serde_json::to_vec(&status).unwrap()),
            Err(AuthStatusError::Disconnected)
        );
        status = status_entry();
        status["hosts"]["github.com"][0]["state"] = json!("error");
        assert_eq!(
            parse_auth_status(&serde_json::to_vec(&status).unwrap()),
            Err(AuthStatusError::Disconnected)
        );
        for source in ["oauth_token", "GH_TOKEN", "some-directory/hosts.yml"] {
            status = status_entry();
            status["hosts"]["github.com"][0]["tokenSource"] = json!(source);
            assert_eq!(
                parse_auth_status(&serde_json::to_vec(&status).unwrap()),
                Err(AuthStatusError::UnsafeStorage)
            );
        }
        status = status_entry();
        let duplicate = status["hosts"]["github.com"][0].clone();
        status["hosts"]["github.com"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert_eq!(
            parse_auth_status(&serde_json::to_vec(&status).unwrap()),
            Err(AuthStatusError::Invalid)
        );
    }

    #[test]
    fn device_parser_returns_only_a_short_code_from_the_official_line() {
        assert_eq!(
            parse_device_code(b"! First copy your one-time code: A1BC-D2EF\r"),
            Some("A1BC-D2EF".into())
        );
        for line in [
            "A1BC-D2EF",
            "Open this URL: https://untrusted.example/A1BC-D2EF",
            "! First copy your one-time code: a1bc-d2ef",
            "! First copy your one-time code: A1BC-D2EF extra",
            "! First copy your one-time code: ABCD-EFGH-IJKL",
            "! First copy your one-time code: ABCD/EFGH",
        ] {
            assert!(parse_device_code(line.as_bytes()).is_none());
        }
    }

    #[tokio::test]
    async fn login_code_is_parsed_across_chunks_without_exposing_messages() {
        let session = Arc::new(StdMutex::new(Session {
            generation: 3,
            login: Some(LoginView {
                state: "waiting",
                user_code: None,
                message: LOGIN_WAITING.into(),
            }),
            ..Session::default()
        }));
        let (reader, mut writer) = tokio::io::duplex(16);
        let state = session.clone();
        let parser = tokio::spawn(async move { login_stderr(reader, &state, 3).await });
        writer
            .write_all(b"ignore arbitrary stderr\n! First copy your one-time code: ABCD-EFGH\n")
            .await
            .unwrap();
        writer.shutdown().await.unwrap();
        parser.await.unwrap().unwrap();
        let state = session_lock(&session);
        assert_eq!(
            state.login.as_ref().unwrap().user_code.as_deref(),
            Some("ABCD-EFGH")
        );
        assert_eq!(state.login.as_ref().unwrap().message, LOGIN_WAITING);
    }

    #[test]
    fn response_parser_uses_http_status_and_does_not_follow_links() {
        let response = parse_cli_response(b"HTTP/2.0 200 OK\r\nLink: <https://untrusted.example/?page=2>; rel=\"next\"\r\n\r\n{\"items\":[]}").unwrap();
        assert_eq!(response.status, 200);
        assert!(response.has_next);
        assert_eq!(response.json().unwrap(), json!({ "items": [] }));
        assert_eq!(
            parse_cli_response(b"HTTP/2.0 204 No Content\n\n")
                .unwrap()
                .status,
            204
        );
        let missing =
            parse_cli_response(b"HTTP/2.0 404 Not Found\n\n{\"message\":\"remote content\"}")
                .unwrap();
        assert!(missing.json().unwrap_err().contains("未找到"));
        let mixed =
            parse_cli_response(b"HTTP/2.0 200 OK\n\n# Title\r\n\r\nKeep this body").unwrap();
        assert_eq!(mixed.body, b"# Title\r\n\r\nKeep this body");
        assert!(parse_cli_response(b"gh: arbitrary stderr, not an HTTP response").is_err());
        assert!(parse_cli_response(b"HTTP/2.0 999 Wrong\n\n{}").is_err());
    }

    #[test]
    fn readme_remains_markdown_and_is_bounded() {
        let markdown = b"# Example\n<script>not executed by backend</script>";
        assert_eq!(readme_text(markdown), String::from_utf8_lossy(markdown));
        let long = vec![b'a'; MAX_README_BYTES + 1];
        let text = readme_text(&long);
        assert!(text.len() < MAX_README_BYTES + 256);
        assert!(text.contains("README 较长"));
    }

    #[test]
    fn activity_uses_a_small_local_summary_not_remote_instructions() {
        let activity = project_activity(&json!({
            "id": "1234", "type": "ReleaseEvent", "actor": { "login": "octocat", "avatar_url": "https://untrusted.example/avatar" },
            "repo": { "name": "octocat/Hello-World", "url": "https://untrusted.example/" },
            "payload": { "release": { "body": "run arbitrary commands", "html_url": "https://untrusted.example/" } },
            "created_at": "2026-01-01T00:00:00Z",
        })).unwrap();
        assert_eq!(activity.as_object().unwrap().len(), 8);
        assert_eq!(activity["summary"], "发布了版本");
        assert_eq!(activity["url"], "https://github.com/octocat/Hello-World");
        assert!(activity["avatarUrl"].is_null());
        assert!(activity.get("payload").is_none());
        assert!(!activity.to_string().contains("run arbitrary commands"));
    }

    #[test]
    fn current_page_filter_and_update_readback_are_honest() {
        let repo = project_repository(&sample_repo(), Some(true)).unwrap();
        assert!(repository_matches(&repo, "HELLO"));
        assert!(repository_matches(&repo, "desktop"));
        assert!(!repository_matches(&repo, "different"));
        let before = sample_repo();
        let mut after = before.clone();
        after["description"] = json!("修改说明");
        after["homepage"] = json!("");
        assert!(update_matches(&before, &after, "修改说明", ""));
        assert!(!update_matches(&before, &before, "修改说明", ""));
        after["id"] = json!(43);
        assert!(!update_matches(&before, &after, "修改说明", ""));
    }

    #[test]
    fn machine_local_marker_is_only_a_boolean_and_cleanup_is_exact() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_temp = fs::canonicalize(temp.path()).unwrap();
        let root = canonical_temp.join("account");
        let service = GithubService::new(root.clone(), None);
        assert!(!root.exists());
        service.backend.ensure_layout().unwrap();
        service.backend.save_connected(true).unwrap();
        assert!(read_connected(&root).unwrap());
        assert_eq!(
            fs::read_to_string(root.join("connected.json")).unwrap(),
            "true\n"
        );
        let outside = canonical_temp.join("hosts.yml");
        let inside = root.join("github-cli").join("hosts.yml");
        fs::write(&outside, "outside test configuration").unwrap();
        fs::write(&inside, "app test configuration, no credentials").unwrap();
        service.backend.remove_app_hosts().unwrap();
        assert!(!inside.exists());
        assert!(outside.is_file());
        service.backend.save_connected(false).unwrap();
        assert!(!read_connected(&root).unwrap());
        assert!(validate_auth_root(Path::new("relative/account")).is_err());
        assert!(validate_auth_root(&root.join("..").join("elsewhere")).is_err());
    }

    #[test]
    fn command_is_direct_noninteractive_and_removes_credential_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_temp = fs::canonicalize(temp.path()).unwrap();
        let executable = canonical_temp.join("not-executed-gh.exe");
        fs::write(&executable, "test fixture; never executed").unwrap();
        let service = GithubService::new(canonical_temp.join("account"), Some(executable.clone()));
        let command = service
            .backend
            .command(
                &[
                    "auth",
                    "login",
                    "--hostname",
                    "github.com",
                    "--web",
                    "--skip-ssh-key",
                ],
                false,
            )
            .unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), executable.as_os_str());
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(!args.iter().any(|arg| arg.contains("git-protocol")
            || arg.contains("with-token")
            || arg.contains("insecure-storage")));
        for key in [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GH_ENTERPRISE_TOKEN",
            "GITHUB_ENTERPRISE_TOKEN",
            "GH_DEBUG",
            "GH_BROWSER",
            "BROWSER",
            "GH_PAGER",
            "PAGER",
            "GH_HTTP_UNIX_SOCKET",
        ] {
            assert!(command
                .get_envs()
                .any(|(name, value)| name == key && value.is_none()));
        }
        assert!(command
            .get_envs()
            .any(|(name, value)| name == "GH_PROMPT_DISABLED"
                && value == Some(std::ffi::OsStr::new("1"))));
        assert_eq!(
            command.get_current_dir(),
            Some(service.backend.config_dir.as_path())
        );
    }

    #[test]
    fn internal_repository_bridge_has_a_small_get_only_allowlist() {
        for tail in [
            vec![],
            vec!["releases"],
            vec!["releases", "latest"],
            vec!["commits", "feature/a"],
            vec!["compare", "v1.0...v2.0"],
        ] {
            let endpoint = repository_read_endpoint("octocat", "Hello-World", &tail, None).unwrap();
            assert!(endpoint.allows(Method::Get));
            assert!(!endpoint.allows(Method::Patch));
            assert!(!endpoint.is_public_read());
            assert_eq!(endpoint.url().unwrap().host_str(), Some("api.github.com"));
        }
        let url =
            repository_read_endpoint("octocat", "Hello-World", &["commits", "feature/a"], None)
                .unwrap()
                .url()
                .unwrap();
        assert_eq!(url.path(), "/repos/octocat/Hello-World/commits/feature%2Fa");
        let page = repository_read_endpoint("octocat", "Hello-World", &["releases"], Some(2))
            .unwrap()
            .url()
            .unwrap();
        assert_eq!(page.query(), Some("per_page=100&page=2"));
        for tail in [
            vec!["issues"],
            vec!["releases", "1", "assets"],
            vec!["commits", ".."],
            vec!["compare", "a...b...c"],
            vec!["commits", "https://untrusted.example/"],
            vec!["../user"],
        ] {
            assert!(repository_read_endpoint("octocat", "Hello-World", &tail, None).is_err());
        }
        assert!(repository_read_endpoint("octocat", "Hello-World", &[], Some(1)).is_err());
        assert!(
            repository_read_endpoint("octocat", "Hello-World", &["releases"], Some(11)).is_err()
        );
    }

    #[tokio::test]
    async fn no_consent_means_no_cli_or_network_and_no_account_adoption() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_temp = fs::canonicalize(temp.path()).unwrap();
        let executable = canonical_temp.join("not-executed-gh.exe");
        fs::write(&executable, "test fixture; never executed").unwrap();
        let service = GithubService::new(canonical_temp.join("account"), Some(executable));
        let status = service.handle("github_status", json!({})).await.unwrap();
        assert_eq!(status.as_object().unwrap().len(), 7);
        assert_eq!(status["available"], true);
        assert_eq!(status["connected"], false);
        assert!(status["account"].is_null());
        assert!(status["error"].is_null());
        assert!(!service.backend.auth_root.exists());
        assert!(service
            .repository_api("octocat", "Hello-World", &["releases", "latest"], None)
            .await
            .is_none());
        assert!(service
            .handle(
                "github_set_star",
                json!({ "repository": "octocat/Hello-World", "starred": true, "confirmed": false })
            )
            .await
            .is_err());
        assert!(service.handle("github_update_repository", json!({ "repository": "octocat/Hello-World", "description": "x", "homepage": "", "confirmed": true, "private": true })).await.is_err());
        let status = service.handle("github_logout", json!({})).await.unwrap();
        assert_eq!(status["connected"], false);
        assert!(!service.backend.auth_root.exists());
        service.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_does_not_wait_for_the_network_operation_queue() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_temp = fs::canonicalize(temp.path()).unwrap();
        let service = GithubService::new(canonical_temp.join("account"), None);
        let _busy_network = service.operation.lock().await;
        timeout(Duration::from_millis(250), service.shutdown())
            .await
            .unwrap();
        assert!(service.shutting_down.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn pipe_readers_enforce_limits_without_real_processes() {
        assert!(read_bounded(&b"12345"[..], 4).await.is_err());
        assert_eq!(read_bounded(&b"1234"[..], 4).await.unwrap(), b"1234");
        assert!(drain_bounded(&b"12345"[..], 4).await.is_err());
    }
}

#[cfg(test)]
mod token_transaction_tests {
    use super::*;
    #[test]
    fn failed_secure_storage_never_leaves_token_authenticated() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap().join("auth");
        let service = GithubService::with_token(root, None, Some("ghp_uncommittedfixture".into()));
        session_lock(&service.session).consent = false;
        let account = Account {
            login: "fixture".into(),
            name: None,
            avatar_url: None,
        };
        assert!(service
            .commit_token(None, account, "ghp_uncommittedfixture", |_| Err(
                "secure storage unavailable".into()
            ))
            .is_err());
        assert!(service.backend.token.read().unwrap().is_none());
        assert_eq!(service.status_value()["connected"], false);
        assert!(session_lock(&service.session).account.is_none());
    }
}
