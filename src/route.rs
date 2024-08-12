use std::str::FromStr;
use std::sync::Arc;

use crate::command::CommandExtractor;
use crate::event_sourcing::command::LedgerCommand;
use crate::house_account::HouseAccountExtractor;
use crate::repository::adapter::DatabaseClient;
use crate::state::ApplicationState;

use axum::extract::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cqrs_es::persist::ViewRepository;
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
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
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
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
    CommandExtractor(metadata, command): CommandExtractor,
) -> Response {
    match state
        .bank_account
        .cqrs
        .execute_with_metadata(&id, command, metadata)
        .await
    {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
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
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

pub async fn house_account_create_handler(
    Extension(tenant_id): Extension<i32>,
    Path(id): Path<String>,
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
                account_id: Uuid::from_str(&id).unwrap(),
            },
        )
        .await
    {
        Ok(_) => {
            let mut house_account = house_account;
            house_account.ledger_id = ledger_id.to_string();

            match client.create_house_account(house_account).await {
                Ok(_) => StatusCode::CREATED.into_response(),
                Err(err) => {
                    error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
                    (StatusCode::BAD_REQUEST, err.to_string()).into_response()
                }
            }
        }
        Err(err) => {
            error!("Error: {:#?}, with tenant_id: {}\n", err, tenant_id);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}
