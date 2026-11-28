//! HTTP layer: GraphQL over POST + WebSocket, health check, static admin UI,
//! client self-registration and the `/t/<secret>/` HTTP tunnels.

use crate::app::App;
use crate::auth;
use crate::graphql::{AppSchema, RequestMeta, SessionToken};
use async_graphql::Data;
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use crate::db::{ClientDoc, ServiceDoc};
use crate::engine::Engine;
use axum::body::Body;
use axum::extract::{Path, Request, State, WebSocketUpgrade};
use axum::Json;
use mongodb::bson::{DateTime, doc, oid::ObjectId};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    app: Arc<App>,
    schema: AppSchema,
}

pub fn router(app: Arc<App>, schema: AppSchema, web_dir: PathBuf) -> Router {
    let not_found = ServeFile::new(web_dir.join("404.html"));
    let files = ServeDir::new(&web_dir).append_index_html_on_directories(true).not_found_service(not_found);
    Router::new()
        .route("/api/graphql", post(graphql))
        .route("/api/graphql/ws", get(graphql_ws))
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/register", post(register))
        .fallback_service(files)
        .with_state(AppState { app: app.clone(), schema })
        .merge(tunnel_router(app.engine.clone()))
}

/// `/t/<secret>/<path>` → `/<path>` on the local service of an `http` tunnel.
pub fn tunnel_router(engine: Engine) -> Router {
    Router::new().route("/t/{secret}", any(tunnel)).route("/t/{secret}/{*rest}", any(tunnel)).with_state(engine)
}

async fn tunnel(State(engine): State<Engine>, Path(p): Path<HashMap<String, String>>, mut req: Request) -> Response {
    let Some(stream) = engine.open_http(&proto::sha256_hex(p["secret"].as_bytes())).await else {
        return (StatusCode::NOT_FOUND, "unknown tunnel").into_response();
    };
    let result = async {
        let (mut sender, conn) = hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream?)).await?;
        tokio::spawn(conn);
        let rest = p.get("rest").map_or("", String::as_str);
        let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
        *req.uri_mut() = format!("/{rest}{query}").parse()?;
        anyhow::Ok(sender.send_request(req).await?.map(Body::new))
    }
    .await;
    result.unwrap_or_else(|e| (StatusCode::BAD_GATEWAY, format!("tunnel unavailable: {e:#}")).into_response())
}

/// Where self-registered clients forward their tunnel unless they ask for another port (the Rustart MCP server).
const REGISTERED_LOCAL_PORT: u16 = 8765;
/// shortcut: one global budget for new registrations, per process; use a per-IP limit if it gets abused.
const REGISTRATIONS_PER_HOUR: u32 = 30;

#[derive(Deserialize, Default)]
struct RegisterReq {
    /// Token of an already registered client: issues a new URL secret instead.
    token: Option<String>,
    /// Local port (on the client's own loopback) the tunnel forwards to. Default: 8765.
    port: Option<u16>,
}

/// Creates a client with one `http` service (or rotates its URL secret).
async fn register(State(st): State<AppState>, body: Option<Json<RegisterReq>>) -> Response {
    let app = &st.app;
    if !app.allow_registration.load(std::sync::atomic::Ordering::Relaxed) {
        return (StatusCode::NOT_FOUND, "registration is disabled").into_response();
    }
    let Some(host) = &app.public_host else {
        return (StatusCode::SERVICE_UNAVAILABLE, "PUBLIC_HOST is not set").into_response();
    };
    let secret = auth::random_token(24);
    let http_key = proto::sha256_hex(secret.as_bytes());
    let body = body.map(|Json(b)| b).unwrap_or_default();
    // Unprivileged ports only; with a token and no port, the client's current port is kept.
    if body.port.is_some_and(|p| p < 1024) {
        return (StatusCode::BAD_REQUEST, "port must be 1024 or above").into_response();
    }
    let result = match body.token {
        Some(token) => rotate(app, &token, &http_key, body.port).await.map(|port| (None, port)),
        None => {
            let port = body.port.unwrap_or(REGISTERED_LOCAL_PORT);
            create(app, &http_key, port).await.map(|token| (Some(token), port))
        }
    };
    match result {
        Ok((token, local_port)) => {
            app.resync().await;
            Json(json!({
                "server": format!("{host}:{}", app.control_port),
                "fingerprint": app.fingerprint,
                "token": token,
                "path": format!("/t/{secret}"),
                "local_port": local_port,
            }))
            .into_response()
        }
        Err(resp) => resp,
    }
}

/// Returns the local port the client's tunnel forwards to.
async fn rotate(app: &crate::app::App, token: &str, http_key: &str, port: Option<u16>) -> Result<u16, Response> {
    let filter = doc! { "token_hash": proto::sha256_hex(token.as_bytes()) };
    let client = app.store.clients().find_one(filter).await.map_err(internal)?;
    let client = client.ok_or_else(|| (StatusCode::UNAUTHORIZED, "unknown token").into_response())?;
    let filter = doc! { "client_id": client.id, "protocol": "http" };
    let mut set = doc! { "http_key": http_key };
    if let Some(port) = port {
        set.insert("local_addr", format!("127.0.0.1:{port}"));
    }
    app.store.services().update_many(filter.clone(), doc! { "$set": set }).await.map_err(internal)?;
    app.store.log("client.url_rotated", &client.name, None, None).await;
    let service = app.store.services().find_one(filter).await.map_err(internal)?;
    let current = service.and_then(|s| s.local_addr.rsplit(':').next()?.parse().ok());
    Ok(port.or(current).unwrap_or(REGISTERED_LOCAL_PORT))
}

async fn create(app: &crate::app::App, http_key: &str, port: u16) -> Result<String, Response> {
    static BUDGET: Mutex<Option<(Instant, u32)>> = Mutex::new(None);
    {
        let mut b = BUDGET.lock().unwrap();
        let (since, used) = b.get_or_insert((Instant::now(), 0));
        if since.elapsed() > Duration::from_secs(3600) {
            (*since, *used) = (Instant::now(), 0);
        }
        if *used >= REGISTRATIONS_PER_HOUR {
            return Err((StatusCode::TOO_MANY_REQUESTS, "too many registrations, retry later").into_response());
        }
        *used += 1;
    }
    let token = auth::random_token(24);
    let name = format!("auto-{}", auth::random_token(4));
    let client = ClientDoc {
        id: ObjectId::new(),
        name: name.clone(),
        token_hash: proto::sha256_hex(token.as_bytes()),
        enabled: true,
        created_at: DateTime::now(),
        last_seen_at: None,
    };
    let service = ServiceDoc {
        id: ObjectId::new(),
        name: name.clone(),
        client_id: client.id,
        protocol: proto::Protocol::Http,
        bind_port: 0,
        local_addr: format!("127.0.0.1:{port}"),
        enabled: true,
        created_at: DateTime::now(),
        bytes_in: 0,
        bytes_out: 0,
        http_key: Some(http_key.into()),
    };
    app.store.clients().insert_one(&client).await.map_err(internal)?;
    app.store.services().insert_one(&service).await.map_err(internal)?;
    app.store.log("client.registered", &name, None, None).await;
    Ok(token)
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::warn!("registration failed: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn is_https(headers: &HeaderMap) -> bool {
    headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

async fn graphql(State(st): State<AppState>, headers: HeaderMap, req: GraphQLRequest) -> Response {
    // Only JSON bodies: blocks cross-site form posts (CSRF) on top of SameSite cookies.
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !json {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "application/json required").into_response();
    }
    let user = auth::user_from_headers(&st.app.store, &headers).await;
    let req = req
        .into_inner()
        .data(user)
        .data(RequestMeta { https: is_https(&headers) })
        .data(SessionToken(auth::token_from_headers(&headers)));
    GraphQLResponse::from(st.schema.execute(req).await).into_response()
}

/// Rejects WebSocket handshakes from other origins (cookies would otherwise be sent).
fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    match (host, origin) {
        (Some(host), Some(origin)) => origin.split_once("://").is_some_and(|(_, rest)| rest == host),
        (_, None) => true, // non-browser client
        _ => false,
    }
}

async fn graphql_ws(
    State(st): State<AppState>,
    headers: HeaderMap,
    protocol: GraphQLProtocol,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let user = auth::user_from_headers(&st.app.store, &headers).await;
    let schema = st.schema.clone();
    ws.protocols(async_graphql::http::ALL_WEBSOCKET_PROTOCOLS)
        .on_upgrade(move |socket| {
            let mut data = Data::default();
            data.insert(user);
            GraphQLWebSocket::new(socket, schema, protocol).with_data(data).serve()
        })
}
