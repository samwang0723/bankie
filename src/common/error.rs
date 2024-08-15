use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Serialize)]
pub enum AppError {
    BadRequest,
    NotFound,
    InternalServerError,
}

impl AppError {
    fn code(&self) -> u16 {
        match self {
            AppError::BadRequest => 400,
            AppError::NotFound => 404,
            AppError::InternalServerError => 500,
        }
    }

    fn message(&self) -> &str {
        match self {
            AppError::BadRequest => "Bad Request",
            AppError::NotFound => "Resource Not Found",
            AppError::InternalServerError => "Internal Server Error",
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
