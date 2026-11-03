use std::{
    collections::HashMap,
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, MatchedPath, Path as RoutePath, Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    task, time,
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
};
use uuid::Uuid;

use crate::{
    access::{Access, AccessError},
    agent::{self, Watch},
    cache::{ArtifactCache, write_private},
    db::Store,
    error::{Result, SocError, invalid},
    event::{self, Options},
    forms::{self, FormProfile},
    graph::{self, DeviceStatus},
    model::{Event, OutputMode, RiskLayout},
    rooms::{self, Room},
};

const CACHE_BUDGET: u64 = 128 * 1024 * 1024;
const SOURCE_LIMIT: usize = 128 * 1024;

pub struct Config {
    pub bind: String,
    pub socket: Option<PathBuf>,
    pub base: String,
    pub data: PathBuf,
    pub frontend: PathBuf,
    pub poll_interval: Duration,
}

#[derive(Clone)]
struct App {
    store: Store,
    cache: ArtifactCache,
    profile: FormProfile,
    rooms: Arc<Vec<Room>>,
    data: PathBuf,
    frontend_index: PathBuf,
    downloads: Arc<Mutex<HashMap<String, Download>>>,
    access: Access,
    limits: Limits,
    microsoft_client_id: Option<String>,
    microsoft_tenant_id: Option<String>,
}

struct Download {
    owner: String,
    path: PathBuf,
    expires: Instant,
}

#[derive(Clone)]
struct Limits {
    global: Arc<Semaphore>,
    owners: Arc<Vec<Arc<Semaphore>>>,
    mail: Arc<Semaphore>,
}

struct WorkPermits {
    _global: OwnedSemaphorePermit,
    _owner: OwnedSemaphorePermit,
}

#[derive(Debug)]
struct WebError {
    status: StatusCode,
    error: SocError,
}

impl From<SocError> for WebError {
    fn from(error: SocError) -> Self {
        let status = match error {
            SocError::Message(_) | SocError::Json(_) | SocError::Toml(_) | SocError::Csv(_) => {
                StatusCode::BAD_REQUEST
            }
            SocError::Http(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self { status, error }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let message = if self.status.is_server_error() {
            tracing::error!(category = error_kind(&self.error), "SOC request failed");
            "SOC Minis couldn’t finish that just now. Your saved work is unchanged".to_owned()
        } else {
            self.error.to_string()
        };
        (self.status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

type WebResult<T> = std::result::Result<T, WebError>;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

#[derive(Serialize)]
struct AccountStatus {
    workspace: String,
    microsoft_mailbox: Option<String>,
    microsoft_ready: bool,
    forms_ready: bool,
}

#[derive(Deserialize)]
struct RoomQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    minimum: u32,
}

#[derive(Deserialize)]
struct EventRequest {
    source: String,
    #[serde(default = "both")]
    mode: OutputMode,
    #[serde(default = "risk_form")]
    layout: RiskLayout,
    #[serde(default)]
    submit: bool,
    #[serde(default)]
    authorised: bool,
}

const fn both() -> OutputMode {
    OutputMode::Both
}

const fn risk_form() -> RiskLayout {
    RiskLayout::Form
}

#[derive(Serialize)]
struct EventResponse {
    title: String,
    submitted: bool,
    preview: Option<Value>,
    downloads: Vec<FileLink>,
}

#[derive(Serialize)]
struct FileLink {
    kind: String,
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    mailbox: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    tenant_id: String,
}

#[derive(Deserialize)]
struct WatchRequest {
    source: String,
    event_id: String,
    #[serde(default = "return_address")]
    recipient: String,
    since: Option<String>,
}

fn return_address() -> String {
    "pfeventa@uq.edu.au".into()
}

#[derive(Deserialize)]
struct EventAction {
    event_id: String,
    #[serde(default)]
    confirm_not_sent: bool,
}

pub async fn serve(config: Config) -> Result<()> {
    let address = if config.socket.is_none() {
        Some(
            config
                .bind
                .parse::<SocketAddr>()
                .map_err(|_| invalid("SOC_BIND must be an IP address and port"))?,
        )
    } else {
        None
    };
    let base = normalise_base(&config.base)?;
    let frontend_index = config.frontend.join("index.html");
    if !frontend_index.is_file() {
        return Err(invalid(format!(
            "Build the SOC frontend first; {} is missing",
            frontend_index.display()
        )));
    }
    let local = address.is_some_and(|address| address.ip().is_loopback());
    let access = Access::from_env(local)?;
    let store = Store::open(&config.data)?;
    let state = App {
        store: store.clone(),
        cache: ArtifactCache::new(config.data.join("cache"), CACHE_BUDGET)?,
        profile: FormProfile::bundled()?,
        rooms: Arc::new(rooms::read(None)?),
        data: config.data.clone(),
        frontend_index,
        downloads: Arc::new(Mutex::new(HashMap::new())),
        access,
        limits: Limits::new(),
        microsoft_client_id: env::var("SOC_MICROSOFT_CLIENT_ID").ok(),
        microsoft_tenant_id: env::var("SOC_MICROSOFT_TENANT_ID").ok(),
    };
    let poll_store = store;
    let poll_limits = state.limits.clone();
    let poll_interval = config.poll_interval;
    let poller = tokio::spawn(async move {
        let mut interval = time::interval(poll_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let Ok(_mail) = poll_limits.mail.clone().try_acquire_owned() else {
                tracing::warn!(
                    "background booking poll skipped because another mail pass is active"
                );
                continue;
            };
            if let Err(error) = agent::poll_all(&poll_store).await {
                tracing::warn!(
                    category = error_kind(&error),
                    "background booking poll failed"
                );
            }
        }
    });
    let router = router(&config, &base, state);
    let result = if let Some(path) = config.socket.as_deref() {
        serve_unix(path, router, &base).await
    } else {
        let address = address.ok_or_else(|| invalid("SOC_BIND is missing"))?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(|error| SocError::Io {
                context: format!("Cannot listen on {address}"),
                source: error,
            })?;
        tracing::info!(%address, %base, "SOC Minis is ready");
        serve_listener(listener, router).await
    };
    poller.abort();
    let _ = poller.await;
    result
}

fn router(config: &Config, base: &str, state: App) -> Router {
    let index = config.frontend.join("index.html");
    let files = ServeDir::new(&config.frontend).append_index_html_on_directories(false);
    let hosted = state.access.hosted();
    let scoped = Router::<App>::new()
        .route_service("/", ServeFile::new(index.clone()))
        .route_service("/eventy", ServeFile::new(index.clone()))
        .route_service("/monkey", ServeFile::new(index.clone()))
        .route_service("/meddy", ServeFile::new(index))
        .route("/api/health", get(live))
        .route("/api/health/live", get(live))
        .route("/api/health/ready", get(ready))
        .route("/api/status", get(account_status))
        .route("/api/rooms", get(search_rooms))
        .route("/api/event", post(prepare_event))
        .route("/api/download/{id}", get(download))
        .route("/api/microsoft/start", post(start_login))
        .route("/api/microsoft/poll/{id}", post(poll_login))
        .route("/api/agent/status", get(agent_status))
        .route("/api/agent/watch", post(watch_event))
        .route("/api/agent/poll", post(poll_agent))
        .route("/api/agent/stop", post(stop_event))
        .route("/api/agent/retry", post(retry_send))
        .fallback_service(files)
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(CompressionLayer::new());
    let app = if base == "/" {
        scoped
            .route("/health", get(live))
            .route("/health/live", get(live))
            .route("/health/ready", get(ready))
    } else {
        let shell = format!("{base}/");
        Router::new()
            .route("/health", get(live))
            .route("/health/live", get(live))
            .route("/health/ready", get(ready))
            .route_service(base, ServeFile::new(config.frontend.join("index.html")))
            .nest(&shell, scoped)
    };
    app.with_state(state)
        .layer(middleware::from_fn(move |request, next| {
            request_context(request, next, hosted)
        }))
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let terminate = async {
            let Ok(mut signal) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                return;
            };
            signal.recv().await;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

async fn serve_listener<L>(listener: L, router: Router) -> Result<()>
where
    L: axum::serve::Listener,
    L::Addr: std::fmt::Debug,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|error| SocError::Io {
            context: "SOC server stopped unexpectedly".into(),
            source: error,
        })
}

#[cfg(unix)]
async fn serve_unix(path: &Path, router: Router, base: &str) -> Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    validate_socket_path(path)?;
    let listener = tokio::net::UnixListener::bind(path).map_err(|source| SocError::Io {
        context: format!("Cannot listen on SOC_SOCKET {}", path.display()),
        source,
    })?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) => {
            drop(listener);
            let _ = fs::remove_file(path);
            return Err(SocError::Io {
                context: format!("Cannot inspect bound SOC_SOCKET {}", path.display()),
                source,
            });
        }
    };
    if let Err(source) = fs::set_permissions(path, fs::Permissions::from_mode(0o660)) {
        drop(listener);
        let _ = fs::remove_file(path);
        return Err(SocError::Io {
            context: format!("Cannot protect SOC_SOCKET {}", path.display()),
            source,
        });
    }
    let identity = (metadata.dev(), metadata.ino());
    tracing::info!(socket = %path.display(), %base, "SOC Minis is ready");
    let result = serve_listener(listener, router).await;
    if fs::symlink_metadata(path).is_ok_and(|current| {
        current.file_type().is_socket() && (current.dev(), current.ino()) == identity
    }) {
        let _ = fs::remove_file(path);
    }
    result
}

#[cfg(unix)]
fn validate_socket_path(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid("SOC_SOCKET must be an absolute socket path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("SOC_SOCKET must have a parent directory"))?;
    for ancestor in parent.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|source| SocError::Io {
            context: format!("Cannot inspect SOC_SOCKET directory {}", ancestor.display()),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                "SOC_SOCKET must be inside a private, non-symlink directory",
            ));
        }
    }
    let parent_metadata = fs::symlink_metadata(parent).map_err(|source| SocError::Io {
        context: format!("Cannot inspect SOC_SOCKET directory {}", parent.display()),
        source,
    })?;
    if !parent_metadata.is_dir() || parent_metadata.permissions().mode() & 0o022 != 0 {
        return Err(invalid(
            "SOC_SOCKET must be inside a private, non-symlink directory",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {
            return Err(invalid(format!(
                "SOC_SOCKET already exists at {}; remove the stale socket before starting",
                path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(SocError::Io {
                context: format!("Cannot inspect SOC_SOCKET {}", path.display()),
                source,
            });
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn serve_unix(_path: &Path, _router: Router, _base: &str) -> Result<()> {
    Err(invalid("SOC_SOCKET is only available on Unix systems"))
}

async fn live() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn ready(State(app): State<App>) -> Response {
    let store = app.store.clone();
    let database = task::spawn_blocking(move || store.ready()).await;
    let ready = matches!(database, Ok(Ok(()))) && app.frontend_index.is_file();
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Health {
            status: if ready { "ok" } else { "not_ready" },
        }),
    )
        .into_response()
}

async fn account_status(
    State(app): State<App>,
    headers: HeaderMap,
) -> WebResult<Json<AccountStatus>> {
    let owner = owner(&app, &headers).await?;
    Ok(Json(AccountStatus {
        workspace: owner_key(&owner),
        microsoft_mailbox: graph::connected_mailbox(&app.store, &owner)?,
        microsoft_ready: app.microsoft_client_id.is_some() && app.microsoft_tenant_id.is_some(),
        forms_ready: app.profile.secrets().is_ok(),
    }))
}

async fn search_rooms(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<RoomQuery>,
) -> WebResult<Json<Vec<Room>>> {
    owner(&app, &headers).await?;
    Ok(Json(rooms::search(&app.rooms, &query.q, query.minimum)?))
}

async fn prepare_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventRequest>,
) -> WebResult<Json<EventResponse>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    if request.source.len() > SOURCE_LIMIT {
        return Err(invalid("Keep the event source under 128 KiB").into());
    }
    if request.submit && !request.authorised {
        return Err(invalid(
            "Review the booking preview and authorise this specific Forms submission",
        )
        .into());
    }
    let submission_key = request.submit.then(|| request_key(&request)).transpose()?;
    let directory = app
        .data
        .join("events")
        .join(owner_key(&owner))
        .join(Uuid::new_v4().to_string());
    let source = directory.join("event.toml");
    let source_text = request.source;
    let profile = app.profile.clone();
    let task_profile = profile.clone();
    let cache = app.cache.clone();
    let options = Options {
        mode: request.mode,
        layout: request.layout,
        destination: directory.clone(),
        force: false,
    };
    let generated = task::spawn_blocking(move || {
        write_private(&source, source_text.as_bytes(), false)?;
        let event = Event::load(&source)?;
        let result = event::generate(&source, &options, &cache, &task_profile);
        match result {
            Ok(generated) => Ok((event.title, source, generated)),
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| invalid("Event preparation stopped before it finished"))??;
    let (title, source, generated) = generated;
    if request.submit {
        let payload = generated.payload.as_ref().ok_or_else(|| {
            invalid("Choose form or both output before requesting a Forms submission")
        })?;
        let secrets = profile.secrets()?;
        let request_key = submission_key
            .as_deref()
            .ok_or_else(|| invalid("The submission identity is missing"))?;
        if !app.store.claim_once(
            &owner,
            "forms-submit",
            request_key,
            Duration::from_secs(15 * 60),
        )? {
            if let Some(directory) = source.parent() {
                let _ = fs::remove_dir_all(directory);
            }
            return Err(conflict(
                "This exact booking was already submitted recently; check Forms before trying again",
            ));
        }
        forms::submit(&profile, payload, &secrets).await?;
    }
    let preview = generated
        .payload
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(SocError::from)?;
    let mut outputs = generated
        .outputs
        .into_iter()
        .map(|output| (output.kind, output.path))
        .collect::<Vec<_>>();
    outputs.insert(0, ("source".into(), source));
    let downloads = outputs
        .into_iter()
        .map(|(kind, path)| app.register(&owner, kind, path))
        .collect::<WebResult<Vec<_>>>()?;
    Ok(Json(EventResponse {
        title,
        submitted: request.submit,
        preview,
        downloads,
    }))
}

async fn download(
    State(app): State<App>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> WebResult<Response> {
    let owner = owner(&app, &headers).await?;
    let path = {
        let mut downloads = app
            .downloads
            .lock()
            .map_err(|_| invalid("Downloads are temporarily unavailable"))?;
        downloads.retain(|_, item| item.expires > Instant::now());
        let item = downloads
            .get(&id)
            .filter(|item| item.owner == owner)
            .ok_or_else(|| invalid("That download expired; prepare the event again"))?;
        item.path.clone()
    };
    if !path.is_file() || path.is_symlink() {
        return Err(invalid("That generated file is no longer available").into());
    }
    let bytes = fs::read(&path).map_err(|source| SocError::Io {
        context: "Cannot read the generated file".into(),
        source,
    })?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("The generated filename is invalid"))?;
    let content_type = match path.extension().and_then(|value| value.to_str()) {
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("json") => "application/json",
        Some("toml") => "application/toml",
        _ => "application/octet-stream",
    };
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            name.replace(['\"', '\\'], "_")
        ))
        .map_err(|_| invalid("The generated filename is invalid"))?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

async fn start_login(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> WebResult<Json<graph::DeviceFlow>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    let client_id = configured(
        request.client_id,
        app.microsoft_client_id.as_deref(),
        "client ID",
    )?;
    let tenant_id = configured(
        request.tenant_id,
        app.microsoft_tenant_id.as_deref(),
        "tenant ID",
    )?;
    Ok(Json(
        graph::start_device_login(&app.store, &owner, &request.mailbox, client_id, tenant_id)
            .await?,
    ))
}

async fn poll_login(
    State(app): State<App>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> WebResult<Json<DeviceStatus>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    Ok(Json(
        graph::poll_device_login(&app.store, &owner, &id).await?,
    ))
}

async fn agent_status(
    State(app): State<App>,
    headers: HeaderMap,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers).await?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn watch_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<WatchRequest>,
) -> WebResult<Json<agent::EventStatus>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    if request.source.len() > SOURCE_LIMIT {
        return Err(invalid("Keep the event source under 128 KiB").into());
    }
    let source = app
        .data
        .join("events")
        .join(owner_key(&owner))
        .join(format!("watch-{}.toml", Uuid::new_v4()));
    write_private(&source, request.source.as_bytes(), false)?;
    let result = agent::watch(
        &app.store,
        &app.cache,
        &owner,
        &Watch {
            source: &source,
            event_id: &request.event_id,
            attachment: None,
            recipient: &request.recipient,
            since: request.since.as_deref(),
        },
    );
    let _ = fs::remove_file(source);
    Ok(Json(result?))
}

async fn poll_agent(
    State(app): State<App>,
    headers: HeaderMap,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    let _mail = app.limits.mail()?;
    agent::poll(&app.store, &owner).await?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn stop_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventAction>,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers).await?;
    agent::stop(&app.store, &owner, &request.event_id)?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn retry_send(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventAction>,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers).await?;
    let _work = app.limits.acquire(&owner)?;
    let _mail = app.limits.mail()?;
    agent::retry_send(
        &app.store,
        &owner,
        &request.event_id,
        request.confirm_not_sent,
    )?;
    agent::poll(&app.store, &owner).await?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

impl App {
    fn register(&self, owner: &str, kind: String, path: PathBuf) -> WebResult<FileLink> {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("The generated filename is invalid"))?
            .to_owned();
        let id = Uuid::new_v4().to_string();
        let mut downloads = self
            .downloads
            .lock()
            .map_err(|_| invalid("Downloads are temporarily unavailable"))?;
        downloads.retain(|_, item| item.expires > Instant::now());
        downloads.insert(
            id.clone(),
            Download {
                owner: owner.to_owned(),
                path,
                expires: Instant::now() + Duration::from_secs(1800),
            },
        );
        Ok(FileLink {
            kind,
            name,
            url: format!("api/download/{id}"),
        })
    }
}

impl Limits {
    fn new() -> Self {
        Self {
            global: Arc::new(Semaphore::new(4)),
            owners: Arc::new((0..256).map(|_| Arc::new(Semaphore::new(1))).collect()),
            mail: Arc::new(Semaphore::new(1)),
        }
    }

    fn acquire(&self, owner: &str) -> WebResult<WorkPermits> {
        let global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        let digest = Sha256::digest(owner.as_bytes());
        let slot = usize::from(digest[0]);
        let owner = self.owners[slot]
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        Ok(WorkPermits {
            _global: global,
            _owner: owner,
        })
    }

    fn mail(&self) -> WebResult<OwnedSemaphorePermit> {
        self.mail.clone().try_acquire_owned().map_err(|_| busy())
    }
}

async fn owner(app: &App, headers: &HeaderMap) -> WebResult<String> {
    match app.access.owner(headers).await {
        Ok(owner) => Ok(owner),
        Err(AccessError::Invalid) => Err(WebError {
            status: StatusCode::UNAUTHORIZED,
            error: invalid("Sign in through the protected SOC Minis address"),
        }),
        Err(AccessError::Unavailable) => Err(WebError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: invalid("SOC Minis could not verify access just now"),
        }),
    }
}

fn configured<'a>(provided: String, fallback: Option<&'a str>, name: &str) -> WebResult<&'a str> {
    if provided.is_empty() {
        return fallback.ok_or_else(|| invalid(format!("Configure the Microsoft {name}")).into());
    }
    Err(invalid(format!(
        "Configure the Microsoft {name} on the server rather than sending it from the page"
    ))
    .into())
}

fn owner_key(owner: &str) -> String {
    format!("{:x}", Sha256::digest(owner.as_bytes()))[..24].to_owned()
}

fn request_key(request: &EventRequest) -> Result<String> {
    let value = serde_json::to_vec(&(request.mode, request.layout, &request.source))?;
    Ok(format!("{:x}", Sha256::digest(value)))
}

fn normalise_base(value: &str) -> Result<String> {
    let value = value.trim();
    if !value.starts_with('/')
        || value.contains(['?', '#', '\\'])
        || value.split('/').any(|part| part == "..")
    {
        return Err(invalid("SOC_BASE must be a safe absolute URL path"));
    }
    let value = value.trim_end_matches('/');
    Ok(if value.is_empty() { "/" } else { value }.to_owned())
}

fn busy() -> WebError {
    WebError {
        status: StatusCode::TOO_MANY_REQUESTS,
        error: invalid("Another SOC task is already running; try again shortly"),
    }
}

fn conflict(message: &str) -> WebError {
    WebError {
        status: StatusCode::CONFLICT,
        error: invalid(message),
    }
}

fn error_kind(error: &SocError) -> &'static str {
    match error {
        SocError::Message(_) => "request",
        SocError::Io { .. } => "io",
        SocError::Database(_) => "database",
        SocError::Json(_) => "json",
        SocError::Toml(_) => "toml",
        SocError::Http(_) => "upstream",
        SocError::Zip(_) => "document",
        SocError::Csv(_) => "catalogue",
    }
}

async fn request_context(mut request: Request, next: Next, hosted: bool) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let private =
        request.uri().path().contains("/api/") || request.uri().path().starts_with("/health");
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    request.headers_mut().remove("x-request-id");
    request.headers_mut().remove("x-soc-user");
    request
        .headers_mut()
        .remove("cf-access-authenticated-user-email");
    let started = Instant::now();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'",
        ),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin-allow-popups"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    if private {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if hosted {
        headers.insert(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    tracing::info!(
        %request_id,
        %method,
        %route,
        status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_millis(),
        "SOC request"
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[test]
    fn protects_base_paths_and_owner_keys() {
        assert_eq!(normalise_base("/soc/").expect("base"), "/soc");
        assert!(normalise_base("soc").is_err());
        assert!(normalise_base("/../admin").is_err());
        assert_eq!(owner_key("member@example.org").len(), 24);
    }

    #[test]
    fn bounds_parallel_work_per_owner() {
        let limits = Limits::new();
        let work = limits.acquire("owner@example.org").expect("first task");
        assert!(matches!(
            limits.acquire("owner@example.org"),
            Err(WebError {
                status: StatusCode::TOO_MANY_REQUESTS,
                ..
            })
        ));
        drop(work);
        assert!(limits.acquire("owner@example.org").is_ok());

        let mail = limits.mail().expect("first mail pass");
        assert!(matches!(
            limits.mail(),
            Err(WebError {
                status: StatusCode::TOO_MANY_REQUESTS,
                ..
            })
        ));
        drop(mail);
        assert!(limits.mail().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn requires_a_private_absolute_socket_path() {
        use std::os::unix::fs::symlink;

        let temporary = fs::canonicalize(std::env::temp_dir()).expect("canonical temporary path");
        let root = temporary.join(format!("soc-socket-{}", Uuid::new_v4()));
        fs::create_dir(&root).expect("private socket directory");
        let path = root.join("soc.sock");
        assert!(validate_socket_path(&path).is_ok());
        assert!(validate_socket_path(Path::new("soc.sock")).is_err());

        let link = root.with_extension("link");
        symlink(&root, &link).expect("socket directory symlink");
        assert!(validate_socket_path(&link.join("soc.sock")).is_err());
        fs::remove_file(link).expect("clean symlink");
        fs::remove_dir(root).expect("clean socket directory");
    }

    #[tokio::test]
    async fn serves_the_scoped_shell_assets_and_health() {
        let root = std::env::temp_dir().join(format!("soc-web-{}", Uuid::new_v4()));
        let frontend = root.join("frontend");
        fs::create_dir_all(frontend.join("assets")).expect("frontend directories");
        fs::write(
            frontend.join("index.html"),
            "<!doctype html><title>SOC</title>",
        )
        .expect("frontend shell");
        fs::write(frontend.join("assets/app.js"), "export {}").expect("frontend asset");
        let data = root.join("data");
        let frontend_index = frontend.join("index.html");
        let state = App {
            store: Store::open(&data).expect("store"),
            cache: ArtifactCache::new(data.join("cache"), CACHE_BUDGET).expect("cache"),
            profile: FormProfile::bundled().expect("profile"),
            rooms: Arc::new(rooms::read(None).expect("rooms")),
            data: data.clone(),
            frontend_index,
            downloads: Arc::new(Mutex::new(HashMap::new())),
            access: Access::local(None).expect("local access"),
            limits: Limits::new(),
            microsoft_client_id: None,
            microsoft_tenant_id: None,
        };
        let config = Config {
            bind: "127.0.0.1:0".into(),
            socket: None,
            base: "/soc".into(),
            data,
            frontend,
            poll_interval: Duration::from_secs(60),
        };
        let status = account_status(State(state.clone()), HeaderMap::new())
            .await
            .expect("account status")
            .0;
        assert_eq!(status.workspace, owner_key("local@localhost"));
        let app = router(&config, "/soc", state);
        for path in [
            "/soc",
            "/soc/",
            "/soc/eventy",
            "/soc/monkey",
            "/soc/meddy",
            "/soc/assets/app.js",
            "/soc/api/health",
            "/soc/api/health/live",
            "/soc/api/health/ready",
            "/health",
            "/health/live",
            "/health/ready",
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert!(response.headers().contains_key("x-request-id"), "{path}");
            assert_eq!(
                response
                    .headers()
                    .get("x-content-type-options")
                    .and_then(|value| value.to_str().ok()),
                Some("nosniff"),
                "{path}"
            );
        }
        for path in ["/soc/unknown", "/soc/api/unknown", "/unknown"] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
        fs::remove_dir_all(root).expect("clean state");
    }
}
