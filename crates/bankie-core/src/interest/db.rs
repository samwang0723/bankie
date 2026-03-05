use async_trait::async_trait;
use chrono::NaiveDate;
use mockall::automock;
use rust_decimal::Decimal;
use sqlx::Error;
use uuid::Uuid;

use super::models::{InterestAccrual, InterestPosting, InterestRateConfig, InterestRateTier};

/// Interest-specific DB operations.
/// dev-1 implements the SQL layer; dev-2 codes against this trait + mock.
#[automock]
#[async_trait]
pub trait InterestDbClient: Send + Sync {
    // ── Rate config CRUD ────────────────────────────────────────────────
    async fn get_active_rate_configs(
        &self,
        currency: Option<String>,
    ) -> Result<Vec<InterestRateConfig>, Error>;

    async fn get_rate_config_by_id(&self, id: Uuid) -> Result<Option<InterestRateConfig>, Error>;

    async fn upsert_rate_config(
        &self,
        config: &InterestRateConfig,
    ) -> Result<InterestRateConfig, Error>;

    async fn get_tiers_for_config(
        &self,
        rate_config_id: Uuid,
    ) -> Result<Vec<InterestRateTier>, Error>;

    async fn replace_tiers(
        &self,
        rate_config_id: Uuid,
        tiers: &[InterestRateTier],
    ) -> Result<(), Error>;

    /// Sunset a rate config: update effective_to and/or is_active.
    /// Never modifies rate values (currency, day_count, posting_frequency, etc.).
    async fn sunset_rate_config(
        &self,
        id: Uuid,
        effective_to: Option<NaiveDate>,
        is_active: bool,
    ) -> Result<InterestRateConfig, Error>;

    // ── Accrual read ────────────────────────────────────────────────────
    async fn get_accruals(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestAccrual>, Error>;

    /// Sum of daily_interest for un-posted accruals in [period_start, period_end).
    async fn sum_unposted_accruals(
        &self,
        account_id: &str,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> Result<Decimal, Error>;

    // ── Posting CRUD ────────────────────────────────────────────────────
    async fn get_postings(
        &self,
        account_id: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<InterestPosting>, Error>;

    async fn create_posting(&self, posting: &InterestPosting) -> Result<(), Error>;

    async fn update_posting_status(
        &self,
        posting_id: Uuid,
        status: &str,
        transaction_id: Option<Uuid>,
        error_message: Option<String>,
    ) -> Result<(), Error>;

    /// Find last posting end date for an account (to determine next period_start).
    async fn last_posting_end_date(&self, account_id: &str) -> Result<Option<NaiveDate>, Error>;

    // ── Account enumeration for posting job ─────────────────────────────
    /// List all Interest-type sub-accounts with their ledger_id, currency, tenant_id.
    async fn list_interest_accounts(&self) -> Result<Vec<InterestAccountInfo>, Error>;

    // ── Estimate helper ─────────────────────────────────────────────────
    async fn get_current_balance(&self, ledger_id: &str) -> Result<Decimal, Error>;
}

/// Lightweight struct for the posting job to iterate over accounts.
#[derive(Debug, Clone)]
pub struct InterestAccountInfo {
    pub account_id: String,
    pub ledger_id: String,
    pub currency: String,
    pub tenant_id: i32,
}
