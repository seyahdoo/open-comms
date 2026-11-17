mod auth;
mod authz;
mod bson_util;
mod config;
mod db;
mod handlers;
mod hub;
mod media;
mod routes_auth;
mod ws;

use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config::Config;
use db::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::info;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    str0m::crypto::from_feature_flags().install_process_default();

    let config = Config::from_env()?;
    let media = media::start(&media::MediaConfig {
        bind_addr: config.media_bind_addr,
        advertise_addr: config.media_advertise_addr,
    })?;
    let state = db::init_state(&config, media).await?;

    let web_dist = PathBuf::from(&config.web_dist_dir);
    let index_file = web_dist.join("index.html");
    let static_files = ServeDir::new(web_dist).fallback(ServeFile::new(index_file));
    info!("serving web client from {}", config.web_dist_dir);

    let app = Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(routes_auth::register))
        .route("/auth/me", get(routes_auth::me))
        .route("/ws", get(ws_handler))
        .route("/channels", get(handlers::list_channels).post(handlers::create_channel))
        .route(
            "/channels/:channel_id/messages",
            get(handlers::list_messages).post(handlers::create_message),
        )
        .route(
            "/channels/:channel_id/messages/:parent_id/replies",
            post(handlers::create_reply),
        )
        .route(
            "/channels/:channel_id/messages/:message_id/reactions",
            post(handlers::add_reaction),
        )
        .fallback_service(static_files)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    info!("server listening on {}", config.bind_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "open-comms-server",
    })
}

#[derive(Debug, Deserialize, Default)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    let user = if let Some(token) = query.token.as_deref() {
        auth::user_for_token(&state, token).await.ok().flatten()
    } else {
        None
    };
    ws.on_upgrade(move |socket| ws::handle_socket(socket, state, user.map(|u| u.id)))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
