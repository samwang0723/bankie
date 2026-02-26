use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use uuid::Uuid;

use crate::common::account::generate_bank_account_number;
use crate::domain::models::HouseAccount;

pub struct HouseAccountExtractor(pub HashMap<String, String>, pub HouseAccount);

const USER_AGENT_HDR: &str = "User-Agent";

#[async_trait]
impl<S> FromRequest<S> for HouseAccountExtractor
where
    S: Send + Sync,
{
    type Rejection = HouseAccountExtractionError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mut metadata = HashMap::default();
        metadata.insert("time".to_string(), chrono::Utc::now().to_rfc3339());
        metadata.insert("uri".to_string(), req.uri().to_string());
        if let Some(user_agent) = req.headers().get(USER_AGENT_HDR) {
            if let Ok(value) = user_agent.to_str() {
                metadata.insert(USER_AGENT_HDR.to_string(), value.to_string());
            }
        }

        let body = Bytes::from_request(req, state).await?;
        let mut house_account: HouseAccount = serde_json::from_slice(body.as_ref())?;
        house_account.id = Uuid::new_v4();
        house_account.account_number = generate_bank_account_number(10);
        Ok(HouseAccountExtractor(metadata, house_account))
    }
}

pub struct HouseAccountExtractionError;

impl IntoResponse for HouseAccountExtractionError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            "house_account could not be read".to_string(),
        )
            .into_response()
    }
}

impl From<axum::extract::rejection::BytesRejection> for HouseAccountExtractionError {
    fn from(_: axum::extract::rejection::BytesRejection) -> Self {
        HouseAccountExtractionError
    }
}

impl From<serde_json::Error> for HouseAccountExtractionError {
    fn from(_: serde_json::Error) -> Self {
        HouseAccountExtractionError
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{header::USER_AGENT, Request},
    };

    #[tokio::test]
    async fn test_house_account_extractor() {
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(
                r#"
                {
                    "status": "active",
                    "account_name": "Master USD account",
                    "account_type": "Settlement",
                    "currency": "USD"
                }
                "#,
            ))
            .unwrap();

        let state = ();
        let result = HouseAccountExtractor::from_request(request, &state).await;

        match result {
            Ok(extractor) => {
                let HouseAccountExtractor(metadata, house_account) = extractor;
                assert_eq!(metadata.get("uri").unwrap(), "/test-uri");
                assert_eq!(metadata.get(USER_AGENT_HDR).unwrap(), "test-agent");
                assert_eq!(house_account.account_name, "Master USD account");
                assert_eq!(house_account.account_type, "Settlement");
                assert_eq!(house_account.currency, "USD");
                assert!(house_account.account_number.len() == 10);
            }
            Err(_) => panic!("Extraction failed"),
        }
    }

    #[tokio::test]
    async fn test_house_account_extractor_invalid_body() {
        let request = Request::builder()
            .uri("/test-uri")
            .header(USER_AGENT, "test-agent")
            .body(Body::from(r#"{"invalid": "body"}"#))
            .unwrap();

        let state = ();
        let result = HouseAccountExtractor::from_request(request, &state).await;
        assert!(result.is_err());
    }
}
