use std::str::FromStr;

use anyhow::{anyhow, Context};
use rust_decimal::Decimal;
use tokio_cron_scheduler::{Job, JobSchedulerError};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    common::money::{Currency, Money},
    domain::finance::Outbox,
    event_sourcing::command::LedgerCommand,
    repository::redis::{acquire_lock, release_lock, LOCK_KEY, LOCK_TIMEOUT},
    state::LedgerLoaderSaver,
    SharedState,
};

pub async fn create_ledger_job(state: SharedState) -> Result<Job, JobSchedulerError> {
    Job::new_async("1/10 * * * * *", move |_uuid, _l| {
        let db = state.database.clone();
        let ledger = state.ledger.clone().unwrap();
        let cache = state.cache.clone().unwrap();
        Box::pin(async move {
            match db.get_unprocessed_outbox().await {
                Ok(events) => {
                    // acquire lock
                    let identifier = acquire_lock(&cache, LOCK_KEY, LOCK_TIMEOUT).await.unwrap();

                    // process events
                    for event in events {
                        info!("Processing event: {:?}", event);
                        match process_event(event, &ledger).await {
                            Ok(transaction_id) => {
                                // mark outbox processed and complete transaction
                                if let Err(err) = db.complete_transaction(transaction_id).await {
                                    error!("Error completing transaction: {:?}", err);
                                }
                            }
                            Err(e) => {
                                error!("Error processing event: {:?}", e);
                            }
                        }
                    }

                    // release lock
                    release_lock(&cache, LOCK_KEY, &identifier).await;
                }
                Err(e) => {
                    error!("Error fetching events: {:?}", e);
                }
            }
        })
    })
}

async fn process_event(event: Outbox, ledger: &LedgerLoaderSaver) -> Result<Uuid, anyhow::Error> {
    let key = match event.event_type.as_str() {
        "LedgerCommand::Credit" => "Credit",
        "LedgerCommand::Debit" => "DebitRelease",
        _ => panic!("Unknown event type: {}", event.event_type),
    };
    let payload = event.payload;

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
        _ => panic!("Unknown event type: {}", event.event_type),
    };

    // Note ledger changes and update balance
    ledger
        .cqrs
        .execute(id_str, command)
        .await
        .map_err(|e| anyhow!("Failed to write ledger: {}", e))?;

    Ok(transaction_id)
}
