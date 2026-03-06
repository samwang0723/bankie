use chrono::NaiveDate;
use rust_decimal::Decimal;
use tokio_cron_scheduler::{Job, JobSchedulerError};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::repository::redis::{acquire_lock, release_lock, LOCK_TIMEOUT};
use crate::SharedState;

use super::calculator::calculate_daily_interest;
use super::models::InterestAccrual;
use super::repository::InterestRepository;

const ACCRUAL_LOCK_KEY: &str = "interest_accrual_lock";

/// Daily interest accrual job — runs at 00:05 UTC.
/// For each Interest-kind account with a positive balance snapshot,
/// calculates daily interest using the active rate config + tiers,
/// and inserts an accrual record (idempotent via ON CONFLICT DO NOTHING).
pub async fn create_accrual_job(state: SharedState) -> Result<Job, JobSchedulerError> {
    // Cron: "0 5 0 * * *" = every day at 00:05:00 UTC
    Job::new_async("0 5 0 * * *", move |_uuid, _l| {
        let interest_repo = state.interest_repo.clone();
        let cache = state.cache.clone();
        Box::pin(async move {
            let cache = match cache {
                Some(c) => c,
                None => {
                    error!("Cache not configured, skipping interest accrual");
                    return;
                }
            };

            let repo = match interest_repo {
                Some(r) => r,
                None => {
                    error!("Interest repository not configured, skipping accrual");
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

            if let Err(e) = run_accrual(repo.as_ref(), accrual_date).await {
                error!("Interest accrual failed: {:?}", e);
            }

            release_lock(&cache, ACCRUAL_LOCK_KEY, &identifier).await;
        })
    })
}

/// Core accrual logic, separated for testability with mock `InterestRepository`.
pub async fn run_accrual(
    repo: &dyn InterestRepository,
    accrual_date: NaiveDate,
) -> Result<(), anyhow::Error> {
    let accounts = repo.get_interest_eligible_accounts(accrual_date).await?;
    if accounts.is_empty() {
        info!("No eligible accounts for interest accrual");
        return Ok(());
    }

    let mut success_count = 0u64;
    let mut skip_count = 0u64;
    let mut error_count = 0u64;

    for account in &accounts {
        // Look up active rate config for this currency + Interest kind
        let config = match repo
            .get_active_rate_config(&account.currency, "Interest", accrual_date)
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => {
                skip_count += 1;
                continue;
            }
            Err(e) => {
                error!(
                    "Failed to get rate config for {} (account {}): {:?}",
                    account.currency, account.account_id, e
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

        let tiers = match repo.get_rate_tiers(config.id).await {
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

        let (daily_interest, breakdowns) =
            calculate_daily_interest(account.balance, &tiers, &day_count);

        if daily_interest == Decimal::ZERO {
            skip_count += 1;
            continue;
        }

        let tier_breakdown =
            serde_json::to_value(&breakdowns).unwrap_or(serde_json::Value::Array(vec![]));

        let accrual = InterestAccrual {
            id: Uuid::new_v4(),
            tenant_id: account.tenant_id,
            account_id: account.account_id.clone(),
            ledger_id: account.ledger_id.clone(),
            currency: account.currency.clone(),
            accrual_date,
            balance_used: account.balance,
            daily_interest,
            rate_config_id: config.id,
            tier_breakdown,
        };

        match repo.create_accrual(accrual).await {
            Ok(()) => success_count += 1,
            Err(e) => {
                error!(
                    "Failed to insert accrual for account {}: {:?}",
                    account.account_id, e
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
    use crate::interest::models::{InterestEligibleAccount, InterestRateConfig, InterestRateTier};
    use crate::interest::repository::MockInterestRepository;
    use chrono::NaiveDate;
    use mockall::predicate::*;
    use rust_decimal_macros::dec;

    fn make_eligible_account(
        id: &str,
        ledger_id: &str,
        currency: &str,
        balance: Decimal,
        tenant_id: i32,
    ) -> InterestEligibleAccount {
        InterestEligibleAccount {
            tenant_id,
            account_id: id.to_string(),
            ledger_id: ledger_id.to_string(),
            currency: currency.to_string(),
            balance,
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
        let mut mock = MockInterestRepository::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| Ok(vec![]));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_no_rate_config() {
        let mut mock = MockInterestRepository::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(|_, _, _| Ok(None));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_skips_empty_tiers() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();
        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id, "USD"))));
        mock.expect_get_rate_tiers()
            .times(1)
            .returning(|_| Ok(vec![]));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_inserts_record_successfully() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_create_accrual()
            .times(1)
            .withf(|accrual| {
                accrual.account_id == "acc-1"
                    && accrual.currency == "USD"
                    && accrual.balance_used == dec!(10000)
                    && accrual.daily_interest > Decimal::ZERO
            })
            .returning(|_| Ok(()));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_handles_insert_error_gracefully() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_create_accrual()
            .times(1)
            .returning(|_| Err(sqlx::Error::Protocol("test error".to_string())));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_multiple_accounts_mixed_results() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
                Ok(vec![
                    make_eligible_account("acc-1", "led-1", "USD", dec!(10000), 1),
                    make_eligible_account("acc-3", "led-3", "TWD", dec!(50000), 2),
                ])
            });

        let config_id_clone = config_id;
        mock.expect_get_active_rate_config()
            .times(2)
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(2)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        mock.expect_create_accrual().times(2).returning(|_| Ok(()));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_correct_daily_interest_amount() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0.045))]));

        // Verify the exact daily interest: 10000 * 0.045 / 365
        let expected_daily = dec!(10000) * dec!(0.045) / dec!(365);
        mock.expect_create_accrual()
            .times(1)
            .withf(move |accrual| accrual.daily_interest == expected_daily)
            .returning(|_| Ok(()));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_rate_config_lookup_error() {
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(|_, _, _| Err(sqlx::Error::Protocol("db error".to_string())));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok()); // errors are logged, not propagated
    }

    #[tokio::test]
    async fn test_accrual_tier_lookup_error() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        mock.expect_get_rate_tiers()
            .times(1)
            .returning(|_| Err(sqlx::Error::Protocol("db error".to_string())));

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_accrual_zero_interest_skipped() {
        let config_id = Uuid::new_v4();
        let mut mock = MockInterestRepository::new();

        mock.expect_get_interest_eligible_accounts()
            .times(1)
            .returning(|_| {
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
            .returning(move |_, _, _| Ok(Some(make_rate_config(config_id_clone, "USD"))));

        // 0% APR → zero daily interest → should skip insert
        mock.expect_get_rate_tiers()
            .times(1)
            .returning(move |_| Ok(vec![make_tier(config_id, 1, dec!(0), None, dec!(0))]));

        // create_accrual should NOT be called
        mock.expect_create_accrual().times(0);

        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let result = run_accrual(&mock, date).await;
        assert!(result.is_ok());
    }
}
