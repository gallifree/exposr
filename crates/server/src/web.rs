//! HTTP layer: GraphQL over POST + WebSocket, health check, static admin UI.

use crate::app::App;
use crate::auth;
use crate::graphql::{AppSchema, RequestMeta, SessionToken};
use async_graphql::Data;
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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
        .fallback_service(files)
        .with_state(AppState { app, schema })
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
