use std::{
    collections::HashMap,
    env, fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as RoutePath, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, task, time};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

use crate::{
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
    downloads: Arc<Mutex<HashMap<String, Download>>>,
    local: bool,
    origin_secret: Option<String>,
    dev_user: Option<String>,
    microsoft_client_id: Option<String>,
    microsoft_tenant_id: Option<String>,
}

struct Download {
    owner: String,
    path: PathBuf,
    expires: Instant,
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
            tracing::error!(error = %self.error, "SOC request failed");
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
    let address = config
        .bind
        .parse::<SocketAddr>()
        .map_err(|_| invalid("SOC_BIND must be an IP address and port"))?;
    let base = normalise_base(&config.base)?;
    if !config.frontend.join("index.html").is_file() {
        return Err(invalid(format!(
            "Build the SOC frontend first; {} is missing",
            config.frontend.join("index.html").display()
        )));
    }
    if !address.ip().is_loopback() && env::var_os("SOC_ORIGIN_SECRET").is_none() {
        return Err(invalid(
            "Set SOC_ORIGIN_SECRET before listening beyond this computer",
        ));
    }
    let store = Store::open(&config.data)?;
    let state = App {
        store: store.clone(),
        cache: ArtifactCache::new(config.data.join("cache"), CACHE_BUDGET)?,
        profile: FormProfile::bundled()?,
        rooms: Arc::new(rooms::read(None)?),
        data: config.data,
        downloads: Arc::new(Mutex::new(HashMap::new())),
        local: address.ip().is_loopback(),
        origin_secret: env::var("SOC_ORIGIN_SECRET").ok(),
        dev_user: env::var("SOC_DEV_USER").ok(),
        microsoft_client_id: env::var("SOC_MICROSOFT_CLIENT_ID").ok(),
        microsoft_tenant_id: env::var("SOC_MICROSOFT_TENANT_ID").ok(),
    };
    let poll_store = store;
    let poll_interval = config.poll_interval;
    let poller = tokio::spawn(async move {
        let mut interval = time::interval(poll_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = agent::poll_all(&poll_store).await {
                tracing::warn!(%error, "background booking poll failed");
            }
        }
    });
    let files = ServeDir::new(&config.frontend)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(config.frontend.join("index.html")));
    let scoped = Router::new()
        .route("/api/health", get(health))
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
        .with_state(state)
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());
    let router = if base == "/" {
        scoped
    } else {
        Router::new()
            .route("/health", get(health))
            .nest(&base, scoped)
    };
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| SocError::Io {
            context: format!("Cannot listen on {address}"),
            source: error,
        })?;
    tracing::info!(%address, %base, "SOC Minis is ready");
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|error| SocError::Io {
            context: "SOC server stopped unexpectedly".into(),
            source: error,
        });
    poller.abort();
    let _ = poller.await;
    result
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn account_status(
    State(app): State<App>,
    headers: HeaderMap,
) -> WebResult<Json<AccountStatus>> {
    let owner = owner(&app, &headers)?;
    Ok(Json(AccountStatus {
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
    owner(&app, &headers)?;
    Ok(Json(rooms::search(&app.rooms, &query.q, query.minimum)?))
}

async fn prepare_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventRequest>,
) -> WebResult<Json<EventResponse>> {
    let owner = owner(&app, &headers)?;
    if request.source.len() > SOURCE_LIMIT {
        return Err(invalid("Keep the event source under 128 KiB").into());
    }
    if request.submit && !request.authorised {
        return Err(invalid(
            "Review the booking preview and authorise this specific Forms submission",
        )
        .into());
    }
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
        forms::submit(&profile, payload, &profile.secrets()?).await?;
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
    let owner = owner(&app, &headers)?;
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
    let owner = owner(&app, &headers)?;
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
    let owner = owner(&app, &headers)?;
    Ok(Json(
        graph::poll_device_login(&app.store, &owner, &id).await?,
    ))
}

async fn agent_status(
    State(app): State<App>,
    headers: HeaderMap,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers)?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn watch_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<WatchRequest>,
) -> WebResult<Json<agent::EventStatus>> {
    let owner = owner(&app, &headers)?;
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
    let owner = owner(&app, &headers)?;
    agent::poll(&app.store, &owner).await?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn stop_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventAction>,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers)?;
    agent::stop(&app.store, &owner, &request.event_id)?;
    Ok(Json(agent::status(&app.store, &owner)?))
}

async fn retry_send(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<EventAction>,
) -> WebResult<Json<Vec<agent::EventStatus>>> {
    let owner = owner(&app, &headers)?;
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

fn owner(app: &App, headers: &HeaderMap) -> WebResult<String> {
    if let Some(secret) = &app.origin_secret {
        let supplied = headers
            .get("x-soc-origin-secret")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !constant_time(secret.as_bytes(), supplied.as_bytes()) {
            return Err(WebError {
                status: StatusCode::UNAUTHORIZED,
                error: invalid("Open SOC Minis through its protected address"),
            });
        }
    } else if !app.local {
        return Err(WebError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: invalid("SOC Minis access protection is not configured"),
        });
    }
    let value = if app.local {
        headers
            .get("x-soc-user")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .or_else(|| app.dev_user.clone())
            .unwrap_or_else(|| "local@localhost".into())
    } else {
        headers
            .get("cf-access-authenticated-user-email")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let value = value.trim().to_lowercase();
    if value.is_empty()
        || value.len() > 254
        || value.chars().any(char::is_control)
        || !value.contains('@')
    {
        return Err(WebError {
            status: StatusCode::UNAUTHORIZED,
            error: invalid("Sign in through the protected SOC Minis address"),
        });
    }
    Ok(value)
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

fn constant_time(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_base_paths_and_owner_keys() {
        assert_eq!(normalise_base("/soc/").expect("base"), "/soc");
        assert!(normalise_base("soc").is_err());
        assert!(normalise_base("/../admin").is_err());
        assert_eq!(owner_key("member@example.org").len(), 24);
        assert!(constant_time(b"secret", b"secret"));
        assert!(!constant_time(b"secret", b"other!"));
    }
}
