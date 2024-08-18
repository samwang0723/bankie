use auth::jwt::{generate_jwt, generate_secret_key};
use auth::middleware::authorize;
use axum::Router;
use axum::{middleware, routing::get, routing::post};
use clap::Parser;
use clap_derive::Parser;
use job::create_ledger_job;
use route::{
    bank_account_command_handler, bank_account_query_handler, house_account_create_handler,
    house_account_query_handler, ledger_query_handler,
};
use sqlx::PgPool;
use state::new_application_state;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_cron_scheduler::JobScheduler;
use tower_http::add_extension::AddExtensionLayer;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

mod auth;
mod command;
mod common;
mod configs;
mod domain;
mod event_sourcing;
mod house_account;
mod job;
mod repository;
mod route;
mod service;
mod state;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Mode to generate secret key or JWT
    #[arg(short, long)]
    mode: String,

    /// Service ID for JWT token
    #[arg(short, long)]
    service: Option<String>,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    match args.mode.as_str() {
        "secret_key" => {
            let secret_key = generate_secret_key(50);
            info!("Generated: {}", secret_key);
        }
        "jwt" => {
            if let Some(s) = args.service {
                if let Ok(secret_key) = std::env::var("JWT_SECRET") {
                    let jwt = generate_jwt(s.as_str(), &secret_key).await.unwrap();
                    info!("Generated: {}", jwt);
                }
            }
        }
        "server" => {
            let state = new_application_state().await;

            // Add cron job for update ledger from outbox events
            let sched = JobScheduler::new().await.unwrap();
            let job = create_ledger_job(state.clone()).await.unwrap();
            sched.add(job).await.unwrap();
            sched.start().await.unwrap();

            // Configure the Axum routes and services.
            // For this example a single logical endpoint is used and the HTTP method
            // distinguishes whether the call is a command or a query.
            let comression_layer: CompressionLayer = CompressionLayer::new();
            let router = Router::new()
                .route("/v1/bank_account/:id", get(bank_account_query_handler))
                .route("/v1/bank_account", post(bank_account_command_handler))
                .route("/v1/ledger/:id", get(ledger_query_handler))
                .route(
                    "/v1/house_account",
                    get(house_account_query_handler).post(house_account_create_handler),
                )
                .layer(middleware::from_fn(authorize::<PgPool>))
                .layer(AddExtensionLayer::new(Arc::new(state.clone())))
                .layer(comression_layer)
                .layer(TraceLayer::new_for_http())
                .with_state(state);
            // Start the Axum server.
            let listener = TcpListener::bind("0.0.0.0:3030").await.unwrap();
            info!("Server running on: {}", listener.local_addr().unwrap());
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        }
        _ => {}
    }
}
