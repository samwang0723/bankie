use crate::command::CommandExtractor;
use crate::state::ApplicationState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cqrs_es::persist::ViewRepository;

// Serves as our query endpoint to respond with the materialized `BankAccountView`
// for the requested account.
pub async fn bank_account_query_handler(
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.bank_account.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            println!("Error: {:#?}\n", err);
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
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

pub async fn ledger_query_handler(
    Path(id): Path<String>,
    State(state): State<ApplicationState>,
) -> Response {
    let view = match state.ledger.query.load(&id).await {
        Ok(view) => view,
        Err(err) => {
            println!("Error: {:#?}\n", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}
