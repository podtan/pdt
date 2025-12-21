//! Error types for PDT API

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

/// API error type
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(#[from] mongodb::error::Error),

    #[error("BSON OID error: {0}")]
    BsonOid(#[from] bson::oid::Error),

    #[error("BSON serialization error: {0}")]
    BsonSer(#[from] bson::ser::Error),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Error response format
#[derive(Serialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    pub request_id: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = Uuid::new_v4().to_string();

        let (status, code, message) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg.clone()),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg.clone()),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "CONFLICT", msg.clone()),
            ApiError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "An internal error occurred".to_string(),
                )
            }
            ApiError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    "A database error occurred".to_string(),
                )
            }
            ApiError::BsonOid(e) => {
                tracing::error!("BSON OID error: {}", e);
                (
                    StatusCode::BAD_REQUEST,
                    "INVALID_ID",
                    format!("Invalid ID format: {}", e),
                )
            }
            ApiError::BsonSer(e) => {
                tracing::error!("BSON serialization error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "SERIALIZATION_ERROR",
                    "A serialization error occurred".to_string(),
                )
            }
            ApiError::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
        };

        let body = ErrorResponse {
            code: code.to_string(),
            message,
            details: None,
            request_id,
        };

        (status, Json(body)).into_response()
    }
}

/// Result type alias for API operations
pub type Result<T> = std::result::Result<T, ApiError>;
