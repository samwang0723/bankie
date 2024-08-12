use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

use crate::common::account::generate_bank_account_number;
use crate::domain::models::HouseAccount;

// This is a custom Axum extension that builds metadata from the inbound request
// and parses and deserializes the body as the house account payload.
pub struct HouseAccountExtractor(pub HashMap<String, String>, pub HouseAccount);

const USER_AGENT_HDR: &str = "User-Agent";

#[async_trait]
impl<S> FromRequest<S> for HouseAccountExtractor
where
    S: Send + Sync,
{
    type Rejection = HouseAccountExtractionError;

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
        let mut house_account: HouseAccount = serde_json::from_slice(body.as_ref())?;
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
