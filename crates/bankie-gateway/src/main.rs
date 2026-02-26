mod config;
mod middleware;
mod models;
mod proxy;
mod redis_ops;
mod repo;

use axum::{
    middleware as axum_middleware,
    routing::{any, get},
    Json, Router,
};
use serde_json::{json, Value};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::info;

use crate::config::SETTINGS;

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "service": "bankie-gateway"}))
}

async fn ready() -> Json<Value> {
    // TODO: check DB + Redis connectivity
    Json(json!({"status": "ok", "service": "bankie-gateway"}))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let settings = SETTINGS.clone();
    let addr = settings.listen_addr.clone();

    // Connect to database
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&settings.database.connection_string())
        .await
        .expect("Failed to connect to database");

    // Connect to Redis
    let redis_client = redis::Client::open(settings.redis.connection_string())
        .expect("Failed to create Redis client");

    // Build the proxied API routes with full middleware stack:
    //   api_key_resolver → rate_limiter → jwt_minter → proxy_handler
    let api_routes = Router::new()
        .fallback(any(proxy::proxy_handler))
        .layer(axum_middleware::from_fn(middleware::jwt_minter::jwt_minter))
        .layer(axum_middleware::from_fn(
            middleware::rate_limiter::rate_limiter,
        ))
        .layer(axum_middleware::from_fn(
            middleware::api_key_resolver::api_key_resolver,
        ))
        .layer(axum::extract::Extension(redis_client))
        .layer(axum::extract::Extension(pool))
        .with_state(settings.clone());

    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .merge(api_routes)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    info!("bankie-gateway listening on {}", addr);

    // Graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(());
    });

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
            info!("Gateway shutting down gracefully");
        })
        .await
        .unwrap();
}
