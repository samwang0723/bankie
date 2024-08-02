use axum::routing::get;
use axum::Router;
use route::{bank_account_command_handler, bank_account_query_handler, ledger_query_handler};
use state::new_application_state;

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
    dotenv::dotenv().ok();
    let state = new_application_state().await;
    // Configure the Axum routes and services.
    // For this example a single logical endpoint is used and the HTTP method
    // distinguishes whether the call is a command or a query.
    let router = Router::new()
        .route(
            "/bank_account/:id",
            get(bank_account_query_handler).post(bank_account_command_handler),
        )
        .route("/ledger/:id", get(ledger_query_handler))
        .with_state(state);
    // Start the Axum server.
    axum::Server::bind(&"0.0.0.0:3030".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}
