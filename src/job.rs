use std::str::FromStr;

use anyhow::{anyhow, Context};
use rust_decimal::Decimal;
use tokio_cron_scheduler::{Job, JobSchedulerError};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    common::money::{Currency, Money},
    domain::finance::Outbox,
    event_sourcing::command::LedgerCommand,
    repository::redis::{acquire_lock, release_lock, LOCK_KEY, LOCK_TIMEOUT},
    state::LedgerLoaderSaver,
    SharedState,
};

const MAX_RETRIES: i32 = 5;

pub async fn create_ledger_job(state: SharedState) -> Result<Job, JobSchedulerError> {
    Job::new_async("1/10 * * * * *", move |_uuid, _l| {
        let db = state.database.clone();
        let ledger = state.ledger.clone();
        let cache = state.cache.clone();
        Box::pin(async move {
            let ledger = match ledger {
                Some(l) => l,
                None => {
                    error!("Ledger not configured, skipping outbox processing");
                    return;
                }
            };
            let cache = match cache {
                Some(c) => c,
                None => {
                    error!("Cache not configured, skipping outbox processing");
                    return;
                }
            };

            match db.get_unprocessed_outbox().await {
                Ok(events) => {
                    if events.is_empty() {
                        return;
                    }

                    // Acquire lock — skip cycle if unavailable
                    let identifier = match acquire_lock(&cache, LOCK_KEY, LOCK_TIMEOUT).await {
                        Some(id) => id,
                        None => {
                            warn!("Could not acquire outbox lock, skipping cycle");
                            return;
                        }
                    };

                    for event in events {
                        info!("Processing outbox event id={}", event.id);
                        match process_event(&event, &ledger).await {
                            Ok(transaction_id) => {
                                if let Err(err) = db.complete_transaction(transaction_id).await {
                                    error!(
                                        "Error completing transaction {}: {:?}",
                                        transaction_id, err
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Error processing outbox event id={}: {:?}", event.id, e);
                                // Retry with backoff: increment retry count
                                if event.retry_count + 1 >= MAX_RETRIES {
                                    warn!("Outbox event id={} exceeded max retries, moving to dead letter", event.id);
                                    if let Err(dl_err) = db
                                        .move_to_dead_letter(
                                            event.id,
                                            event.transaction_id,
                                            &event.event_type,
                                            &event.payload,
                                            &e.to_string(),
                                            event.retry_count + 1,
                                        )
                                        .await
                                    {
                                        error!("Failed to move to dead letter: {:?}", dl_err);
                                    }
                                } else if let Err(retry_err) =
                                    db.increment_outbox_retry(event.id, &e.to_string()).await
                                {
                                    error!("Failed to increment retry count: {:?}", retry_err);
                                }
                            }
                        }
                    }

                    release_lock(&cache, LOCK_KEY, &identifier).await;
                }
                Err(e) => {
                    error!("Error fetching outbox events: {:?}", e);
                }
            }
        })
    })
}

async fn process_event(event: &Outbox, ledger: &LedgerLoaderSaver) -> Result<Uuid, anyhow::Error> {
    // C5 FIX: Replace panic! with error return for unknown event types
    let key = match event.event_type.as_str() {
        "LedgerCommand::Credit" => "Credit",
        "LedgerCommand::Debit" => "DebitRelease",
        _ => return Err(anyhow!("Unknown event type: {}", event.event_type)),
    };
    let payload = &event.payload;

    info!("payload: {}", payload[key]);
    let id_str = payload[key]["id"].as_str().context("Missing 'id' field")?;
    let id = Uuid::parse_str(id_str).context("Invalid 'id' format")?;

    let account_id_str = payload[key]["account_id"]
        .as_str()
        .context("Missing 'account_id' field")?;
    let account_id = Uuid::parse_str(account_id_str).context("Invalid 'account_id' format")?;

    let transaction_id_str = payload[key]["transaction_id"]
        .as_str()
        .context("Missing 'transaction_id' field")?;
    let transaction_id =
        Uuid::parse_str(transaction_id_str).context("Invalid 'transaction_id' format")?;

    let amount_str = payload[key]["amount"]["amount"]
        .as_str()
        .context("Missing 'amount' field")?;
    let amount = Decimal::from_str(amount_str).context("Invalid 'amount' format")?;

    let currency_str = payload[key]["amount"]["currency"]
        .as_str()
        .context("Missing 'currency' field")?;
    let currency = Currency::from(currency_str.to_string());

    let amount = Money::new(amount, currency);

    // C5 FIX: No panic on unknown event type
    let command = match event.event_type.as_str() {
        "LedgerCommand::Credit" => LedgerCommand::Credit {
            id,
            account_id,
            transaction_id,
            amount,
        },
        "LedgerCommand::Debit" => LedgerCommand::DebitRelease {
            id,
            account_id,
            transaction_id,
            amount,
        },
        _ => return Err(anyhow!("Unknown event type: {}", event.event_type)),
    };

    ledger
        .cqrs
        .execute(id_str, command)
        .await
        .map_err(|e| anyhow!("Failed to write ledger: {}", e))?;

    Ok(transaction_id)
}
