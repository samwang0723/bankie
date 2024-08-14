use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::common::money::Money;
use crate::event_sourcing::command::{BankAccountCommand, LedgerCommand};
use crate::house_account::HouseAccountExtractor;
use crate::repository::adapter::DatabaseClient;
use crate::state::ApplicationState;

use axum::extract::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cqrs_es::persist::ViewRepository;
use rust_decimal::Decimal;
use serde_json::json;
use tracing::error;
use uuid::Uuid;

// Serves as our query endpoint to respond with the materialized `BankAccountView`
// for the requested account.
pub async fn bank_account_query_handler(
    Extension(tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.bank_account.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": err.to_string()})),
            )
                .into_response();
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

// Serves as our command endpoint to make changes in a `BankAccount` aggregate.
pub async fn bank_account_command_handler(
    Extension(tenant_id): Extension<i32>,
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
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": err.to_string()})),
            )
                .into_response()
        }
    }
}

pub async fn ledger_query_handler(
    Extension(tenant_id): Extension<i32>,
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.ledger.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": err.to_string()})),
            )
                .into_response();
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

pub async fn house_account_create_handler(
    Extension(tenant_id): Extension<i32>,
    State(state): State<ApplicationState>,
    HouseAccountExtractor(_metadata, house_account): HouseAccountExtractor,
) -> Response {
    let client = Arc::clone(&state.pool);
    let ledger_id = Uuid::new_v4();
    match state
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
        Ok(_) => {
            let mut house_account = house_account;
            house_account.ledger_id = ledger_id.to_string();

            let house_account_id = house_account.id.to_string();
            match client.create_house_account(house_account).await {
                Ok(_) => {
                    (StatusCode::CREATED, Json(json!({ "id": house_account_id}))).into_response()
                }
                Err(err) => {
                    error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": err.to_string()})),
                    )
                        .into_response()
                }
            }
        }
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": err.to_string()})),
            )
                .into_response()
        }
    }
}
