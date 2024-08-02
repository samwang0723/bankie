use axum::routing::get;
use axum::Router;
use log::info;
use route::{bank_account_command_handler, bank_account_query_handler, ledger_query_handler};
use state::new_application_state;
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;

mod command;
mod common;
mod configs;
mod domain;
mod event_sourcing;
mod repository;
mod route;
mod service;
mod state;

#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv::dotenv().ok();
    let state = new_application_state().await;
    // Configure the Axum routes and services.
    // For this example a single logical endpoint is used and the HTTP method
    // distinguishes whether the call is a command or a query.
    let comression_layer: CompressionLayer = CompressionLayer::new();
    let router = Router::new()
        .route(
            "/bank_account/:id",
            get(bank_account_query_handler).post(bank_account_command_handler),
        )
        .route("/ledger/:id", get(ledger_query_handler))
        .layer(comression_layer)
        .with_state(state);
    // Start the Axum server.
    let listener = TcpListener::bind("0.0.0.0:3030").await.unwrap();
    info!("Server running on: {}", listener.local_addr().unwrap());
    axum::serve(listener, router.into_make_service())
        .await
        .unwrap();
}
