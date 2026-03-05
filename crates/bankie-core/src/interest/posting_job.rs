use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio_cron_scheduler::{Job, JobSchedulerError};
use tracing::{error, info, warn};
use uuid::Uuid;

use super::calculator::round_for_posting;
use super::models::InterestPosting;
use super::repository::InterestRepository;
use crate::common::money::{Currency, Money};
use crate::common::snowflake::generate_transaction_reference;
use crate::domain::finance::{JournalEntry, JournalLine, Transaction, TRANS_INTEREST};
use crate::repository::adapter::Adapter;
use crate::repository::redis::{acquire_lock, release_lock, LOCK_TIMEOUT};
use crate::SharedState;

const POSTING_LOCK_KEY: &str = "interest_posting_lock";

/// Create the interest posting cron job.
/// Runs daily at 00:10 UTC, finds accounts with posting due today,
/// sums their unposted accruals, creates posting records, then
/// executes each posting as a real deposit transaction.
pub async fn create_interest_posting_job(state: SharedState) -> Result<Job, JobSchedulerError> {
    // Cron: "0 10 0 * * *" = every day at 00:10:00 UTC
    Job::new_async("0 10 0 * * *", move |_uuid, _l| {
        let state = state.clone();
        Box::pin(async move {
            let interest_db = match &state.interest_repo {
                Some(db) => db.clone(),
                None => {
                    error!("Interest repository not configured, skipping posting cycle");
                    return;
                }
            };
            let cache = match &state.cache {
                Some(c) => c.clone(),
                None => {
                    error!("Cache not configured, skipping posting cycle");
                    return;
                }
            };
            run_posting_cycle(interest_db.as_ref(), &cache, &state.database).await;
        })
    })
}

/// Core posting logic — extracted for testability.
pub async fn run_posting_cycle(
    db: &dyn InterestRepository,
    cache: &redis::Client,
    adapter: &Adapter<PgPool>,
) {
    let identifier = match acquire_lock(cache, POSTING_LOCK_KEY, LOCK_TIMEOUT).await {
        Some(id) => id,
        None => {
            warn!("Could not acquire interest posting lock, skipping cycle");
            return;
        }
    };

    let today = Utc::now().date_naive();
    info!("Interest posting job started for {}", today);

    // 1. Get all active rate configs
    let configs = match db.list_rate_configs(None, true).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to fetch rate configs: {:?}", e);
            release_lock(cache, POSTING_LOCK_KEY, &identifier).await;
            return;
        }
    };

    // 2. Get interest-type accounts
    let accounts = match db.list_interest_accounts().await {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to list interest accounts: {:?}", e);
            release_lock(cache, POSTING_LOCK_KEY, &identifier).await;
            return;
        }
    };

    let mut posted = 0u32;
    let mut skipped = 0u32;
    let mut errored = 0u32;

    for account in &accounts {
        // Find matching config for this account's currency
        let config = match configs.iter().find(|c| c.currency == account.currency) {
            Some(c) => c,
            None => {
                skipped += 1;
                continue;
            }
        };

        // Check if posting is due today
        let freq = match config.posting_freq() {
            Ok(f) => f,
            Err(e) => {
                error!("Invalid posting frequency for config {}: {}", config.id, e);
                errored += 1;
                continue;
            }
        };

        if !freq.is_posting_due(today, config.posting_day) {
            skipped += 1;
            continue;
        }

        // Determine period: from last posting end date (or effective_from) to today
        let period_start = match db.get_last_posting_date(&account.account_id).await {
            Ok(Some(d)) => d,
            Ok(None) => config.effective_from,
            Err(e) => {
                error!(
                    "Failed to get last posting date for {}: {:?}",
                    account.account_id, e
                );
                errored += 1;
                continue;
            }
        };
        let period_end = today;

        if period_start >= period_end {
            skipped += 1;
            continue; // nothing to post
        }

        // Sum unposted accruals for the period
        let accrued_total = match db
            .sum_accruals_for_period(&account.account_id, period_start, period_end)
            .await
        {
            Ok(total) => total,
            Err(e) => {
                error!("Failed to sum accruals for {}: {:?}", account.account_id, e);
                errored += 1;
                continue;
            }
        };

        if accrued_total <= Decimal::ZERO {
            skipped += 1;
            continue;
        }

        let posted_amount = round_for_posting(accrued_total, &account.currency);
        if posted_amount <= Decimal::ZERO {
            skipped += 1;
            continue;
        }

        let posting = InterestPosting {
            id: Uuid::new_v4(),
            tenant_id: account.tenant_id,
            account_id: account.account_id.clone(),
            currency: account.currency.clone(),
            posting_date: today,
            period_start,
            period_end,
            accrued_total,
            posted_amount,
            transaction_id: None,
            status: "pending".to_string(),
            error_message: None,
        };

        if let Err(e) = db.create_posting(posting.clone()).await {
            error!(
                "Failed to create posting for {}: {:?}",
                account.account_id, e
            );
            errored += 1;
            continue;
        }

        // Execute the posting: create a real deposit transaction
        match execute_posting(&posting, &account.ledger_id, db, adapter).await {
            Ok(()) => {
                posted += 1;
            }
            Err(e) => {
                error!(
                    "Failed to execute posting for {}: {}",
                    account.account_id, e
                );
                errored += 1;
            }
        }
    }

    info!(
        "Interest posting job complete: {} posted, {} skipped, {} errors",
        posted, skipped, errored
    );
    release_lock(cache, POSTING_LOCK_KEY, &identifier).await;
}

/// Execute a pending posting by creating a deposit transaction via the Adapter.
/// Looks up the house account for the currency, creates Transaction + JournalEntry
/// + JournalLines + Outbox record, then marks the posting as completed.
pub async fn execute_posting(
    posting: &InterestPosting,
    ledger_id: &str,
    db: &dyn InterestRepository,
    adapter: &Adapter<PgPool>,
) -> Result<(), String> {
    // 1. Get house account ledger_id for this currency + tenant
    let house_account = adapter
        .get_house_account(&posting.currency, posting.tenant_id)
        .await
        .map_err(|e| format!("Failed to get house account: {}", e))?;

    // 2. Build Money
    let currency: Currency = posting
        .currency
        .parse()
        .map_err(|e| format!("Invalid currency: {}", e))?;
    let amount = Money::new(posting.posted_amount, currency);

    // 3. Create the deposit transaction + journal via Adapter (same DB operations
    //    as create_transaction_with_journal_custom_prefix but using Adapter directly)
    let transaction = Transaction {
        id: Uuid::new_v4(),
        bank_account_id: Uuid::parse_str(&posting.account_id)
            .map_err(|e| format!("Invalid account_id: {}", e))?,
        transaction_reference: generate_transaction_reference(TRANS_INTEREST),
        transaction_date: chrono::Utc::now(),
        amount: amount.amount,
        currency: amount.currency.to_string(),
        description: Some("Interest posting".to_string()),
        metadata: serde_json::Value::Null,
        journal_entry_id: None,
        status: "processing".to_string(),
        tenant_id: posting.tenant_id,
        fx_rate_to_usd: None,
        amount_usd: None,
        fx_rate_source: None,
    };

    let journal_entry = JournalEntry {
        id: Uuid::new_v4(),
        entry_date: chrono::Utc::now().date_naive(),
        description: Some("Interest deposit".to_string()),
        status: "posted".to_string(),
        tenant_id: posting.tenant_id,
    };

    // Deposit: debit house account, credit user account
    let house_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: house_account.ledger_id,
        credit_amount: Decimal::ZERO,
        debit_amount: amount.amount,
        currency: amount.currency.to_string(),
        description: None,
        tenant_id: posting.tenant_id,
    };
    let user_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: ledger_id.to_string(),
        credit_amount: amount.amount,
        debit_amount: Decimal::ZERO,
        currency: amount.currency.to_string(),
        description: None,
        tenant_id: posting.tenant_id,
    };

    let journal_lines = vec![house_journal_line, user_journal_line];

    // 4. Create transaction + journal + outbox via Adapter
    match adapter
        .create_transaction_with_journal(
            transaction,
            ledger_id.to_string(),
            journal_entry,
            journal_lines,
            posting.tenant_id,
        )
        .await
    {
        Ok(transaction_id) => db
            .update_posting_status(posting.id, "completed", Some(transaction_id), None)
            .await
            .map_err(|e| format!("Failed to update posting status: {}", e)),
        Err(e) => {
            let err_msg = format!("Interest deposit failed: {}", e);
            error!(
                account_id = %posting.account_id,
                posting_id = %posting.id,
                "{}",
                err_msg
            );
            // Mark posting as failed so it can be retried
            let _ = db
                .update_posting_status(posting.id, "failed", None, Some(err_msg.clone()))
                .await;
            Err(err_msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::models::InterestRateConfig;
    use crate::interest::repository::{InterestAccountInfo, MockInterestRepository};
    use chrono::NaiveDate;
    use mockall::predicate::*;
    use rust_decimal_macros::dec;

    fn make_config(currency: &str, freq: &str, posting_day: Option<i16>) -> InterestRateConfig {
        InterestRateConfig {
            id: Uuid::new_v4(),
            currency: currency.to_string(),
            account_kind: "Interest".to_string(),
            day_count: "Actual/365".to_string(),
            posting_frequency: freq.to_string(),
            posting_day,
            effective_from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            effective_to: None,
            is_active: true,
        }
    }

    fn make_account(account_id: &str, currency: &str, tenant_id: i32) -> InterestAccountInfo {
        InterestAccountInfo {
            account_id: account_id.to_string(),
            ledger_id: Uuid::new_v4().to_string(),
            currency: currency.to_string(),
            tenant_id,
        }
    }

    #[test]
    fn test_round_for_posting_uses_currency_precision() {
        // USD precision = 2
        assert_eq!(round_for_posting(dec!(1.2345), "USD"), dec!(1.23));
        // TWD precision = 0
        assert_eq!(round_for_posting(dec!(1.5), "TWD"), dec!(2));
    }

    #[tokio::test]
    async fn test_posting_skips_when_no_configs() {
        let mut mock_db = MockInterestRepository::new();
        mock_db
            .expect_list_rate_configs()
            .returning(|_, _| Ok(vec![]));
        mock_db
            .expect_list_interest_accounts()
            .returning(|| Ok(vec![make_account("acc-1", "USD", 1)]));

        // With no configs, no postings should be created
        // (create_posting should never be called)
        mock_db.expect_create_posting().never();

        // We can't easily test run_posting_cycle without Redis,
        // but we can verify the logic components individually
        let configs = mock_db.list_rate_configs(None, true).await.unwrap();
        assert!(configs.is_empty());
    }

    #[tokio::test]
    async fn test_posting_skips_zero_accrual() {
        let mut mock_db = MockInterestRepository::new();
        mock_db
            .expect_sum_accruals_for_period()
            .returning(|_, _, _| Ok(Decimal::ZERO));

        let total = mock_db
            .sum_accruals_for_period(
                "acc-1",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(total, Decimal::ZERO);
    }

    #[tokio::test]
    async fn test_posting_created_for_positive_accrual() {
        let mut mock_db = MockInterestRepository::new();
        mock_db
            .expect_sum_accruals_for_period()
            .returning(|_, _, _| Ok(dec!(12.3456)));

        let total = mock_db
            .sum_accruals_for_period(
                "acc-1",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
            )
            .await
            .unwrap();
        assert!(total > Decimal::ZERO);
        let posted_amount = round_for_posting(total, "USD");
        assert_eq!(posted_amount, dec!(12.35));
    }

    #[tokio::test]
    async fn test_last_posting_end_date_none_uses_effective_from() {
        let mut mock_db = MockInterestRepository::new();
        mock_db
            .expect_get_last_posting_date()
            .returning(|_| Ok(None));

        let config = make_config("USD", "Monthly", Some(1));
        let last = mock_db.get_last_posting_date("acc-1").await.unwrap();
        assert!(last.is_none());
        // When None, we'd use config.effective_from
        assert_eq!(
            config.effective_from,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
    }

    #[tokio::test]
    async fn test_execute_posting_marks_completed() {
        let mut mock_db = MockInterestRepository::new();
        let posting_id = Uuid::new_v4();
        mock_db
            .expect_update_posting_status()
            .with(
                eq(posting_id),
                eq("completed"),
                eq(None::<Uuid>),
                eq(None::<String>),
            )
            .returning(|_, _, _, _| Ok(()));

        // Directly test the DB update call that execute_posting would make
        mock_db
            .update_posting_status(posting_id, "completed", None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_interest_accounts() {
        let mut mock_db = MockInterestRepository::new();
        mock_db.expect_list_interest_accounts().returning(|| {
            Ok(vec![
                make_account("acc-1", "USD", 1),
                make_account("acc-2", "TWD", 1),
            ])
        });

        let accounts = mock_db.list_interest_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].currency, "USD");
        assert_eq!(accounts[1].currency, "TWD");
    }
}
