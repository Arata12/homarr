use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

pub type HomarrResult<T> = Result<T, HomarrError>;

#[derive(Debug, Error)]
pub enum HomarrError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    UnprocessableEntity(String),
    #[error("Upstream service error: {0}")]
    Upstream(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl HomarrError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::UnprocessableEntity(_) => "UNPROCESSABLE_ENTITY",
            Self::Upstream(_) => "UPSTREAM_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Internal(_) => "INTERNAL_SERVER_ERROR",
        }
    }
}

impl IntoResponse for HomarrError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}
