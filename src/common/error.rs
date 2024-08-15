use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Serialize)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    InternalServerError(String),
}

impl AppError {
    fn code(&self) -> u16 {
        match self {
            AppError::BadRequest(_) => 400,
            AppError::NotFound(_) => 404,
            AppError::InternalServerError(_) => 500,
        }
    }

    fn message(&self) -> &str {
        match self {
            AppError::BadRequest(msg) => msg,
            AppError::NotFound(msg) => msg,
            AppError::InternalServerError(msg) => msg,
        }
    }
}

// Step 2: Implement the std::fmt::Display and std::error::Error Traits
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status_code =
            StatusCode::from_u16(self.code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = Json(serde_json::json!({
            "code": self.code(),
            "message": self.message(),
        }));
        (status_code, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::json;

    #[tokio::test]
    async fn test_bad_request_error() {
        let error = AppError::BadRequest("Bad request".into());
        assert_eq!(error.code(), 400);
        assert_eq!(error.message(), "Bad request");

        let response = error.into_response();
        let status = response.status();
        let body = response.into_body();

        assert_eq!(status, StatusCode::BAD_REQUEST);

        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body_json, json!({"code": 400, "message": "Bad request"}));
    }

    #[tokio::test]
    async fn test_not_found_error() {
        let error = AppError::NotFound("Not found".into());
        assert_eq!(error.code(), 404);
        assert_eq!(error.message(), "Not found");

        let response = error.into_response();
        let status = response.status();
        let body = response.into_body();

        assert_eq!(status, StatusCode::NOT_FOUND);

        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body_json, json!({"code": 404, "message": "Not found"}));
    }

    #[tokio::test]
    async fn test_internal_server_error() {
        let error = AppError::InternalServerError("Internal server error".into());
        assert_eq!(error.code(), 500);
        assert_eq!(error.message(), "Internal server error");

        let response = error.into_response();
        let status = response.status();
        let body = response.into_body();

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            body_json,
            json!({"code": 500, "message": "Internal server error"})
        );
    }
}
