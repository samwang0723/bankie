use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::common::error::AppError;
use crate::common::money::Money;
use crate::domain::finance::TransactionWithMoney;
use crate::event_sourcing::command::{BankAccountCommand, LedgerCommand};
use crate::house_account::HouseAccountExtractor;
use crate::SharedState;

use axum::extract::{Extension, Query};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use cqrs_es::persist::ViewRepository;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct HouseAccountParams {
    pub currency: String,
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

pub async fn user_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_user_bank_accounts(id).await {
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
        BankAccountCommand::Deposit { amount, .. } => Some(amount.currency.to_string()),
        BankAccountCommand::Withdrawal { amount, .. } => Some(amount.currency.to_string()),
        BankAccountCommand::Transfer { amount, .. } => Some(amount.currency.to_string()),
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
        BankAccountCommand::FreezeAccount { id } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "status": "frozen"}),
        ),
        BankAccountCommand::UnfreezeAccount { id } => (
            StatusCode::OK,
            json!({"id": id.to_string(), "status": "unfrozen"}),
        ),
        BankAccountCommand::CloseAccount { id } => (
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
    Extension(_tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    Query(params): Query<HouseAccountParams>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_house_accounts(&params.currency).await {
        Ok(accounts) => (StatusCode::OK, Json(json!({ "entries": accounts }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

pub async fn house_account_create_handler(
    Extension(_tenant_id): Extension<i32>,
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
            },
        )
        .await
    {
        return AppError::BadRequest(err.to_string()).into_response();
    }

    house_account.ledger_id = ledger_id.to_string();
    let house_account_id = house_account.id.to_string();
    if let Err(err) = client.create_house_account(house_account).await {
        return AppError::BadRequest(err.to_string()).into_response();
    }

    (StatusCode::CREATED, Json(json!({ "id": house_account_id}))).into_response()
}

pub async fn sub_account_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_sub_accounts(id).await {
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
    Extension(_tenant_id): Extension<i32>,
    Path(account_number): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let client = Arc::clone(&state.database);
    match client.get_bank_account_by_number(account_number).await {
        Ok(account) => (StatusCode::OK, Json(account)).into_response(),
        Err(_) => AppError::NotFound("Account not found".to_string()).into_response(),
    }
}

pub async fn transaction_query_handler(
    Extension(_tenant_id): Extension<i32>,
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
            .get_transactions(params.bank_account_id, offset, limit)
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
    Extension(_tenant_id): Extension<i32>,
    Path(account_id): Path<String>,
    State(state): State<SharedState>,
    Query(params): Query<BalanceHistoryParams>,
) -> Response {
    let client = &state.database.clone();
    match client
        .get_balance_history(account_id, params.start_date, params.end_date)
        .await
    {
        Ok(snapshots) => (StatusCode::OK, Json(json!({ "entries": snapshots }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
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
