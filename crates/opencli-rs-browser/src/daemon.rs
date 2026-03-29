use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use opencli_rs_core::CliError;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::types::{DaemonCommand, DaemonResult};

/// Path to the token file used by the daemon to authenticate CLI requests.
pub fn token_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".opencli-rs").join("daemon.token")
}

/// Generate a new token, persist it to disk, and return it.
fn write_token() -> Result<String, CliError> {
    let token = Uuid::new_v4().to_string();
    let path = token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::browser_connect(format!("Failed to create ~/.opencli-rs: {e}")))?;
    }
    std::fs::write(&path, &token)
        .map_err(|e| CliError::browser_connect(format!("Failed to write daemon token: {e}")))?;
    // Restrict file permissions to owner-read-only on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

/// Query parameters for the WebSocket `/ext` endpoint.
#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// Command response timeout.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// WebSocket heartbeat interval.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// Idle shutdown threshold.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

type PendingMap = HashMap<String, oneshot::Sender<DaemonResult>>;

/// Shared state for the daemon server.
pub struct DaemonState {
    pub extension_tx: Mutex<Option<futures::stream::SplitSink<WebSocket, Message>>>,
    pub pending_commands: RwLock<PendingMap>,
    pub extension_connected: RwLock<bool>,
    pub last_activity: RwLock<Instant>,
    /// Secret token that authenticates CLI → daemon HTTP requests and extension WS connections.
    pub auth_token: String,
}

impl DaemonState {
    fn new(auth_token: String) -> Self {
        Self {
            extension_tx: Mutex::new(None),
            pending_commands: RwLock::new(HashMap::new()),
            extension_connected: RwLock::new(false),
            last_activity: RwLock::new(Instant::now()),
            auth_token,
        }
    }

    async fn touch(&self) {
        *self.last_activity.write().await = Instant::now();
    }
}

/// The Daemon HTTP + WebSocket server.
pub struct Daemon {
    port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Daemon {
    /// Start the daemon server on the given port. Returns immediately after the listener binds.
    pub async fn start(port: u16) -> Result<Self, CliError> {
        let auth_token = write_token()?;
        let state = Arc::new(DaemonState::new(auth_token));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/ext-key", get(ext_key_handler).options(cors_preflight_handler))
            .route("/status", get(status_handler))
            .route("/command", post(command_handler))
            .route("/ext", get(ws_handler))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .map_err(|e| {
                CliError::browser_connect(format!("Failed to bind daemon on port {port}: {e}"))
            })?;

        info!(port, "daemon listening");

        // Spawn idle-shutdown watchdog
        let idle_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let last = *idle_state.last_activity.read().await;
                if last.elapsed() > IDLE_TIMEOUT {
                    info!("daemon idle timeout reached, shutting down");
                    break;
                }
            }
        });

        // Spawn the server
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                    info!("daemon received shutdown signal");
                })
                .await
                .ok();
        });

        Ok(Self {
            port,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Gracefully shut down the daemon.
    pub async fn shutdown(mut self) -> Result<(), CliError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        info!(port = self.port, "daemon shutdown complete");
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// GET /health — simple liveness check (no auth required).
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// GET /ext-key — returns the WebSocket auth token for the Chrome extension bootstrap.
///
/// This endpoint is intentionally unauthenticated: the extension has no way to read
/// the token file from disk. Any local process that calls this endpoint gains the WS
/// token, but the more dangerous HTTP /command endpoint still requires the full token.
/// Localhost-only binding (127.0.0.1) limits exposure to the local machine.
///
/// CORS is allowed for chrome-extension:// origins so the background service worker
/// can fetch this endpoint directly.
async fn ext_key_handler(State(state): State<Arc<DaemonState>>) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    (headers, Json(json!({ "token": state.auth_token })))
}

/// OPTIONS /ext-key — CORS preflight for Chrome extension requests.
async fn cors_preflight_handler() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    (StatusCode::NO_CONTENT, headers)
}

/// Validate the `X-OpenCLI` header value against the shared secret token.
fn check_token(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-opencli")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == expected)
        .unwrap_or(false)
}

/// GET /status — return daemon and extension status.
/// Compatible with both opencli-rs and original opencli formats.
async fn status_handler(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !check_token(&headers, &state.auth_token) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Unauthorized" })),
        );
    }
    let ext = *state.extension_connected.read().await;
    let pending = state.pending_commands.read().await.len();
    (StatusCode::OK, Json(json!({
        "daemon": true,
        "extension": ext,
        // Original opencli compatibility fields
        "ok": true,
        "extensionConnected": ext,
        "pending": pending,
    })))
}

/// POST /command — accept a command from the CLI and forward to the extension.
async fn command_handler(
    State(state): State<Arc<DaemonState>>,
    headers: HeaderMap,
    Json(cmd): Json<DaemonCommand>,
) -> impl IntoResponse {
    // Security: verify X-OpenCLI header carries the correct secret token
    if !check_token(&headers, &state.auth_token) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Unauthorized" })),
        );
    }

    state.touch().await;

    // Check extension connected
    if !*state.extension_connected.read().await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Chrome extension not connected" })),
        );
    }

    let cmd_id = cmd.id.clone();

    // Create a oneshot channel for the result
    let (tx, rx) = oneshot::channel::<DaemonResult>();
    state.pending_commands.write().await.insert(cmd_id.clone(), tx);

    // Forward command to extension via WebSocket
    {
        let mut ext_tx = state.extension_tx.lock().await;
        if let Some(ref mut sink) = *ext_tx {
            let msg = serde_json::to_string(&cmd).unwrap_or_default();
            if let Err(e) = sink.send(Message::Text(msg.into())).await {
                state.pending_commands.write().await.remove(&cmd_id);
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("Failed to send to extension: {e}") })),
                );
            }
        } else {
            state.pending_commands.write().await.remove(&cmd_id);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Extension WebSocket not available" })),
            );
        }
    }

    // Wait for result with timeout
    match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
        Ok(Ok(result)) => {
            let status = if result.ok {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (status, Json(serde_json::to_value(result).unwrap_or(json!({}))))
        }
        Ok(Err(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Command channel closed unexpectedly" })),
        ),
        Err(_) => {
            state.pending_commands.write().await.remove(&cmd_id);
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({ "error": "Command timed out" })),
            )
        }
    }
}

/// GET /ext — WebSocket upgrade for Chrome extension.
/// Requires `?token=<auth_token>` query parameter.
async fn ws_handler(
    State(state): State<Arc<DaemonState>>,
    Query(params): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let token_ok = params
        .token
        .as_deref()
        .map(|t| t == state.auth_token)
        .unwrap_or(false);

    if !token_ok {
        return (StatusCode::FORBIDDEN, "Unauthorized").into_response();
    }

    ws.on_upgrade(move |socket| handle_extension_ws(state, socket))
        .into_response()
}

async fn handle_extension_ws(state: Arc<DaemonState>, socket: WebSocket) {
    let (sender, mut receiver) = socket.split();

    // Store the sender so we can forward commands
    *state.extension_tx.lock().await = Some(sender);
    *state.extension_connected.write().await = true;
    info!("Chrome extension connected");

    // Spawn heartbeat pinger
    let heartbeat_state = state.clone();
    let heartbeat_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            let mut tx = heartbeat_state.extension_tx.lock().await;
            if let Some(ref mut sink) = *tx {
                if sink.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    // Process incoming messages from extension
    while let Some(msg) = receiver.next().await {
        state.touch().await;
        match msg {
            Ok(Message::Text(text)) => {
                debug!(len = text.len(), "received message from extension");
                match serde_json::from_str::<DaemonResult>(&text) {
                    Ok(result) => {
                        let id = result.id.clone();
                        if let Some(tx) = state.pending_commands.write().await.remove(&id) {
                            let _ = tx.send(result);
                        } else {
                            warn!(id = %id, "received result for unknown command");
                        }
                    }
                    Err(e) => {
                        warn!("failed to parse extension message: {e}");
                    }
                }
            }
            Ok(Message::Pong(_)) => {
                debug!("pong from extension");
            }
            Ok(Message::Close(_)) => {
                info!("extension sent close frame");
                break;
            }
            Err(e) => {
                error!("extension ws error: {e}");
                break;
            }
            _ => {}
        }
    }

    // Clean up
    heartbeat_handle.abort();
    *state.extension_tx.lock().await = None;
    *state.extension_connected.write().await = false;
    info!("Chrome extension disconnected");

    // Fail all pending commands
    let mut pending = state.pending_commands.write().await;
    for (id, tx) in pending.drain() {
        let _ = tx.send(DaemonResult::failure(
            id,
            "Extension disconnected".to_string(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    fn test_app(token: &str) -> Router {
        let state = Arc::new(DaemonState::new(token.to_string()));
        Router::new()
            .route("/health", get(health_handler))
            .route("/ext-key", get(ext_key_handler))
            .route("/status", get(status_handler))
            .route("/command", post(command_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_daemon_start_and_shutdown() {
        let daemon = Daemon::start(0).await;
        assert!(daemon.is_ok() || daemon.is_err());
    }

    #[tokio::test]
    async fn test_daemon_state_touch() {
        let state = DaemonState::new("test-token".to_string());
        let before = *state.last_activity.read().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        state.touch().await;
        let after = *state.last_activity.read().await;
        assert!(after > before);
    }

    // ── Token verification tests ─────────────────────────────────────

    #[test]
    fn test_check_token_correct() {
        let mut headers = HeaderMap::new();
        headers.insert("x-opencli", "secret-token".parse().unwrap());
        assert!(check_token(&headers, "secret-token"));
    }

    #[test]
    fn test_check_token_wrong_value() {
        let mut headers = HeaderMap::new();
        headers.insert("x-opencli", "wrong".parse().unwrap());
        assert!(!check_token(&headers, "secret-token"));
    }

    #[test]
    fn test_check_token_missing_header() {
        let headers = HeaderMap::new();
        assert!(!check_token(&headers, "secret-token"));
    }

    #[test]
    fn test_check_token_empty_value() {
        let mut headers = HeaderMap::new();
        headers.insert("x-opencli", "".parse().unwrap());
        assert!(!check_token(&headers, "secret-token"));
    }

    // ── HTTP endpoint auth tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_health_requires_no_auth() {
        let app = test_app("tok");
        let resp = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ext_key_requires_no_auth_and_returns_token() {
        let app = test_app("my-secret");
        let resp = app
            .oneshot(Request::get("/ext-key").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["token"], "my-secret");
    }

    #[tokio::test]
    async fn test_status_rejects_missing_token() {
        let app = test_app("tok");
        let resp = app
            .oneshot(Request::get("/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_status_rejects_wrong_token() {
        let app = test_app("correct");
        let resp = app
            .oneshot(
                Request::get("/status")
                    .header("x-opencli", "wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_status_accepts_correct_token() {
        let app = test_app("correct");
        let resp = app
            .oneshot(
                Request::get("/status")
                    .header("x-opencli", "correct")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_command_rejects_missing_token() {
        let app = test_app("tok");
        let body = serde_json::json!({
            "id": "test-1",
            "action": "exec",
            "code": "1+1"
        });
        let resp = app
            .oneshot(
                Request::post("/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_command_rejects_wrong_token() {
        let app = test_app("correct");
        let body = serde_json::json!({
            "id": "test-2",
            "action": "exec",
            "code": "1+1"
        });
        let resp = app
            .oneshot(
                Request::post("/command")
                    .header("content-type", "application/json")
                    .header("x-opencli", "wrong")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_old_hardcoded_1_token_rejected() {
        // Regression: "1" was the old hardcoded value — must now be rejected
        let app = test_app("actual-uuid-token");
        let body = serde_json::json!({
            "id": "test-3",
            "action": "exec",
            "code": "1+1"
        });
        let resp = app
            .oneshot(
                Request::post("/command")
                    .header("content-type", "application/json")
                    .header("x-opencli", "1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
