use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::common::error::AppError;
use crate::common::money::Money;
use crate::domain::finance::TransactionWithMoney;
use crate::event_sourcing::command::{BankAccountCommand, LedgerCommand};
use crate::house_account::HouseAccountExtractor;
use crate::report;
use crate::SharedState;

use axum::extract::{Extension, Query};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDate, Utc};
use cqrs_es::persist::ViewRepository;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct HouseAccountParams {
    pub currency: Option<String>,
}

#[derive(Deserialize)]
pub struct TransactionParams {
    pub bank_account_id: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub transaction_type: Option<String>,
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Deserialize)]
pub struct BalanceHistoryParams {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

#[derive(Deserialize)]
pub struct AccountListParams {
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

pub async fn accounts_query_handler(
    Extension(tenant_id): Extension<i32>,
    Query(params): Query<AccountListParams>,
    State(state): State<SharedState>,
) -> Response {
    let limit = params.limit.min(100);
    let client = Arc::clone(&state.database);
    let accounts = client.get_accounts(params.offset, limit, tenant_id).await;
    let total = client.count_accounts(tenant_id).await;

    match (accounts, total) {
        (Ok(entries), Ok(count)) => (
            StatusCode::OK,
            Json(json!({
                "entries": entries,
                "pagination": {
                    "total": count,
                    "offset": params.offset,
                    "limit": limit
                }
            })),
        )
            .into_response(),
        (Err(err), _) | (_, Err(err)) => {
            AppError::InternalServerError(err.to_string()).into_response()
        }
    }
}

pub async fn user_query_handler(
    Extension(tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_user_bank_accounts(id, tenant_id).await {
        Ok(accounts) => (StatusCode::OK, Json(json!({ "entries": accounts }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

pub async fn bank_account_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let bank_account = match &state.bank_account {
        Some(ba) => ba,
        None => {
            return AppError::InternalServerError("Bank account not configured".to_string())
                .into_response()
        }
    };
    let view = match bank_account.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            return AppError::InternalServerError(err.to_string()).into_response();
        }
    };
    match view {
        None => AppError::NotFound("Resource Not Found".to_string()).into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

pub async fn bank_account_command_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    CommandExtractor(_metadata, command): CommandExtractor,
) -> Response {
    // Validate currency/asset_code against AssetRegistry
    let asset_code = match &command {
        BankAccountCommand::OpenAccount { currency, .. } => Some(currency.to_string()),
        BankAccountCommand::Deposit { amount, .. } => Some(amount.asset_code()),
        BankAccountCommand::Withdrawal { amount, .. } => Some(amount.asset_code()),
        BankAccountCommand::Transfer { amount, .. } => Some(amount.asset_code()),
        BankAccountCommand::ApproveAccount { .. }
        | BankAccountCommand::FreezeAccount { .. }
        | BankAccountCommand::UnfreezeAccount { .. }
        | BankAccountCommand::CloseAccount { .. } => None,
    };
    if let Some(ref code) = asset_code {
        if !state.asset_registry.validate_asset_code(code).await {
            return AppError::BadRequest(format!("Unsupported asset code: {}", code))
                .into_response();
        }
    }

    let result = match &command {
        BankAccountCommand::OpenAccount {
            id, account_number, ..
        } => (
            StatusCode::CREATED,
            json!({"id": id.to_string(), "account_number": account_number}),
        ),
        BankAccountCommand::ApproveAccount { id, .. } => {
            (StatusCode::OK, json!({"id": id.to_string()}))
        }
        BankAccountCommand::FreezeAccount { id, .. } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "status": "frozen"}),
        ),
        BankAccountCommand::UnfreezeAccount { id, .. } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "status": "unfrozen"}),
        ),
        BankAccountCommand::CloseAccount { id, .. } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "status": "closed"}),
        ),
        BankAccountCommand::Deposit { id, .. } => (StatusCode::OK, json!({"id": id.to_string()})),
        BankAccountCommand::Withdrawal { id, .. } => {
            (StatusCode::OK, json!({"id": id.to_string()}))
        }
        BankAccountCommand::Transfer {
            id, to_account_id, ..
        } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "to_account_id": to_account_id.to_string()}),
        ),
    };
    if let Some(command_sender) = &state.command_sender {
        match command_sender.send(command).await {
            Ok(_) => (result.0, Json(result.1)).into_response(),
            Err(err) => AppError::BadRequest(err.to_string()).into_response(),
        }
    } else {
        AppError::InternalServerError("Command Sender not found".to_string()).into_response()
    }
}

pub async fn ledger_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let ledger = match &state.ledger {
        Some(l) => l,
        None => {
            return AppError::InternalServerError("Ledger not configured".to_string())
                .into_response()
        }
    };
    let view = match ledger.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            return AppError::InternalServerError(err.to_string()).into_response();
        }
    };
    match view {
        None => AppError::NotFound("Resource Not Found".to_string()).into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

pub async fn house_account_query_handler(
    Extension(tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    Query(params): Query<HouseAccountParams>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_house_accounts(params.currency, tenant_id).await {
        Ok(accounts) => (StatusCode::OK, Json(json!({ "entries": accounts }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

pub async fn house_account_create_handler(
    Extension(tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    HouseAccountExtractor(_metadata, mut house_account): HouseAccountExtractor,
) -> Response {
    // Validate currency against AssetRegistry
    if !state
        .asset_registry
        .validate_asset_code(&house_account.currency)
        .await
    {
        return AppError::BadRequest(format!(
            "Unsupported asset code: {}",
            house_account.currency
        ))
        .into_response();
    }

    let client = &state.database.clone();
    let ledger_id = Uuid::new_v4();
    let ledger = match &state.ledger {
        Some(l) => l,
        None => {
            return AppError::InternalServerError("Ledger not configured".to_string())
                .into_response()
        }
    };

    let currency = house_account.currency.parse().unwrap_or_default();
    if let Err(err) = ledger
        .cqrs
        .execute(
            &ledger_id.to_string(),
            LedgerCommand::Init {
                id: ledger_id,
                account_id: house_account.id,
                amount: Money::new(Decimal::ZERO, currency),
                tenant_id,
            },
        )
        .await
    {
        return AppError::BadRequest(err.to_string()).into_response();
    }

    house_account.ledger_id = ledger_id.to_string();
    house_account.tenant_id = tenant_id;
    let house_account_id = house_account.id.to_string();
    if let Err(err) = client.create_house_account(house_account).await {
        return AppError::BadRequest(err.to_string()).into_response();
    }

    (StatusCode::CREATED, Json(json!({ "id": house_account_id}))).into_response()
}

pub async fn sub_account_query_handler(
    Extension(tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_sub_accounts(id, tenant_id).await {
        Ok(accounts) => {
            // Separate master from sub-accounts
            let (master, sub_accounts): (Vec<_>, Vec<_>) = accounts
                .into_iter()
                .partition(|a| a.parent_id.is_none() || a.parent_id.as_deref() == Some(""));
            let master = master.into_iter().next();
            (
                StatusCode::OK,
                Json(json!({
                    "master": master,
                    "sub_accounts": sub_accounts
                })),
            )
                .into_response()
        }
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

pub async fn bank_account_by_number_handler(
    Extension(tenant_id): Extension<i32>,
    Path(account_number): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client
        .get_bank_account_by_number(account_number, tenant_id)
        .await
    {
        Ok(account) => (StatusCode::OK, Json(account)).into_response(),
        Err(_) => AppError::NotFound("Account not found".to_string()).into_response(),
    }
}

pub async fn transaction_query_handler(
    Extension(tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    Query(params): Query<TransactionParams>,
) -> Response {
    let client = &state.database.clone();

    // Clamp limit to max 100
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    let has_filters = params.start_date.is_some()
        || params.end_date.is_some()
        || params.transaction_type.is_some()
        || params.status.is_some();

    if has_filters {
        // Use filtered query with count for pagination
        let account_id = params.bank_account_id.clone();
        let count_result = client
            .count_transactions_filtered(
                params.bank_account_id.clone(),
                params.start_date,
                params.end_date,
                params.transaction_type.clone(),
                params.status.clone(),
                tenant_id,
            )
            .await;

        let total = match count_result {
            Ok(c) => c,
            Err(err) => return AppError::InternalServerError(err.to_string()).into_response(),
        };

        match client
            .get_transactions_filtered(
                account_id,
                offset,
                limit,
                params.start_date,
                params.end_date,
                params.transaction_type,
                params.status,
                tenant_id,
            )
            .await
        {
            Ok(transactions) => {
                let transactions: Vec<TransactionWithMoney> = transactions
                    .into_iter()
                    .map(|t| t.into_transaction_with_money())
                    .collect();
                (
                    StatusCode::OK,
                    Json(json!({
                        "entries": transactions,
                        "pagination": {
                            "total": total,
                            "offset": offset,
                            "limit": limit,
                        }
                    })),
                )
                    .into_response()
            }
            Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
        }
    } else {
        // Use simple query (backward compatible)
        match client
            .get_transactions(params.bank_account_id, offset, limit, tenant_id)
            .await
        {
            Ok(transactions) => {
                let transactions: Vec<TransactionWithMoney> = transactions
                    .into_iter()
                    .map(|t| t.into_transaction_with_money())
                    .collect();
                (StatusCode::OK, Json(json!({ "entries": transactions }))).into_response()
            }
            Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
        }
    }
}

pub async fn balance_history_handler(
    Extension(tenant_id): Extension<i32>,
    Path(account_id): Path<String>,
    State(state): State<SharedState>,
    Query(params): Query<BalanceHistoryParams>,
) -> Response {
    let client = &state.database.clone();
    match client
        .get_balance_history(account_id, params.start_date, params.end_date, tenant_id)
        .await
    {
        Ok(snapshots) => (StatusCode::OK, Json(json!({ "entries": snapshots }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct SettlementReportParams {
    pub bank_account_id: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub currency: Option<String>,
}

pub async fn settlement_report_handler(
    Extension(tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    Query(params): Query<SettlementReportParams>,
) -> Response {
    // Validate date range: start <= end
    if params.start_date > params.end_date {
        return AppError::BadRequest("start_date must be before or equal to end_date".to_string())
            .into_response();
    }

    // Validate max date range (90 days)
    let days = (params.end_date - params.start_date).num_days();
    if days > report::MAX_REPORT_DAYS {
        return AppError::BadRequest(format!(
            "Date range exceeds maximum of {} days",
            report::MAX_REPORT_DAYS
        ))
        .into_response();
    }

    let client = &state.database.clone();

    // Resolve account IDs: specific account or all accounts for tenant
    let account_ids: Vec<String> = if let Some(ref id) = params.bank_account_id {
        vec![id.clone()]
    } else {
        match client.get_accounts(0, 100, tenant_id).await {
            Ok(accounts) => accounts.into_iter().filter_map(|a| a.id).collect(),
            Err(err) => return AppError::InternalServerError(err.to_string()).into_response(),
        }
    };

    if account_ids.is_empty() {
        return AppError::BadRequest("No accounts found for this tenant".to_string())
            .into_response();
    }

    let generated_at = Utc::now();
    let mut full_csv = String::new();

    for (idx, account_id) in account_ids.iter().enumerate() {
        // Fetch opening balance
        let opening_balance = match client
            .get_opening_balance(account_id.clone(), params.start_date, tenant_id)
            .await
        {
            Ok(Some(balance)) => balance,
            Ok(None) => Decimal::ZERO,
            Err(err) => return AppError::InternalServerError(err.to_string()).into_response(),
        };

        // Fetch settlement report data
        let rows = match client
            .get_settlement_report_data(
                account_id.clone(),
                params.start_date,
                params.end_date,
                tenant_id,
            )
            .await
        {
            Ok(rows) => rows,
            Err(err) => return AppError::InternalServerError(err.to_string()).into_response(),
        };

        // Determine currency from first row or params
        let currency = params
            .currency
            .as_deref()
            .or_else(|| rows.first().map(|r| r.currency.as_str()))
            .unwrap_or("USD");

        // Determine account number from first row
        let account_number = rows
            .first()
            .and_then(|r| r.account_number.as_deref())
            .unwrap_or("unknown");

        let csv = report::generate_settlement_csv(
            &rows,
            opening_balance,
            account_number,
            params.start_date,
            params.end_date,
            currency,
            generated_at,
        );

        if idx == 0 {
            full_csv.push_str(&csv);
        } else {
            // Skip BOM for subsequent accounts, add separator
            let csv_no_bom = csv.trim_start_matches(report::UTF8_BOM);
            full_csv.push_str("\n");
            full_csv.push_str(csv_no_bom);
        }
    }

    let filename = if params.bank_account_id.is_some() {
        format!(
            "settlement_{}_{}_to_{}.csv",
            account_ids[0], params.start_date, params.end_date
        )
    } else {
        format!(
            "settlement_all_{}_to_{}.csv",
            params.start_date, params.end_date
        )
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        full_csv,
    )
        .into_response()
}

pub async fn health_check_handler() -> StatusCode {
    StatusCode::OK
}

pub async fn readiness_check_handler(State(state): State<SharedState>) -> Response {
    // Check DB connectivity by verifying bank_account loader exists
    if state.bank_account.is_none() || state.ledger.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "not_ready"})),
        )
            .into_response();
    }
    // Check Redis connectivity
    if state.cache.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "no_cache"})),
        )
            .into_response();
    }
    (StatusCode::OK, Json(json!({"status": "ready"}))).into_response()
}
