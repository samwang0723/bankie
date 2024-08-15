use std::error::Error;
use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::common::money::Money;
use crate::event_sourcing::command::{BankAccountCommand, LedgerCommand};
use crate::house_account::HouseAccountExtractor;
use crate::repository::adapter::DatabaseClient;
use crate::state::ApplicationState;

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
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.bank_account.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            return handle_error(err, StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

// Serves as our command endpoint to make changes in a `BankAccount` aggregate.
pub async fn bank_account_command_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<ApplicationState>,
    CommandExtractor(metadata, command): CommandExtractor,
) -> Response {
    let result = match &command {
        BankAccountCommand::OpenAccount { id, .. } => (StatusCode::CREATED, id.to_string()),
        BankAccountCommand::ApproveAccount { id, .. } => (StatusCode::OK, id.to_string()),
        BankAccountCommand::Deposit { id, .. } => (StatusCode::OK, id.to_string()),
        BankAccountCommand::Withdrawl { id, .. } => (StatusCode::OK, id.to_string()),
    };
    match state
        .bank_account
        .cqrs
        .execute_with_metadata(&result.1, command, metadata)
        .await
    {
        Ok(_) => (result.0, Json(json!({"id": result.1}))).into_response(),
        Err(err) => handle_error(err, StatusCode::BAD_REQUEST),
    }
}

pub async fn ledger_query_handler(
    Extension(_tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.ledger.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            return handle_error(err, StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

pub async fn house_account_query_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<ApplicationState>,
    Query(params): Query<HouseAccountParams>,
) -> Response {
    let client = Arc::clone(&state.pool);
    match client.get_house_accounts(params.currency.into()).await {
        Ok(accounts) => (StatusCode::OK, Json(accounts)).into_response(),
        Err(err) => handle_error(err, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn house_account_create_handler(
    Extension(_tenant_id): Extension<i32>,
    State(state): State<ApplicationState>,
    HouseAccountExtractor(_metadata, mut house_account): HouseAccountExtractor,
) -> Response {
    let client = Arc::clone(&state.pool);
    let ledger_id = Uuid::new_v4();

    if let Err(err) = state
        .ledger
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
        return handle_error(err, StatusCode::BAD_REQUEST);
    }

    house_account.ledger_id = ledger_id.to_string();
    let house_account_id = house_account.id.to_string();
    if let Err(err) = client.create_house_account(house_account).await {
        return handle_error(err, StatusCode::BAD_REQUEST);
    }

    (StatusCode::CREATED, Json(json!({ "id": house_account_id}))).into_response()
}

fn handle_error(err: impl Error, status: StatusCode) -> Response {
    (status, Json(json!({ "error": err.to_string()}))).into_response()
}
