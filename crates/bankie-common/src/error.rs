use axum::{
    http::{header, StatusCode},
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
    TooManyRequests(String, u64),
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
            AppError::TooManyRequests(_, _) => 429,
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
            AppError::TooManyRequests(msg, _) => msg,
            AppError::UnprocessableEntity(msg) => msg,
            AppError::InternalServerError(msg) => msg,
        }
    }

    /// Create an InternalServerError that logs the real error but returns a
    /// sanitized message to the client. Prevents leaking internal details.
    #[allow(dead_code)]
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

        let retry_after = if let AppError::TooManyRequests(_, secs) = &self {
            Some(*secs)
        } else {
            None
        };

        let body = if let Some(secs) = retry_after {
            Json(serde_json::json!({
                "code": self.code(),
                "message": self.message(),
                "retry_after": secs,
            }))
        } else {
            Json(serde_json::json!({
                "code": self.code(),
                "message": self.message(),
            }))
        };

        let mut response = (status_code, body).into_response();
        if let Some(secs) = retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, secs.to_string().parse().unwrap());
        }
        response
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
    async fn test_too_many_requests_error() {
        let error = AppError::TooManyRequests("Rate limit exceeded".into(), 120);
        assert_eq!(error.code(), 429);
        assert_eq!(error.message(), "Rate limit exceeded");

        let response = error.into_response();
        let status = response.status();
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        // Verify Retry-After header
        let retry_after = response.headers().get("retry-after").unwrap();
        assert_eq!(retry_after.to_str().unwrap(), "120");

        // Verify body includes retry_after
        let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            body_json,
            json!({"code": 429, "message": "Rate limit exceeded", "retry_after": 120})
        );
    }

    #[tokio::test]
    async fn test_internal_helper_sanitizes_message() {
        let error = AppError::internal("SQL error: relation 'foo' does not exist");
        assert_eq!(error.code(), 500);
        assert_eq!(error.message(), "An internal error occurred");
    }
}
