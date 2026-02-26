use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use tracing::info;

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "service": "bankie-gateway"}))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new().route("/health", get(health));

    let addr = "0.0.0.0:4040";
    info!("bankie-gateway listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
