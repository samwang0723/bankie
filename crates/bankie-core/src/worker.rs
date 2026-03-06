use bankie_core::interest::accrual_job::create_accrual_job;
use bankie_core::interest::posting_job::create_interest_posting_job;
use bankie_core::job::{create_balance_snapshot_job, create_ledger_job};
use bankie_core::state::new_worker_state;
use tokio_cron_scheduler::JobScheduler;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("bankie-worker starting...");

    let state = new_worker_state().await;

    let mut sched = match JobScheduler::new().await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create job scheduler: {:?}", e);
            return;
        }
    };

    // Register all 4 cron jobs
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
    match create_accrual_job(state.clone()).await {
        Ok(job) => {
            if let Err(e) = sched.add(job).await {
                error!("Failed to add interest accrual job: {:?}", e);
            }
        }
        Err(e) => {
            error!("Failed to create interest accrual job: {:?}", e);
        }
    }
    match create_interest_posting_job(state.clone()).await {
        Ok(job) => {
            if let Err(e) = sched.add(job).await {
                error!("Failed to add interest posting job: {:?}", e);
            }
        }
        Err(e) => {
            error!("Failed to create interest posting job: {:?}", e);
        }
    }

    if let Err(e) = sched.start().await {
        error!("Failed to start scheduler: {:?}", e);
        return;
    }

    info!("bankie-worker running — 4 jobs registered");

    // Graceful shutdown on SIGINT/SIGTERM
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install signal handler");
    info!("Received shutdown signal, shutting down worker...");

    if let Err(e) = sched.shutdown().await {
        error!("Error during scheduler shutdown: {:?}", e);
    }

    info!("bankie-worker stopped.");
}
