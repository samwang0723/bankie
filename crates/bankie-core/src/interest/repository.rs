use async_trait::async_trait;
use chrono::NaiveDate;
use mockall::automock;
use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

use super::models::{
    InterestAccrual, InterestEligibleAccount, InterestPosting, InterestRateConfig, InterestRateTier,
};

/// Lightweight struct for the posting job to iterate over accounts.
#[derive(Debug, Clone)]
pub struct InterestAccountInfo {
    pub account_id: String,
    pub ledger_id: String,
    pub currency: String,
    pub tenant_id: i32,
}

/// Unified repository trait for all interest engine operations.
/// Used by accrual_job, posting_job, and route handlers.
#[allow(clippy::too_many_arguments)]
#[automock]
#[async_trait]
pub trait InterestRepository: Send + Sync {
    // ── Rate config queries ──────────────────────────────────────────────

    async fn get_active_rate_config(
        &self,
        currency: &str,
        account_kind: &str,
        date: NaiveDate,
    ) -> Result<Option<InterestRateConfig>, Error>;

    async fn get_rate_config_by_id(&self, id: Uuid) -> Result<Option<InterestRateConfig>, Error>;

    async fn get_rate_tiers(&self, rate_config_id: Uuid) -> Result<Vec<InterestRateTier>, Error>;

    async fn list_rate_configs(
        &self,
        currency: Option<String>,
        active_only: bool,
    ) -> Result<Vec<InterestRateConfig>, Error>;

    async fn upsert_rate_config(
        &self,
        config: &InterestRateConfig,
    ) -> Result<InterestRateConfig, Error>;

    async fn sunset_rate_config(
        &self,
        id: Uuid,
        effective_to: Option<NaiveDate>,
        is_active: bool,
    ) -> Result<InterestRateConfig, Error>;

    async fn replace_rate_tiers(
        &self,
        rate_config_id: Uuid,
        tiers: &[InterestRateTier],
    ) -> Result<(), Error>;

    // ── Accrual operations ───────────────────────────────────────────────

    async fn get_interest_eligible_accounts(
        &self,
        accrual_date: NaiveDate,
    ) -> Result<Vec<InterestEligibleAccount>, Error>;

    async fn create_accrual(&self, accrual: InterestAccrual) -> Result<(), Error>;

    async fn sum_accruals_for_period(
        &self,
        account_id: &str,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> Result<Decimal, Error>;

    async fn get_accrual_history(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestAccrual>, Error>;

    // ── Posting operations ───────────────────────────────────────────────

    async fn get_postings(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestPosting>, Error>;

    async fn create_posting(&self, posting: InterestPosting) -> Result<(), Error>;

    #[allow(dead_code)]
    async fn update_posting_status(
        &self,
        posting_id: Uuid,
        status: &str,
        transaction_id: Option<Uuid>,
        error_message: Option<String>,
    ) -> Result<(), Error>;

    async fn get_last_posting_date(&self, account_id: &str) -> Result<Option<NaiveDate>, Error>;

    // ── Account queries ──────────────────────────────────────────────────

    async fn list_interest_accounts(&self) -> Result<Vec<InterestAccountInfo>, Error>;

    async fn get_current_balance(&self, ledger_id: &str) -> Result<Decimal, Error>;
}

/// PostgreSQL implementation of `InterestRepository`.
#[derive(Clone)]
pub struct PgInterestRepository {
    pool: PgPool,
}

impl PgInterestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InterestRepository for PgInterestRepository {
    async fn get_active_rate_config(
        &self,
        currency: &str,
        account_kind: &str,
        date: NaiveDate,
    ) -> Result<Option<InterestRateConfig>, Error> {
        sqlx::query_as::<_, InterestRateConfig>(
            r#"
            SELECT id, currency, account_kind, day_count, posting_frequency,
                   posting_day, effective_from, effective_to, is_active
            FROM interest_rate_configs
            WHERE currency = $1
              AND account_kind = $2
              AND is_active = true
              AND effective_from <= $3
              AND (effective_to IS NULL OR effective_to >= $3)
            ORDER BY effective_from DESC
            LIMIT 1
            "#,
        )
        .bind(currency)
        .bind(account_kind)
        .bind(date)
        .fetch_optional(&self.pool)
        .await
    }

    async fn get_rate_config_by_id(&self, id: Uuid) -> Result<Option<InterestRateConfig>, Error> {
        sqlx::query_as::<_, InterestRateConfig>(
            r#"
            SELECT id, currency, account_kind, day_count, posting_frequency,
                   posting_day, effective_from, effective_to, is_active
            FROM interest_rate_configs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn get_rate_tiers(&self, rate_config_id: Uuid) -> Result<Vec<InterestRateTier>, Error> {
        sqlx::query_as::<_, InterestRateTier>(
            r#"
            SELECT id, rate_config_id, tier_order, min_balance, max_balance, apr
            FROM interest_rate_tiers
            WHERE rate_config_id = $1
            ORDER BY tier_order
            "#,
        )
        .bind(rate_config_id)
        .fetch_all(&self.pool)
        .await
    }

    async fn list_rate_configs(
        &self,
        currency: Option<String>,
        active_only: bool,
    ) -> Result<Vec<InterestRateConfig>, Error> {
        let mut query = String::from(
            "SELECT id, currency, account_kind, day_count, posting_frequency, \
             posting_day, effective_from, effective_to, is_active \
             FROM interest_rate_configs WHERE 1=1",
        );
        if currency.is_some() {
            query.push_str(" AND currency = $1");
        }
        if active_only {
            query.push_str(" AND is_active = true");
        }
        query.push_str(" ORDER BY effective_from DESC");

        if let Some(ref curr) = currency {
            sqlx::query_as::<_, InterestRateConfig>(&query)
                .bind(curr)
                .fetch_all(&self.pool)
                .await
        } else {
            sqlx::query_as::<_, InterestRateConfig>(&query)
                .fetch_all(&self.pool)
                .await
        }
    }

    async fn upsert_rate_config(
        &self,
        config: &InterestRateConfig,
    ) -> Result<InterestRateConfig, Error> {
        sqlx::query_as::<_, InterestRateConfig>(
            r#"
            INSERT INTO interest_rate_configs
                (id, currency, account_kind, day_count, posting_frequency,
                 posting_day, effective_from, effective_to, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                currency = EXCLUDED.currency,
                account_kind = EXCLUDED.account_kind,
                day_count = EXCLUDED.day_count,
                posting_frequency = EXCLUDED.posting_frequency,
                posting_day = EXCLUDED.posting_day,
                effective_from = EXCLUDED.effective_from,
                effective_to = EXCLUDED.effective_to,
                is_active = EXCLUDED.is_active,
                updated_at = now()
            RETURNING id, currency, account_kind, day_count, posting_frequency,
                      posting_day, effective_from, effective_to, is_active
            "#,
        )
        .bind(config.id)
        .bind(&config.currency)
        .bind(&config.account_kind)
        .bind(&config.day_count)
        .bind(&config.posting_frequency)
        .bind(config.posting_day)
        .bind(config.effective_from)
        .bind(config.effective_to)
        .bind(config.is_active)
        .fetch_one(&self.pool)
        .await
    }

    async fn sunset_rate_config(
        &self,
        id: Uuid,
        effective_to: Option<NaiveDate>,
        is_active: bool,
    ) -> Result<InterestRateConfig, Error> {
        sqlx::query_as::<_, InterestRateConfig>(
            r#"
            UPDATE interest_rate_configs
            SET effective_to = $2, is_active = $3, updated_at = now()
            WHERE id = $1
            RETURNING id, currency, account_kind, day_count, posting_frequency,
                      posting_day, effective_from, effective_to, is_active
            "#,
        )
        .bind(id)
        .bind(effective_to)
        .bind(is_active)
        .fetch_one(&self.pool)
        .await
    }

    async fn replace_rate_tiers(
        &self,
        rate_config_id: Uuid,
        tiers: &[InterestRateTier],
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM interest_rate_tiers WHERE rate_config_id = $1")
            .bind(rate_config_id)
            .execute(&mut *tx)
            .await?;

        for tier in tiers {
            sqlx::query(
                r#"
                INSERT INTO interest_rate_tiers
                    (id, rate_config_id, tier_order, min_balance, max_balance, apr)
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(tier.id)
            .bind(rate_config_id)
            .bind(tier.tier_order)
            .bind(tier.min_balance)
            .bind(tier.max_balance)
            .bind(tier.apr)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn get_interest_eligible_accounts(
        &self,
        accrual_date: NaiveDate,
    ) -> Result<Vec<InterestEligibleAccount>, Error> {
        let rows = sqlx::query(
            r#"
            SELECT bav.tenant_id, bav.id AS account_id, bav.ledger_id,
                   bav.currency, bs.balance
            FROM bank_account_view bav
            INNER JOIN balance_snapshots bs
                ON bs.account_id = bav.id AND bs.snapshot_date = $1
            WHERE bav.kind = 'Interest'
              AND bav.status = 'Approved'
              AND bs.balance > 0
            "#,
        )
        .bind(accrual_date)
        .fetch_all(&self.pool)
        .await?;

        let accounts = rows
            .iter()
            .filter_map(|row| {
                let tenant_id: Option<i32> = row.try_get("tenant_id").ok();
                let account_id: Option<String> = row.try_get("account_id").ok();
                let ledger_id: Option<String> = row.try_get("ledger_id").ok();
                let currency: Option<String> = row.try_get("currency").ok();
                let balance: Option<Decimal> = row.try_get("balance").ok();

                match (tenant_id, account_id, ledger_id, currency, balance) {
                    (Some(t), Some(a), Some(l), Some(c), Some(b)) => {
                        Some(InterestEligibleAccount {
                            tenant_id: t,
                            account_id: a,
                            ledger_id: l,
                            currency: c,
                            balance: b,
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        Ok(accounts)
    }

    async fn create_accrual(&self, accrual: InterestAccrual) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO interest_accruals
                (id, tenant_id, account_id, ledger_id, currency, accrual_date,
                 balance_used, daily_interest, rate_config_id, tier_breakdown)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (account_id, accrual_date) DO NOTHING
            "#,
        )
        .bind(accrual.id)
        .bind(accrual.tenant_id)
        .bind(&accrual.account_id)
        .bind(&accrual.ledger_id)
        .bind(&accrual.currency)
        .bind(accrual.accrual_date)
        .bind(accrual.balance_used)
        .bind(accrual.daily_interest)
        .bind(accrual.rate_config_id)
        .bind(&accrual.tier_breakdown)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn sum_accruals_for_period(
        &self,
        account_id: &str,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> Result<Decimal, Error> {
        let result: (Decimal,) = sqlx::query_as(
            r#"
            SELECT COALESCE(SUM(daily_interest), 0) as total
            FROM interest_accruals
            WHERE account_id = $1
              AND accrual_date >= $2
              AND accrual_date <= $3
            "#,
        )
        .bind(account_id)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(&self.pool)
        .await?;
        Ok(result.0)
    }

    async fn get_accrual_history(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestAccrual>, Error> {
        sqlx::query_as::<_, InterestAccrual>(
            r#"
            SELECT id, tenant_id, account_id, ledger_id, currency, accrual_date,
                   balance_used, daily_interest, rate_config_id, tier_breakdown
            FROM interest_accruals
            WHERE account_id = $1
              AND accrual_date >= $2
              AND accrual_date <= $3
              AND tenant_id = $4
            ORDER BY accrual_date DESC
            "#,
        )
        .bind(account_id)
        .bind(start_date)
        .bind(end_date)
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
    }

    async fn get_postings(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestPosting>, Error> {
        sqlx::query_as::<_, InterestPosting>(
            r#"
            SELECT id, tenant_id, account_id, currency, posting_date,
                   period_start, period_end, accrued_total, posted_amount,
                   transaction_id, status, error_message
            FROM interest_postings
            WHERE account_id = $1
              AND posting_date >= $2
              AND posting_date <= $3
              AND tenant_id = $4
            ORDER BY posting_date DESC
            "#,
        )
        .bind(account_id)
        .bind(start_date)
        .bind(end_date)
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
    }

    async fn create_posting(&self, posting: InterestPosting) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO interest_postings
                (id, tenant_id, account_id, currency, posting_date,
                 period_start, period_end, accrued_total, posted_amount,
                 transaction_id, status, error_message)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(posting.id)
        .bind(posting.tenant_id)
        .bind(&posting.account_id)
        .bind(&posting.currency)
        .bind(posting.posting_date)
        .bind(posting.period_start)
        .bind(posting.period_end)
        .bind(posting.accrued_total)
        .bind(posting.posted_amount)
        .bind(posting.transaction_id)
        .bind(&posting.status)
        .bind(&posting.error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_posting_status(
        &self,
        posting_id: Uuid,
        status: &str,
        transaction_id: Option<Uuid>,
        error_message: Option<String>,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE interest_postings
            SET status = $2, transaction_id = $3, error_message = $4, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(posting_id)
        .bind(status)
        .bind(transaction_id)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_last_posting_date(&self, account_id: &str) -> Result<Option<NaiveDate>, Error> {
        sqlx::query_scalar::<_, NaiveDate>(
            r#"
            SELECT posting_date
            FROM interest_postings
            WHERE account_id = $1
              AND status = 'completed'
            ORDER BY posting_date DESC
            LIMIT 1
            "#,
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn list_interest_accounts(&self) -> Result<Vec<InterestAccountInfo>, Error> {
        let rows = sqlx::query(
            r#"
            SELECT bav.id AS account_id, bav.ledger_id, bav.currency, bav.tenant_id
            FROM bank_account_view bav
            WHERE bav.kind = 'Interest'
              AND bav.status = 'Approved'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let accounts = rows
            .iter()
            .filter_map(|row| {
                let account_id: Option<String> = row.try_get("account_id").ok();
                let ledger_id: Option<String> = row.try_get("ledger_id").ok();
                let currency: Option<String> = row.try_get("currency").ok();
                let tenant_id: Option<i32> = row.try_get("tenant_id").ok();

                match (account_id, ledger_id, currency, tenant_id) {
                    (Some(a), Some(l), Some(c), Some(t)) => Some(InterestAccountInfo {
                        account_id: a,
                        ledger_id: l,
                        currency: c,
                        tenant_id: t,
                    }),
                    _ => None,
                }
            })
            .collect();

        Ok(accounts)
    }

    async fn get_current_balance(&self, ledger_id: &str) -> Result<Decimal, Error> {
        let balance: Option<Decimal> = sqlx::query_scalar(
            r#"
            SELECT (payload->'available'->>'amount')::numeric
            FROM ledger_views
            WHERE view_id = $1
            "#,
        )
        .bind(ledger_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(balance.unwrap_or(Decimal::ZERO))
    }
}
