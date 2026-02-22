use auth::jwt::{generate_jwt, generate_secret_key};
use auth::middleware::authorize;
use axum::Router;
use axum::{middleware, routing::get, routing::post};
use clap::Parser;
use clap_derive::Parser;
use common::idempotency::idempotency_check;
use event_sourcing::command::BankAccountCommand;
use job::{create_balance_snapshot_job, create_ledger_job};
use route::{
    balance_history_handler, bank_account_by_number_handler, bank_account_command_handler,
    bank_account_query_handler, health_check_handler, house_account_create_handler,
    house_account_query_handler, ledger_query_handler, readiness_check_handler,
    sub_account_query_handler, transaction_query_handler, user_query_handler,
};
use sqlx::PgPool;
use state::{new_application_state, ApplicationState};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task;
use tokio_cron_scheduler::JobScheduler;
use tower_http::add_extension::AddExtensionLayer;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

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

// Wrap ApplicationState in Arc for thread-safe sharing
type SharedState = Arc<ApplicationState<PgPool>>;

/// Command channel capacity — bounded to prevent OOM under load (H1 fix).
const COMMAND_CHANNEL_CAPACITY: usize = 10_000;

async fn process_commands(state: SharedState, mut rx: mpsc::Receiver<BankAccountCommand>) {
    while let Some(command) = rx.recv().await {
        info!("Processing command: {:?}", command);
        let id = match &command {
            BankAccountCommand::OpenAccount { id, .. } => id,
            BankAccountCommand::ApproveAccount { id, .. } => id,
            BankAccountCommand::FreezeAccount { id } => id,
            BankAccountCommand::UnfreezeAccount { id } => id,
            BankAccountCommand::CloseAccount { id } => id,
            BankAccountCommand::Deposit { id, .. } => id,
            BankAccountCommand::Withdrawal { id, .. } => id,
            BankAccountCommand::Transfer { id, .. } => id,
        }
        .to_string();
        if let Some(bank_account) = &state.bank_account {
            match bank_account.cqrs.execute(&id, command).await {
                Ok(_) => {
                    info!("Command processed successfully: {}", id);
                }
                Err(e) => {
                    error!("Error processing command: {:?}", e);
                }
            }
        }
    }
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
                    match generate_jwt(s.as_str(), &secret_key).await {
                        Ok(jwt) => info!("Generated: {}", jwt),
                        Err(e) => error!("Failed to generate JWT: {:?}", e),
                    }
                }
            }
        }
        "server" => {
            // H1 FIX: Bounded channel with backpressure
            let (tx, rx) = mpsc::channel::<BankAccountCommand>(COMMAND_CHANNEL_CAPACITY);
            let state = new_application_state(tx).await;

            // Spawn command processor
            let command_state = state.clone();
            task::spawn(async move {
                process_commands(command_state, rx).await;
            });

            // Add cron job for update ledger from outbox events
            let sched = match JobScheduler::new().await {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create job scheduler: {:?}", e);
                    return;
                }
            };
            match create_ledger_job(state.clone()).await {
                Ok(job) => {
                    if let Err(e) = sched.add(job).await {
                        error!("Failed to add ledger job: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to create ledger job: {:?}", e);
                }
            }
            // Daily balance snapshot job
            match create_balance_snapshot_job(state.clone()).await {
                Ok(job) => {
                    if let Err(e) = sched.add(job).await {
                        error!("Failed to add balance snapshot job: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to create balance snapshot job: {:?}", e);
                }
            }
            if let Err(e) = sched.start().await {
                error!("Failed to start scheduler: {:?}", e);
            }

            // Configure Axum routes
            let compression_layer: CompressionLayer = CompressionLayer::new();

            // Extract Redis client for idempotency middleware injection
            let redis_client = state
                .cache
                .clone()
                .expect("Redis client must be initialized at startup");

            let router = Router::new()
                // Health check endpoints (no auth required) — M3 fix
                .route("/health", get(health_check_handler))
                .route("/ready", get(readiness_check_handler))
                // Authenticated API endpoints
                .route("/v1/bank_account/:id", get(bank_account_query_handler))
                .route(
                    "/v1/bank_account/:id/sub-accounts",
                    get(sub_account_query_handler),
                )
                .route(
                    "/v1/bank_account/by-number/:account_number",
                    get(bank_account_by_number_handler),
                )
                .route("/v1/bank_account", post(bank_account_command_handler))
                .route("/v1/ledger/:id", get(ledger_query_handler))
                .route(
                    "/v1/house_account",
                    get(house_account_query_handler).post(house_account_create_handler),
                )
                .route("/v1/user/:id", get(user_query_handler))
                .route("/v1/transaction", get(transaction_query_handler))
                .route(
                    "/v1/bank_account/:id/balance-history",
                    get(balance_history_handler),
                )
                // Middleware layers (execution order: outermost → innermost → handler)
                .layer(middleware::from_fn(idempotency_check)) // Runs after auth, checks Redis for duplicate Idempotency-Key
                .layer(middleware::from_fn(authorize::<PgPool>)) // JWT auth + tenant extraction
                .layer(AddExtensionLayer::new(redis_client)) // Inject Redis for idempotency middleware
                .layer(AddExtensionLayer::new(state.clone())) // Inject ApplicationState
                .layer(compression_layer)
                .layer(TraceLayer::new_for_http())
                .with_state(state);

            // M7 FIX: Proper error handling instead of unwrap on TcpListener::bind
            let listener = match TcpListener::bind("0.0.0.0:3030").await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind to 0.0.0.0:3030: {:?}", e);
                    return;
                }
            };
            info!(
                "Server running on: {}",
                listener
                    .local_addr()
                    .unwrap_or_else(|_| "unknown".parse().unwrap())
            );

            // M2 FIX: Graceful shutdown
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                info!("Received shutdown signal, gracefully shutting down...");
                let _ = shutdown_tx.send(());
            });

            if let Err(e) = axum::serve(listener, router.into_make_service())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                error!("Server error: {:?}", e);
            }
        }
        _ => {
            warn!("Unknown mode: {}", args.mode);
        }
    }
}
