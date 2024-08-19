use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::common::error::AppError;
use crate::common::money::Money;
use crate::event_sourcing::command::{BankAccountCommand, LedgerCommand};
use crate::house_account::HouseAccountExtractor;
use crate::SharedState;

use axum::extract::{Extension, Query};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cqrs_es::persist::ViewRepository;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct HouseAccountParams {
    pub currency: String,
}

// Serves as our query endpoint to respond with the materialized `BankAccountView`
// for the requested account.
pub async fn bank_account_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> Response {
    let bank_account = &state.bank_account.clone().unwrap();
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

// Serves as our command endpoint to make changes in a `BankAccount` aggregate.
pub async fn bank_account_command_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    CommandExtractor(_metadata, command): CommandExtractor,
) -> Response {
    let result = match &command {
        BankAccountCommand::OpenAccount { id, .. } => (StatusCode::CREATED, id.to_string()),
        BankAccountCommand::ApproveAccount { id, .. } => (StatusCode::OK, id.to_string()),
        BankAccountCommand::Deposit { id, .. } => (StatusCode::OK, id.to_string()),
        BankAccountCommand::Withdrawal { id, .. } => (StatusCode::OK, id.to_string()),
    };
    if let Some(command_sender) = &state.command_sender {
        match command_sender.send(command) {
            Ok(_) => (result.0, Json(json!({"id": result.1}))).into_response(),
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
    let ledger = &state.ledger.clone().unwrap();
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
    match client.get_house_accounts(params.currency.into()).await {
        Ok(accounts) => (StatusCode::OK, Json(json!({ "entries": accounts }))).into_response(),
        Err(err) => AppError::InternalServerError(err.to_string()).into_response(),
    }
}

pub async fn house_account_create_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<SharedState>,
    HouseAccountExtractor(_metadata, mut house_account): HouseAccountExtractor,
) -> Response {
    let client = &state.database.clone();
    let ledger_id = Uuid::new_v4();
    let ledger = &state.ledger.clone().unwrap();
    if let Err(err) = ledger
        .cqrs
        .execute(
            &ledger_id.to_string(),
            LedgerCommand::Init {
                id: ledger_id,
                account_id: house_account.id,
                amount: Money::new(Decimal::ZERO, house_account.currency),
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
