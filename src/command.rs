use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use uuid::Uuid;

use crate::event_sourcing::command::BankAccountCommand;

// This is a custom Axum extension that builds metadata from the inbound request
// and parses and deserializes the body as the command payload.
pub struct CommandExtractor(pub HashMap<String, String>, pub BankAccountCommand);

const USER_AGENT_HDR: &str = "User-Agent";

#[async_trait]
impl<S> FromRequest<S> for CommandExtractor
where
    S: Send + Sync,
{
    type Rejection = CommandExtractionError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Here we are including the current date/time, the uri that was called and the user-agent
        // in a HashMap that we will submit as metadata with the command.
        let mut metadata = HashMap::default();
        metadata.insert("time".to_string(), chrono::Utc::now().to_rfc3339());
        metadata.insert("uri".to_string(), req.uri().to_string());
        if let Some(user_agent) = req.headers().get(USER_AGENT_HDR) {
            if let Ok(value) = user_agent.to_str() {
                metadata.insert(USER_AGENT_HDR.to_string(), value.to_string());
            }
        }

        // Parse and deserialize the request body as the command payload.
        let body = Bytes::from_request(req, state).await?;
        let mut command: BankAccountCommand = serde_json::from_slice(body.as_ref())?;

        // Generate ledger_id instead of bringing in from external
        if let BankAccountCommand::ApproveAccount { id: _, ledger_id } = &mut command {
            *ledger_id = Uuid::new_v4();
        }
        // Generate account id instead of bringing in from external
        if let BankAccountCommand::OpenAccount { id, .. } = &mut command {
            *id = Uuid::new_v4();
            // TODO: Check parent_id if user already had Checking account in same currency
        }
        Ok(CommandExtractor(metadata, command))
    }
}

pub struct CommandExtractionError;

impl IntoResponse for CommandExtractionError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            "command could not be read".to_string(),
        )
            .into_response()
    }
}

impl From<axum::extract::rejection::BytesRejection> for CommandExtractionError {
    fn from(_: axum::extract::rejection::BytesRejection) -> Self {
        CommandExtractionError
    }
}

impl From<serde_json::Error> for CommandExtractionError {
    fn from(_: serde_json::Error) -> Self {
        CommandExtractionError
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        common::money::Currency,
        domain::models::{BankAccountKind, BankAccountType},
    };

    use super::*;
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{header::USER_AGENT, Request},
    };
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_open_account_extractor() {
        // Create a mock request
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(
                r#"
                {
                    "OpenAccount": {
                        "account_type": "Retail",
                        "kind": "Interest",
                        "currency": "TWD",
                        "user_id": "b9aa777c-0868-48ac-9c49-eff869b437d7"
                    }
                }
                "#,
            ))
            .unwrap();

        // Mock state
        let state = ();

        // Call the from_request method
        let result = CommandExtractor::from_request(request, &state).await;

        // Verify the result
        match result {
            Ok(extractor) => {
                let CommandExtractor(metadata, command) = extractor;

                // Check metadata
                assert_eq!(metadata.get("uri").unwrap(), "/test-uri");
                assert_eq!(metadata.get(USER_AGENT_HDR).unwrap(), "test-agent");

                // Check fields
                if let BankAccountCommand::OpenAccount {
                    id,
                    parent_id,
                    account_type,
                    kind,
                    user_id,
                    currency,
                } = command
                {
                    assert!(!id.is_nil());
                    assert!(parent_id.is_none());
                    assert_eq!(account_type, BankAccountType::Retail);
                    assert_eq!(currency, Currency::TWD);
                    assert_eq!(user_id, "b9aa777c-0868-48ac-9c49-eff869b437d7".to_string());
                    assert_eq!(kind, BankAccountKind::Interest);
                } else {
                    panic!("Invalid command");
                }
            }
            Err(_) => panic!("Extraction failed"),
        }
    }

    #[tokio::test]
    async fn test_approve_account_extractor() {
        // Create a mock request
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(
                r#"
                {
                    "ApproveAccount": {
                        "id": "b9aa777c-0868-48ac-9c49-eff869b437d7"
                    }
                }
                "#,
            ))
            .unwrap();

        // Mock state
        let state = ();

        // Call the from_request method
        let result = CommandExtractor::from_request(request, &state).await;

        // Verify the result
        match result {
            Ok(extractor) => {
                let CommandExtractor(metadata, command) = extractor;

                // Check metadata
                assert_eq!(metadata.get("uri").unwrap(), "/test-uri");
                assert_eq!(metadata.get(USER_AGENT_HDR).unwrap(), "test-agent");

                // Check fields
                if let BankAccountCommand::ApproveAccount { id, ledger_id } = command {
                    assert_eq!(
                        id,
                        Uuid::parse_str("b9aa777c-0868-48ac-9c49-eff869b437d7").unwrap()
                    );
                    assert!(!ledger_id.is_nil());
                } else {
                    panic!("Invalid command");
                }
            }
            Err(_) => panic!("Extraction failed"),
        }
    }

    #[tokio::test]
    async fn test_deposit_extractor() {
        // Create a mock request
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(
                r#"
                {
                    "Deposit": {
                        "id": "b9aa777c-0868-48ac-9c49-eff869b437d7",
                        "amount": {
                            "currency": "USD",
                            "amount": 100
                        }
                    }
                }
                "#,
            ))
            .unwrap();

        // Mock state
        let state = ();

        // Call the from_request method
        let result = CommandExtractor::from_request(request, &state).await;

        // Verify the result
        match result {
            Ok(extractor) => {
                let CommandExtractor(metadata, command) = extractor;

                // Check metadata
                assert_eq!(metadata.get("uri").unwrap(), "/test-uri");
                assert_eq!(metadata.get(USER_AGENT_HDR).unwrap(), "test-agent");

                // Check fields
                if let BankAccountCommand::Deposit { id, amount } = command {
                    assert_eq!(
                        id,
                        Uuid::parse_str("b9aa777c-0868-48ac-9c49-eff869b437d7").unwrap()
                    );
                    assert_eq!(amount.currency, Currency::USD);
                    assert_eq!(amount.amount, dec!(100));
                } else {
                    panic!("Invalid command");
                }
            }
            Err(_) => panic!("Extraction failed"),
        }
    }

    #[tokio::test]
    async fn test_withdrawal_extractor() {
        // Create a mock request
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(
                r#"
                {
                    "Withdrawal": {
                        "id": "b9aa777c-0868-48ac-9c49-eff869b437d7",
                        "amount": {
                            "currency": "USD",
                            "amount": 100
                        }
                    }
                }
                "#,
            ))
            .unwrap();

        // Mock state
        let state = ();

        // Call the from_request method
        let result = CommandExtractor::from_request(request, &state).await;

        // Verify the result
        match result {
            Ok(extractor) => {
                let CommandExtractor(metadata, command) = extractor;

                // Check metadata
                assert_eq!(metadata.get("uri").unwrap(), "/test-uri");
                assert_eq!(metadata.get(USER_AGENT_HDR).unwrap(), "test-agent");

                // Check fields
                if let BankAccountCommand::Withdrawal { id, amount } = command {
                    assert_eq!(
                        id,
                        Uuid::parse_str("b9aa777c-0868-48ac-9c49-eff869b437d7").unwrap()
                    );
                    assert_eq!(amount.currency, Currency::USD);
                    assert_eq!(amount.amount, dec!(100));
                } else {
                    panic!("Invalid command");
                }
            }
            Err(_) => panic!("Extraction failed"),
        }
    }

    #[tokio::test]
    async fn test_command_extractor_invalid_body() {
        // Create a mock request with invalid body
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(r#"{"invalid": "body"}"#))
            .unwrap();

        // Mock state
        let state = ();

        // Call the from_request method
        let result = CommandExtractor::from_request(request, &state).await;

        // Verify the result
        assert!(result.is_err());
    }
}
