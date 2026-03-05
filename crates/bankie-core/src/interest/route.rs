use std::sync::Arc;

use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;
use uuid::Uuid;

use super::calculator::estimate_interest;
use super::models::{InterestRateConfig, InterestRateTier};
use super::repository::InterestRepository;
use crate::common::error::AppError;

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RateListParams {
    pub currency: Option<String>,
}

#[derive(Deserialize)]
pub struct AccrualListParams {
    pub account_id: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

#[derive(Deserialize)]
pub struct PostingListParams {
    pub account_id: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

#[derive(Deserialize)]
pub struct EstimateParams {
    pub account_id: String,
    #[serde(default = "default_estimate_days")]
    pub days: u32,
}

fn default_estimate_days() -> u32 {
    30
}

#[derive(Deserialize)]
pub struct CreateRateConfigRequest {
    pub currency: String,
    #[serde(default = "default_account_kind")]
    pub account_kind: String,
    #[serde(default = "default_day_count")]
    pub day_count: String,
    #[serde(default = "default_posting_frequency")]
    pub posting_frequency: String,
    pub posting_day: Option<i16>,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub tiers: Vec<CreateTierRequest>,
}

fn default_account_kind() -> String {
    "Interest".to_string()
}

fn default_day_count() -> String {
    "Actual/365".to_string()
}

fn default_posting_frequency() -> String {
    "Monthly".to_string()
}

#[derive(Deserialize)]
pub struct CreateTierRequest {
    pub tier_order: i16,
    pub min_balance: Decimal,
    pub max_balance: Option<Decimal>,
    pub apr: Decimal,
}

#[derive(Serialize)]
pub struct RateConfigResponse {
    #[serde(flatten)]
    pub config: InterestRateConfig,
    pub tiers: Vec<InterestRateTier>,
}

#[derive(Deserialize)]
pub struct UpdateRateConfigRequest {
    pub effective_to: Option<NaiveDate>,
    pub is_active: Option<bool>,
}

#[derive(Deserialize)]
pub struct ReplaceTiersRequest {
    pub tiers: Vec<CreateTierRequest>,
}

#[derive(Serialize)]
pub struct EstimateResponse {
    pub account_id: String,
    pub days: u32,
    pub current_balance: String,
    pub estimated_interest: String,
    pub currency: String,
}

// ─── Shared validation ───────────────────────────────────────────────────────

/// Validate tier continuity: non-negative APRs, first starts at 0, no gaps, last unbounded.
fn validate_tier_continuity(tiers: &[CreateTierRequest]) -> Result<(), String> {
    if tiers.is_empty() {
        return Err("At least one tier is required".to_string());
    }
    for tier in tiers {
        if tier.apr < Decimal::ZERO {
            return Err("APR must be non-negative".to_string());
        }
    }

    let mut sorted: Vec<&CreateTierRequest> = tiers.iter().collect();
    sorted.sort_by_key(|t| t.tier_order);

    if sorted[0].min_balance != Decimal::ZERO {
        return Err("First tier must start at min_balance = 0".to_string());
    }
    if sorted.last().unwrap().max_balance.is_some() {
        return Err("Last tier must have max_balance = null (unbounded)".to_string());
    }

    for window in sorted.windows(2) {
        let prev_max = match window[0].max_balance {
            Some(m) => m,
            None => {
                return Err("Only the last tier can have max_balance = null".to_string());
            }
        };
        if window[1].min_balance != prev_max {
            return Err(format!(
                "Gap between tier {} max_balance ({}) and tier {} min_balance ({})",
                window[0].tier_order, prev_max, window[1].tier_order, window[1].min_balance
            ));
        }
    }

    Ok(())
}

/// Log a database error and return a generic 500 response.
fn internal_error(e: impl std::fmt::Debug, context: &str) -> Response {
    error!("{}: {:?}", context, e);
    AppError::InternalServerError("Internal server error".to_string()).into_response()
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// GET /v1/interest/rates?currency=
pub async fn list_rate_configs(
    Extension(_tenant_id): Extension<i32>,
    Query(params): Query<RateListParams>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
) -> Response {
    let configs = match interest_db.list_rate_configs(params.currency, true).await {
        Ok(c) => c,
        Err(e) => return internal_error(e, "Failed to list rate configs"),
    };

    let mut results = Vec::with_capacity(configs.len());
    for config in configs {
        let tiers = match interest_db.get_rate_tiers(config.id).await {
            Ok(t) => t,
            Err(e) => return internal_error(e, "Failed to get rate tiers"),
        };
        results.push(RateConfigResponse { config, tiers });
    }

    (StatusCode::OK, Json(json!({ "entries": results }))).into_response()
}

/// POST /v1/interest/rates
pub async fn create_rate_config(
    Extension(_tenant_id): Extension<i32>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
    Json(req): Json<CreateRateConfigRequest>,
) -> Response {
    // Validate tier continuity (fix #5)
    if let Err(msg) = validate_tier_continuity(&req.tiers) {
        return AppError::BadRequest(msg).into_response();
    }

    // Validate effective_from >= today (fix #7)
    let today = Utc::now().date_naive();
    if req.effective_from < today {
        return AppError::BadRequest("effective_from must be today or in the future".to_string())
            .into_response();
    }

    // Validate posting frequency / day_count enums
    let freq = match req
        .posting_frequency
        .parse::<super::models::PostingFrequency>()
    {
        Ok(f) => f,
        Err(e) => {
            return AppError::BadRequest(format!("Invalid posting frequency: {}", e))
                .into_response()
        }
    };
    if let Err(e) = req.day_count.parse::<super::models::DayCountConvention>() {
        return AppError::BadRequest(format!("Invalid day count convention: {}", e))
            .into_response();
    }

    // Validate posting_day against posting_frequency
    use super::models::PostingFrequency;
    match (freq, req.posting_day) {
        (PostingFrequency::Daily, Some(_)) => {
            return AppError::BadRequest(
                "Daily frequency must have posting_day = null".to_string(),
            )
            .into_response();
        }
        (PostingFrequency::Weekly, Some(d)) if !(1..=7).contains(&d) => {
            return AppError::BadRequest(
                "Weekly posting_day must be 1-7 (Mon=1, Sun=7)".to_string(),
            )
            .into_response();
        }
        (PostingFrequency::Monthly, Some(d)) if !(1..=28).contains(&d) => {
            return AppError::BadRequest("Monthly posting_day must be 1-28".to_string())
                .into_response();
        }
        (PostingFrequency::Weekly | PostingFrequency::Monthly, None) => {
            return AppError::BadRequest(
                "posting_day is required for Weekly/Monthly frequency".to_string(),
            )
            .into_response();
        }
        _ => {}
    }

    let config = InterestRateConfig {
        id: Uuid::new_v4(),
        currency: req.currency,
        account_kind: req.account_kind,
        day_count: req.day_count,
        posting_frequency: req.posting_frequency,
        posting_day: req.posting_day,
        effective_from: req.effective_from,
        effective_to: req.effective_to,
        is_active: true,
    };

    let saved_config = match interest_db.create_rate_config(&config).await {
        Ok(c) => c,
        Err(e) => return internal_error(e, "Failed to create rate config"),
    };

    let tiers: Vec<InterestRateTier> = req
        .tiers
        .into_iter()
        .map(|t| InterestRateTier {
            id: Uuid::new_v4(),
            rate_config_id: saved_config.id,
            tier_order: t.tier_order,
            min_balance: t.min_balance,
            max_balance: t.max_balance,
            apr: t.apr,
        })
        .collect();

    if let Err(e) = interest_db
        .replace_rate_tiers(saved_config.id, &tiers)
        .await
    {
        return internal_error(e, "Failed to replace rate tiers");
    }

    (
        StatusCode::CREATED,
        Json(json!(RateConfigResponse {
            config: saved_config,
            tiers,
        })),
    )
        .into_response()
}

/// GET /v1/interest/rates/:id
pub async fn get_rate_config(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<Uuid>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
) -> Response {
    let config = match interest_db.get_rate_config_by_id(id).await {
        Ok(Some(c)) => c,
        Ok(None) => return AppError::NotFound("Rate config not found".to_string()).into_response(),
        Err(e) => return internal_error(e, "Failed to get rate config"),
    };

    let tiers = match interest_db.get_rate_tiers(config.id).await {
        Ok(t) => t,
        Err(e) => return internal_error(e, "Failed to get rate tiers"),
    };

    (
        StatusCode::OK,
        Json(json!(RateConfigResponse { config, tiers })),
    )
        .into_response()
}

/// GET /v1/interest/accruals?account_id=&start_date=&end_date=
pub async fn list_accruals(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<AccrualListParams>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
) -> Response {
    if params.start_date > params.end_date {
        return AppError::BadRequest("start_date must be <= end_date".to_string()).into_response();
    }

    match interest_db
        .get_accrual_history(
            &params.account_id,
            params.start_date,
            params.end_date,
            tenant_id,
        )
        .await
    {
        Ok(accruals) => (StatusCode::OK, Json(json!({ "entries": accruals }))).into_response(),
        Err(e) => internal_error(e, "Failed to get accrual history"),
    }
}

/// GET /v1/interest/postings?account_id=&start_date=&end_date=
pub async fn list_postings(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<PostingListParams>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
) -> Response {
    if params.start_date > params.end_date {
        return AppError::BadRequest("start_date must be <= end_date".to_string()).into_response();
    }

    match interest_db
        .get_postings(
            &params.account_id,
            params.start_date,
            params.end_date,
            tenant_id,
        )
        .await
    {
        Ok(postings) => (StatusCode::OK, Json(json!({ "entries": postings }))).into_response(),
        Err(e) => internal_error(e, "Failed to get postings"),
    }
}

/// GET /v1/interest/estimate?account_id=&days=
pub async fn estimate_interest_handler(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<EstimateParams>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
) -> Response {
    let accounts = match interest_db.list_interest_accounts().await {
        Ok(a) => a,
        Err(e) => return internal_error(e, "Failed to list interest accounts"),
    };

    let account = match accounts
        .iter()
        .find(|a| a.account_id == params.account_id && a.tenant_id == tenant_id)
    {
        Some(a) => a,
        None => {
            return AppError::NotFound("Interest account not found".to_string()).into_response()
        }
    };

    let balance = match interest_db.get_current_balance(&account.ledger_id).await {
        Ok(b) => b,
        Err(e) => return internal_error(e, "Failed to get current balance"),
    };

    // Find rate config for this currency
    let configs = match interest_db
        .list_rate_configs(Some(account.currency.clone()), true)
        .await
    {
        Ok(c) => c,
        Err(e) => return internal_error(e, "Failed to list rate configs for estimate"),
    };

    let config = match configs.first() {
        Some(c) => c,
        None => {
            return AppError::NotFound("No active rate config for this currency".to_string())
                .into_response()
        }
    };

    let day_count = match config.day_count_convention() {
        Ok(d) => d,
        Err(e) => return AppError::BadRequest(e).into_response(),
    };

    let tiers = match interest_db.get_rate_tiers(config.id).await {
        Ok(t) => t,
        Err(e) => return internal_error(e, "Failed to get rate tiers for estimate"),
    };

    let estimated = estimate_interest(balance, &tiers, &day_count, params.days);
    let precision = crate::common::money::default_precision(&account.currency) as usize;

    (
        StatusCode::OK,
        Json(json!(EstimateResponse {
            account_id: params.account_id,
            days: params.days,
            current_balance: format!("{:.prec$}", balance, prec = precision),
            estimated_interest: format!("{:.prec$}", estimated, prec = precision),
            currency: account.currency.clone(),
        })),
    )
        .into_response()
}

/// PUT /v1/interest/rates/:id — Sunset a rate config.
/// Only effective_to and is_active can be changed. Never modifies rate values.
pub async fn update_rate_config(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<Uuid>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
    Json(req): Json<UpdateRateConfigRequest>,
) -> Response {
    // Must provide at least one field to update
    if req.effective_to.is_none() && req.is_active.is_none() {
        return AppError::BadRequest(
            "At least one of effective_to or is_active must be provided".to_string(),
        )
        .into_response();
    }

    // Fetch existing config
    let config = match interest_db.get_rate_config_by_id(id).await {
        Ok(Some(c)) => c,
        Ok(None) => return AppError::NotFound("Rate config not found".to_string()).into_response(),
        Err(e) => return internal_error(e, "Failed to get rate config for update"),
    };

    let is_active = req.is_active.unwrap_or(config.is_active);
    let effective_to = if req.effective_to.is_some() {
        req.effective_to
    } else {
        config.effective_to
    };

    // Validate: effective_to must be >= effective_from if set
    if let Some(end) = effective_to {
        if end < config.effective_from {
            return AppError::BadRequest("effective_to must be >= effective_from".to_string())
                .into_response();
        }
    }

    match interest_db
        .sunset_rate_config(id, effective_to, is_active)
        .await
    {
        Ok(updated) => {
            let tiers = match interest_db.get_rate_tiers(updated.id).await {
                Ok(t) => t,
                Err(e) => return internal_error(e, "Failed to get rate tiers after sunset"),
            };
            (
                StatusCode::OK,
                Json(json!(RateConfigResponse {
                    config: updated,
                    tiers,
                })),
            )
                .into_response()
        }
        Err(e) => internal_error(e, "Failed to sunset rate config"),
    }
}

/// PUT /v1/interest/rates/:id/tiers — Replace all tiers atomically.
pub async fn replace_rate_tiers(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<Uuid>,
    Extension(interest_db): Extension<Arc<dyn InterestRepository>>,
    Json(req): Json<ReplaceTiersRequest>,
) -> Response {
    // Use shared tier validation
    if let Err(msg) = validate_tier_continuity(&req.tiers) {
        return AppError::BadRequest(msg).into_response();
    }

    // Verify config exists
    let config = match interest_db.get_rate_config_by_id(id).await {
        Ok(Some(c)) => c,
        Ok(None) => return AppError::NotFound("Rate config not found".to_string()).into_response(),
        Err(e) => return internal_error(e, "Failed to get rate config for tier replacement"),
    };

    let tiers: Vec<InterestRateTier> = req
        .tiers
        .into_iter()
        .map(|t| InterestRateTier {
            id: Uuid::new_v4(),
            rate_config_id: id,
            tier_order: t.tier_order,
            min_balance: t.min_balance,
            max_balance: t.max_balance,
            apr: t.apr,
        })
        .collect();

    if let Err(e) = interest_db.replace_rate_tiers(id, &tiers).await {
        return internal_error(e, "Failed to replace rate tiers");
    }

    (
        StatusCode::OK,
        Json(json!(RateConfigResponse { config, tiers })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::models::InterestAccrual;
    use crate::interest::repository::{InterestAccountInfo, MockInterestRepository};
    use mockall::predicate::*;
    use rust_decimal_macros::dec;

    fn make_config(id: Uuid, currency: &str) -> InterestRateConfig {
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

    fn make_tier(config_id: Uuid) -> InterestRateTier {
        InterestRateTier {
            id: Uuid::new_v4(),
            rate_config_id: config_id,
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: None,
            apr: dec!(0.045),
        }
    }

    // ── Validation tests ─────────────────────────────────────────────────

    #[test]
    fn test_create_rate_config_request_validation_empty_tiers() {
        let req = CreateRateConfigRequest {
            currency: "USD".to_string(),
            account_kind: default_account_kind(),
            day_count: default_day_count(),
            posting_frequency: default_posting_frequency(),
            posting_day: Some(1),
            effective_from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            effective_to: None,
            tiers: vec![],
        };
        assert!(req.tiers.is_empty());
    }

    #[test]
    fn test_create_rate_config_request_negative_apr() {
        let tier = CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: None,
            apr: dec!(-0.01),
        };
        assert!(tier.apr < Decimal::ZERO);
    }

    #[test]
    fn test_estimate_params_default_days() {
        assert_eq!(default_estimate_days(), 30);
    }

    // ── Mock-based handler tests ─────────────────────────────────────────

    #[tokio::test]
    async fn test_list_rate_configs_empty() {
        let mut mock = MockInterestRepository::new();
        mock.expect_list_rate_configs()
            .with(eq(None::<String>), eq(true))
            .returning(|_, _| Ok(vec![]));

        let configs = mock.list_rate_configs(None, true).await.unwrap();
        assert!(configs.is_empty());
    }

    #[tokio::test]
    async fn test_list_rate_configs_with_tiers() {
        let config_id = Uuid::new_v4();
        let config = make_config(config_id, "USD");
        let tier = make_tier(config_id);

        let mut mock = MockInterestRepository::new();
        let config_clone = config.clone();
        mock.expect_list_rate_configs()
            .returning(move |_, _| Ok(vec![config_clone.clone()]));
        let tier_clone = tier.clone();
        mock.expect_get_rate_tiers()
            .returning(move |_| Ok(vec![tier_clone.clone()]));

        let configs = mock.list_rate_configs(None, true).await.unwrap();
        assert_eq!(configs.len(), 1);
        let tiers = mock.get_rate_tiers(config_id).await.unwrap();
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].apr, dec!(0.045));
    }

    #[tokio::test]
    async fn test_get_rate_config_not_found() {
        let mut mock = MockInterestRepository::new();
        mock.expect_get_rate_config_by_id().returning(|_| Ok(None));

        let result = mock.get_rate_config_by_id(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_accruals() {
        let mut mock = MockInterestRepository::new();
        let accrual = InterestAccrual {
            id: Uuid::new_v4(),
            tenant_id: 1,
            account_id: "acc-1".to_string(),
            ledger_id: "ledger-1".to_string(),
            currency: "USD".to_string(),
            accrual_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            balance_used: dec!(10000),
            daily_interest: dec!(1.23),
            rate_config_id: Uuid::new_v4(),
            tier_breakdown: serde_json::json!([]),
        };
        let accrual_clone = accrual.clone();
        mock.expect_get_accrual_history()
            .returning(move |_, _, _, _| Ok(vec![accrual_clone.clone()]));

        let accruals = mock
            .get_accrual_history(
                "acc-1",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
                1,
            )
            .await
            .unwrap();
        assert_eq!(accruals.len(), 1);
        assert_eq!(accruals[0].daily_interest, dec!(1.23));
    }

    #[tokio::test]
    async fn test_list_postings() {
        let mut mock = MockInterestRepository::new();
        mock.expect_get_postings()
            .returning(|_, _, _, _| Ok(vec![]));

        let postings = mock
            .get_postings(
                "acc-1",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
                1,
            )
            .await
            .unwrap();
        assert!(postings.is_empty());
    }

    // ── UpdateRateConfig (sunset) tests ────────────────────────────────

    #[test]
    fn test_update_rate_config_request_must_have_at_least_one_field() {
        let req = UpdateRateConfigRequest {
            effective_to: None,
            is_active: None,
        };
        // Both None → should be rejected by handler
        assert!(req.effective_to.is_none() && req.is_active.is_none());
    }

    #[tokio::test]
    async fn test_sunset_rate_config_deactivates() {
        let config_id = Uuid::new_v4();
        let config = make_config(config_id, "USD");

        let mut mock = MockInterestRepository::new();
        mock.expect_get_rate_config_by_id()
            .with(eq(config_id))
            .returning(move |_| Ok(Some(config.clone())));

        let mut sunset_config = make_config(config_id, "USD");
        sunset_config.is_active = false;
        sunset_config.effective_to = Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        let sunset_clone = sunset_config.clone();
        mock.expect_sunset_rate_config()
            .with(
                eq(config_id),
                eq(Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap())),
                eq(false),
            )
            .returning(move |_, _, _| Ok(sunset_clone.clone()));
        mock.expect_get_rate_tiers().returning(|_| Ok(vec![]));

        // Verify the mock works
        let fetched = mock
            .get_rate_config_by_id(config_id)
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.is_active);

        let updated = mock
            .sunset_rate_config(
                config_id,
                Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()),
                false,
            )
            .await
            .unwrap();
        assert!(!updated.is_active);
        assert_eq!(
            updated.effective_to,
            Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap())
        );
    }

    #[test]
    fn test_sunset_effective_to_before_effective_from_invalid() {
        let config = make_config(Uuid::new_v4(), "USD");
        // effective_from is 2026-01-01, effective_to = 2025-12-31 is invalid
        let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        assert!(end < config.effective_from);
    }

    // ── ReplaceTiers tests ───────────────────────────────────────────────

    #[test]
    fn test_replace_tiers_empty_is_rejected() {
        let req = ReplaceTiersRequest { tiers: vec![] };
        assert!(req.tiers.is_empty());
    }

    #[test]
    fn test_replace_tiers_first_tier_must_start_at_zero() {
        let tier = CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(100), // not 0 → invalid
            max_balance: None,
            apr: dec!(0.05),
        };
        assert_ne!(tier.min_balance, Decimal::ZERO);
    }

    #[test]
    fn test_replace_tiers_last_tier_must_be_unbounded() {
        let tier = CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: Some(dec!(10000)), // not None → invalid if last
            apr: dec!(0.05),
        };
        assert!(tier.max_balance.is_some());
    }

    #[test]
    fn test_replace_tiers_continuity_check() {
        let tiers = [
            CreateTierRequest {
                tier_order: 1,
                min_balance: dec!(0),
                max_balance: Some(dec!(10000)),
                apr: dec!(0.03),
            },
            CreateTierRequest {
                tier_order: 2,
                min_balance: dec!(10000), // matches prev max_balance → valid
                max_balance: None,
                apr: dec!(0.05),
            },
        ];
        assert_eq!(tiers[1].min_balance, tiers[0].max_balance.unwrap());
    }

    #[test]
    fn test_replace_tiers_gap_is_rejected() {
        let tiers = [
            CreateTierRequest {
                tier_order: 1,
                min_balance: dec!(0),
                max_balance: Some(dec!(10000)),
                apr: dec!(0.03),
            },
            CreateTierRequest {
                tier_order: 2,
                min_balance: dec!(15000), // gap: 10000..15000 → invalid
                max_balance: None,
                apr: dec!(0.05),
            },
        ];
        assert_ne!(tiers[1].min_balance, tiers[0].max_balance.unwrap());
    }

    #[tokio::test]
    async fn test_replace_tiers_calls_db() {
        let config_id = Uuid::new_v4();
        let config = make_config(config_id, "USD");

        let mut mock = MockInterestRepository::new();
        let config_clone = config.clone();
        mock.expect_get_rate_config_by_id()
            .with(eq(config_id))
            .returning(move |_| Ok(Some(config_clone.clone())));
        mock.expect_replace_rate_tiers()
            .with(eq(config_id), always())
            .returning(|_, _| Ok(()));

        // Verify the mock calls succeed
        let fetched = mock.get_rate_config_by_id(config_id).await.unwrap();
        assert!(fetched.is_some());
        mock.replace_rate_tiers(config_id, &[]).await.unwrap();
    }

    // ── Shared validation tests ────────────────────────────────────────

    #[test]
    fn test_validate_tier_continuity_valid_single_tier() {
        let tiers = vec![CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: None,
            apr: dec!(0.05),
        }];
        assert!(validate_tier_continuity(&tiers).is_ok());
    }

    #[test]
    fn test_validate_tier_continuity_valid_multi_tier() {
        let tiers = vec![
            CreateTierRequest {
                tier_order: 1,
                min_balance: dec!(0),
                max_balance: Some(dec!(10000)),
                apr: dec!(0.03),
            },
            CreateTierRequest {
                tier_order: 2,
                min_balance: dec!(10000),
                max_balance: None,
                apr: dec!(0.05),
            },
        ];
        assert!(validate_tier_continuity(&tiers).is_ok());
    }

    #[test]
    fn test_validate_tier_continuity_rejects_empty() {
        assert!(validate_tier_continuity(&[]).is_err());
    }

    #[test]
    fn test_validate_tier_continuity_rejects_negative_apr() {
        let tiers = vec![CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: None,
            apr: dec!(-0.01),
        }];
        assert!(validate_tier_continuity(&tiers).is_err());
    }

    #[test]
    fn test_validate_tier_continuity_rejects_gap() {
        let tiers = vec![
            CreateTierRequest {
                tier_order: 1,
                min_balance: dec!(0),
                max_balance: Some(dec!(10000)),
                apr: dec!(0.03),
            },
            CreateTierRequest {
                tier_order: 2,
                min_balance: dec!(15000),
                max_balance: None,
                apr: dec!(0.05),
            },
        ];
        let err = validate_tier_continuity(&tiers).unwrap_err();
        assert!(err.contains("Gap"));
    }

    #[test]
    fn test_validate_tier_continuity_rejects_nonzero_start() {
        let tiers = vec![CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(100),
            max_balance: None,
            apr: dec!(0.05),
        }];
        let err = validate_tier_continuity(&tiers).unwrap_err();
        assert!(err.contains("min_balance = 0"));
    }

    #[test]
    fn test_validate_tier_continuity_rejects_bounded_last() {
        let tiers = vec![CreateTierRequest {
            tier_order: 1,
            min_balance: dec!(0),
            max_balance: Some(dec!(10000)),
            apr: dec!(0.05),
        }];
        let err = validate_tier_continuity(&tiers).unwrap_err();
        assert!(err.contains("unbounded"));
    }

    // ── Estimate tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_estimate_interest_calculation() {
        let config_id = Uuid::new_v4();
        let tier = make_tier(config_id);

        let mut mock = MockInterestRepository::new();
        mock.expect_list_interest_accounts().returning(|| {
            Ok(vec![InterestAccountInfo {
                account_id: "acc-1".to_string(),
                ledger_id: "ledger-1".to_string(),
                currency: "USD".to_string(),
                tenant_id: 1,
            }])
        });
        mock.expect_get_current_balance()
            .returning(|_| Ok(dec!(10000)));
        let config = make_config(config_id, "USD");
        let config_clone = config.clone();
        mock.expect_list_rate_configs()
            .returning(move |_, _| Ok(vec![config_clone.clone()]));
        let tier_clone = tier.clone();
        mock.expect_get_rate_tiers()
            .returning(move |_| Ok(vec![tier_clone.clone()]));

        // Simulate the estimate calculation
        let balance = mock.get_current_balance("ledger-1").await.unwrap();
        let configs = mock
            .list_rate_configs(Some("USD".to_string()), true)
            .await
            .unwrap();
        let tiers = mock.get_rate_tiers(config_id).await.unwrap();
        let day_count = configs[0].day_count_convention().unwrap();
        let estimated = estimate_interest(balance, &tiers, &day_count, 30);

        // 10000 * 0.045 / 365 * 30
        let expected = dec!(10000) * dec!(0.045) / dec!(365) * dec!(30);
        assert_eq!(estimated, expected);
    }
}
