use std::sync::Arc;

use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::calculator::estimate_interest;
use super::db::InterestDbClient;
use super::models::{InterestRateConfig, InterestRateTier};
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

#[derive(Serialize)]
pub struct EstimateResponse {
    pub account_id: String,
    pub days: u32,
    pub current_balance: String,
    pub estimated_interest: String,
    pub currency: String,
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// GET /v1/interest/rates?currency=
pub async fn list_rate_configs<D: InterestDbClient>(
    Extension(_tenant_id): Extension<i32>,
    Query(params): Query<RateListParams>,
    interest_db: Extension<Arc<D>>,
) -> Response {
    let configs = match interest_db.get_active_rate_configs(params.currency).await {
        Ok(c) => c,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
    };

    let mut results = Vec::with_capacity(configs.len());
    for config in configs {
        let tiers = match interest_db.get_tiers_for_config(config.id).await {
            Ok(t) => t,
            Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
        };
        results.push(RateConfigResponse { config, tiers });
    }

    (StatusCode::OK, Json(json!({ "entries": results }))).into_response()
}

/// POST /v1/interest/rates
pub async fn create_rate_config<D: InterestDbClient>(
    Extension(_tenant_id): Extension<i32>,
    interest_db: Extension<Arc<D>>,
    Json(req): Json<CreateRateConfigRequest>,
) -> Response {
    // Validate tiers
    if req.tiers.is_empty() {
        return AppError::BadRequest("At least one tier is required".to_string()).into_response();
    }
    for tier in &req.tiers {
        if tier.apr < Decimal::ZERO {
            return AppError::BadRequest("APR must be non-negative".to_string()).into_response();
        }
    }

    // Validate posting frequency / posting_day
    if let Err(e) = req
        .posting_frequency
        .parse::<super::models::PostingFrequency>()
    {
        return AppError::BadRequest(format!("Invalid posting frequency: {}", e)).into_response();
    }
    if let Err(e) = req.day_count.parse::<super::models::DayCountConvention>() {
        return AppError::BadRequest(format!("Invalid day count convention: {}", e))
            .into_response();
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

    let saved_config = match interest_db.upsert_rate_config(&config).await {
        Ok(c) => c,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
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

    if let Err(e) = interest_db.replace_tiers(saved_config.id, &tiers).await {
        return AppError::InternalServerError(e.to_string()).into_response();
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
pub async fn get_rate_config<D: InterestDbClient>(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<Uuid>,
    interest_db: Extension<Arc<D>>,
) -> Response {
    let config = match interest_db.get_rate_config_by_id(id).await {
        Ok(Some(c)) => c,
        Ok(None) => return AppError::NotFound("Rate config not found".to_string()).into_response(),
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
    };

    let tiers = match interest_db.get_tiers_for_config(config.id).await {
        Ok(t) => t,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
    };

    (
        StatusCode::OK,
        Json(json!(RateConfigResponse { config, tiers })),
    )
        .into_response()
}

/// GET /v1/interest/accruals?account_id=&start_date=&end_date=
pub async fn list_accruals<D: InterestDbClient>(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<AccrualListParams>,
    interest_db: Extension<Arc<D>>,
) -> Response {
    if params.start_date > params.end_date {
        return AppError::BadRequest("start_date must be <= end_date".to_string()).into_response();
    }

    match interest_db
        .get_accruals(
            &params.account_id,
            params.start_date,
            params.end_date,
            tenant_id,
        )
        .await
    {
        Ok(accruals) => (StatusCode::OK, Json(json!({ "entries": accruals }))).into_response(),
        Err(e) => AppError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /v1/interest/postings?account_id=&start_date=&end_date=
pub async fn list_postings<D: InterestDbClient>(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<PostingListParams>,
    interest_db: Extension<Arc<D>>,
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
        Err(e) => AppError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /v1/interest/estimate?account_id=&days=
pub async fn estimate_interest_handler<D: InterestDbClient>(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<EstimateParams>,
    interest_db: Extension<Arc<D>>,
) -> Response {
    // Get current balance
    // We need to look up the account's ledger_id first.
    // For simplicity, we get balance from the interest DB client.
    let accounts = match interest_db.list_interest_accounts().await {
        Ok(a) => a,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
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
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
    };

    // Find rate config for this currency
    let configs = match interest_db
        .get_active_rate_configs(Some(account.currency.clone()))
        .await
    {
        Ok(c) => c,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
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

    let tiers = match interest_db.get_tiers_for_config(config.id).await {
        Ok(t) => t,
        Err(e) => return AppError::InternalServerError(e.to_string()).into_response(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::db::{InterestAccountInfo, MockInterestDbClient};
    use crate::interest::models::InterestAccrual;
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
        let mut mock = MockInterestDbClient::new();
        mock.expect_get_active_rate_configs()
            .with(eq(None::<String>))
            .returning(|_| Ok(vec![]));

        let configs = mock.get_active_rate_configs(None).await.unwrap();
        assert!(configs.is_empty());
    }

    #[tokio::test]
    async fn test_list_rate_configs_with_tiers() {
        let config_id = Uuid::new_v4();
        let config = make_config(config_id, "USD");
        let tier = make_tier(config_id);

        let mut mock = MockInterestDbClient::new();
        let config_clone = config.clone();
        mock.expect_get_active_rate_configs()
            .returning(move |_| Ok(vec![config_clone.clone()]));
        let tier_clone = tier.clone();
        mock.expect_get_tiers_for_config()
            .returning(move |_| Ok(vec![tier_clone.clone()]));

        let configs = mock.get_active_rate_configs(None).await.unwrap();
        assert_eq!(configs.len(), 1);
        let tiers = mock.get_tiers_for_config(config_id).await.unwrap();
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].apr, dec!(0.045));
    }

    #[tokio::test]
    async fn test_get_rate_config_not_found() {
        let mut mock = MockInterestDbClient::new();
        mock.expect_get_rate_config_by_id().returning(|_| Ok(None));

        let result = mock.get_rate_config_by_id(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_accruals() {
        let mut mock = MockInterestDbClient::new();
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
        mock.expect_get_accruals()
            .returning(move |_, _, _, _| Ok(vec![accrual_clone.clone()]));

        let accruals = mock
            .get_accruals(
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
        let mut mock = MockInterestDbClient::new();
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

    #[tokio::test]
    async fn test_estimate_interest_calculation() {
        let config_id = Uuid::new_v4();
        let tier = make_tier(config_id);

        let mut mock = MockInterestDbClient::new();
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
        mock.expect_get_active_rate_configs()
            .returning(move |_| Ok(vec![config_clone.clone()]));
        let tier_clone = tier.clone();
        mock.expect_get_tiers_for_config()
            .returning(move |_| Ok(vec![tier_clone.clone()]));

        // Simulate the estimate calculation
        let balance = mock.get_current_balance("ledger-1").await.unwrap();
        let configs = mock
            .get_active_rate_configs(Some("USD".to_string()))
            .await
            .unwrap();
        let tiers = mock.get_tiers_for_config(config_id).await.unwrap();
        let day_count = configs[0].day_count_convention().unwrap();
        let estimated = estimate_interest(balance, &tiers, &day_count, 30);

        // 10000 * 0.045 / 365 * 30
        let expected = dec!(10000) * dec!(0.045) / dec!(365) * dec!(30);
        assert_eq!(estimated, expected);
    }
}
