use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    UnprocessableEntity(String),
    InternalServerError(String),
}

impl AppError {
    fn code(&self) -> u16 {
        match self {
            AppError::BadRequest(_) => 400,
            AppError::Unauthorized(_) => 401,
            AppError::Forbidden(_) => 403,
            AppError::NotFound(_) => 404,
            AppError::Conflict(_) => 409,
            AppError::UnprocessableEntity(_) => 422,
            AppError::InternalServerError(_) => 500,
        }
    }

    fn message(&self) -> &str {
        match self {
            AppError::BadRequest(msg) => msg,
            AppError::Unauthorized(msg) => msg,
            AppError::Forbidden(msg) => msg,
            AppError::NotFound(msg) => msg,
            AppError::Conflict(msg) => msg,
            AppError::UnprocessableEntity(msg) => msg,
            AppError::InternalServerError(msg) => msg,
        }
    }

    /// Create an InternalServerError that logs the real error but returns a
    /// sanitized message to the client. Prevents leaking internal details.
    pub fn internal(err: impl fmt::Display) -> Self {
        tracing::error!("Internal error: {}", err);
        AppError::InternalServerError("An internal error occurred".to_string())
    }
}

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
    async fn test_unauthorized_error() {
        let error = AppError::Unauthorized("Not authenticated".into());
        assert_eq!(error.code(), 401);
        assert_eq!(error.message(), "Not authenticated");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_forbidden_error() {
        let error = AppError::Forbidden("Access denied".into());
        assert_eq!(error.code(), 403);
        assert_eq!(error.message(), "Access denied");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
    async fn test_conflict_error() {
        let error = AppError::Conflict("Resource already exists".into());
        assert_eq!(error.code(), 409);
        assert_eq!(error.message(), "Resource already exists");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_unprocessable_entity_error() {
        let error = AppError::UnprocessableEntity("Validation failed".into());
        assert_eq!(error.code(), 422);
        assert_eq!(error.message(), "Validation failed");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
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

    #[tokio::test]
    async fn test_internal_helper_sanitizes_message() {
        let error = AppError::internal("SQL error: relation 'foo' does not exist");
        assert_eq!(error.code(), 500);
        assert_eq!(error.message(), "An internal error occurred");
    }
}
