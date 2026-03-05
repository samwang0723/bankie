use std::sync::Arc;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use tokio_cron_scheduler::{Job, JobSchedulerError};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    repository::{
        adapter::Adapter,
        redis::{acquire_lock, release_lock, LOCK_TIMEOUT},
    },
    SharedState,
};

use super::{calculator::calculate_daily_interest, models::InterestAccrual};

const ACCRUAL_LOCK_KEY: &str = "interest_accrual_lock";

/// Daily interest accrual job — runs at 00:05 UTC.
/// For each Interest-kind account with a fiat currency (USD, TWD),
/// calculates daily interest using the active rate config + tiers,
/// and inserts an accrual record (idempotent via ON CONFLICT DO NOTHING).
pub async fn create_accrual_job(state: SharedState) -> Result<Job, JobSchedulerError> {
    // Cron: "0 5 0 * * *" = every day at 00:05:00 UTC
    Job::new_async("0 5 0 * * *", move |_uuid, _l| {
        let db = state.database.clone();
        let cache = state.cache.clone();
        Box::pin(async move {
            let cache = match cache {
                Some(c) => c,
                None => {
                    error!("Cache not configured, skipping interest accrual");
                    return;
                }
            };

            let identifier = match acquire_lock(&cache, ACCRUAL_LOCK_KEY, LOCK_TIMEOUT).await {
                Some(id) => id,
                None => {
                    warn!("Could not acquire accrual lock, skipping cycle");
                    return;
                }
            };

            let accrual_date = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
            info!("Starting daily interest accrual for {}", accrual_date);

            if let Err(e) = run_accrual(&db, accrual_date).await {
                error!("Interest accrual failed: {:?}", e);
            }

            release_lock(&cache, ACCRUAL_LOCK_KEY, &identifier).await;
        })
    })
}

/// Core accrual logic, separated for testability.
pub async fn run_accrual<C: crate::repository::adapter::DatabaseClient + Send + Sync>(
    db: &Arc<Adapter<C>>,
    accrual_date: NaiveDate,
) -> Result<(), anyhow::Error> {
    let accounts = db.get_interest_eligible_accounts().await?;
    if accounts.is_empty() {
        info!("No eligible accounts for interest accrual");
        return Ok(());
    }

    let mut success_count = 0u64;
    let mut skip_count = 0u64;
    let mut error_count = 0u64;

    for account in &accounts {
        let account_id = match &account.id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => {
                skip_count += 1;
                continue;
            }
        };
        let ledger_id = match &account.ledger_id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => {
                skip_count += 1;
                continue;
            }
        };
        let currency = match &account.currency {
            Some(c) if !c.is_empty() => c.clone(),
            _ => {
                skip_count += 1;
                continue;
            }
        };
        let tenant_id = account.tenant_id.unwrap_or(0);
        let balance = account.available.unwrap_or(Decimal::ZERO);

        if balance <= Decimal::ZERO {
            skip_count += 1;
            continue;
        }

        // Look up active rate config for this currency
        let config = match db
            .get_active_rate_config(currency.clone(), accrual_date)
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => {
                // No rate config for this currency — skip silently
                skip_count += 1;
                continue;
            }
            Err(e) => {
                error!(
                    "Failed to get rate config for {} (account {}): {:?}",
                    currency, account_id, e
                );
                error_count += 1;
                continue;
            }
        };

        let day_count = match config.day_count_convention() {
            Ok(dc) => dc,
            Err(e) => {
                error!("Invalid day_count for config {}: {}", config.id, e);
                error_count += 1;
                continue;
            }
        };

        let tiers = match db.get_rate_tiers(config.id).await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to get rate tiers for config {}: {:?}", config.id, e);
                error_count += 1;
                continue;
            }
        };

        if tiers.is_empty() {
            skip_count += 1;
            continue;
        }

        let (daily_interest, breakdowns) = calculate_daily_interest(balance, &tiers, &day_count);

        if daily_interest == Decimal::ZERO {
            skip_count += 1;
            continue;
        }

        let tier_breakdown =
            serde_json::to_value(&breakdowns).unwrap_or(serde_json::Value::Array(vec![]));

        let accrual = InterestAccrual {
            id: Uuid::new_v4(),
            tenant_id,
            account_id: account_id.clone(),
            ledger_id,
            currency,
            accrual_date,
            balance_used: balance,
            daily_interest,
            rate_config_id: config.id,
            tier_breakdown,
        };

        match db.insert_interest_accrual(accrual).await {
            Ok(()) => success_count += 1,
            Err(e) => {
                error!(
                    "Failed to insert accrual for account {}: {:?}",
                    account_id, e
                );
                error_count += 1;
            }
        }
    }

    info!(
        "Interest accrual complete: {} succeeded, {} skipped, {} failed",
        success_count, skip_count, error_count
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::BankAccountWithLedger;
    use crate::interest::models::{InterestRateConfig, InterestRateTier};
    use crate::repository::adapter::{Adapter, MockDatabaseClient};
    use chrono::NaiveDate;
    use mockall::predicate::*;
    use rust_decimal_macros::dec;

    fn make_eligible_account(
        id: &str,
        ledger_id: &str,
        currency: &str,
        available: Decimal,
        tenant_id: i32,
    ) -> BankAccountWithLedger {
        BankAccountWithLedger {
            id: Some(id.to_string()),
            ledger_id: Some(ledger_id.to_string()),
            currency: Some(currency.to_string()),
            available: Some(available),
            tenant_id: Some(tenant_id),
            status: Some("Approved".to_string()),
            kind: Some("Interest".to_string()),
            ..Default::default()
        }
    }

    fn make_rate_config(id: Uuid, currency: &str) -> InterestRateConfig {
        InterestRateConfig {
            id,
            currency: currency.to_string(),
            account_kind: "Interest".to_string(),
            day_count: "Actual/365".to_string(),
            posting_frequency: "Monthly".to_string(),
            posting_day: Some(1),
            effective_from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            effective_to: None,
            is_active: true,
        }
    }

    fn make_tier(
        config_id: Uuid,
        order: i16,
        min: Decimal,
        max: Option<Decimal>,
        apr: Decimal,
    ) -> InterestRateTier {
        InterestRateTier {
            id: Uuid::new_v4(),
            rate_config_id: config_id,
            tier_order: order,
            min_balance: min,
            max_balance: max,
            apr,
        }
    }

    #[tokio::test]
    async fn test_accrual_no_eligible_accounts() {
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| Ok(vec![]));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_zero_balance() {
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(0),
                    1,
                )])
            });

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_missing_id() {
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![BankAccountWithLedger {
                    id: None,
                    available: Some(dec!(1000)),
                    ..Default::default()
                }])
            });

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_no_rate_config() {
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(10000),
                    1,
                )])
            });
        mock.expect_get_active_rate_config()
            .times(1)
            .returning(|_, _| Ok(None));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_empty_tiers() {
        let config_id = Uuid::new_v4();
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(10000),
                    1,
                )])
            });
        mock.expect_get_active_rate_config()
            .times(1)
            .returning(move |_, _| Ok(Some(make_rate_config(config_id, "USD"))));
        mock.expect_get_rate_tiers()
            .times(1)
            .returning(|_| Ok(vec![]));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_inserts_record_successfully() {
        let config_id = Uuid::new_v4();
        let mut mock = MockDatabaseClient::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(10000),
                    1,
                )])
            });

        let config_id_clone = config_id;
        mock.expect_get_active_rate_config()
            .times(1)
            .returning(move |_, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_insert_interest_accrual()
            .times(1)
            .withf(|accrual| {
                accrual.account_id == "acc-1"
                    && accrual.currency == "USD"
                    && accrual.balance_used == dec!(10000)
                    && accrual.daily_interest > Decimal::ZERO
            })
            .returning(|_| Ok(()));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_handles_insert_error_gracefully() {
        let config_id = Uuid::new_v4();
        let mut mock = MockDatabaseClient::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(10000),
                    1,
                )])
            });

        let config_id_clone = config_id;
        mock.expect_get_active_rate_config()
            .times(1)
            .returning(move |_, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_insert_interest_accrual()
            .times(1)
            .returning(|_| Err(sqlx::Error::Protocol("test error".to_string())));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        // Should not propagate the insert error — it logs and continues
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_multiple_accounts_mixed_results() {
        let config_id = Uuid::new_v4();
        let mut mock = MockDatabaseClient::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![
                    make_eligible_account("acc-1", "led-1", "USD", dec!(10000), 1),
                    make_eligible_account("acc-2", "led-2", "USD", dec!(0), 1), // zero balance
                    make_eligible_account("acc-3", "led-3", "TWD", dec!(50000), 2),
                ])
            });

        let config_id_clone = config_id;
        // Called for acc-1 (USD) and acc-3 (TWD)
        mock.expect_get_active_rate_config()
            .times(2)
            .returning(move |currency, _| Ok(Some(make_rate_config(config_id_clone, &currency))));

        mock.expect_get_rate_tiers()
            .times(2)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_insert_interest_accrual()
            .times(2)
            .returning(|_| Ok(()));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_negative_balance() {
        let mut mock = MockDatabaseClient::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(-500),
                    1,
                )])
            });

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_correct_daily_interest_amount() {
        let config_id = Uuid::new_v4();
        let mut mock = MockDatabaseClient::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|| {
                Ok(vec![make_eligible_account(
                    "acc-1",
                    "led-1",
                    "USD",
                    dec!(10000),
                    1,
                )])
            });

        let config_id_clone = config_id;
        mock.expect_get_active_rate_config()
            .times(1)
            .returning(move |_, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        // Verify the exact daily interest: 10000 * 0.045 / 365
        let expected_daily = dec!(10000) * dec!(0.045) / dec!(365);
        mock.expect_insert_interest_accrual()
            .times(1)
            .withf(move |accrual| accrual.daily_interest == expected_daily)
            .returning(|_| Ok(()));

        let db = Arc::new(Adapter::new(mock));
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&db, date).await;
        assert!(result.is_ok());
    }
}
