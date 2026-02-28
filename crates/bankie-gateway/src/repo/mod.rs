pub mod api_key;
pub mod dashboard;
pub mod member;
pub mod org;
pub mod pg;

use std::fmt;

#[derive(Debug)]
pub enum RepoError {
    NotFound,
    Conflict(String),
    Database(String),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepoError::NotFound => write!(f, "not found"),
            RepoError::Conflict(msg) => write!(f, "conflict: {msg}"),
            RepoError::Database(msg) => write!(f, "database error: {msg}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<RepoError> for bankie_common::error::AppError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => {
                bankie_common::error::AppError::NotFound("Resource not found".to_string())
            }
            RepoError::Conflict(msg) => bankie_common::error::AppError::Conflict(msg),
            RepoError::Database(msg) => bankie_common::error::AppError::internal(msg),
        }
    }
}
