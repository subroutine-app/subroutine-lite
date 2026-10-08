use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use subroutine_core::{ApiErrorBody, ApiErrorCode, ResourceKey};
use uuid::Uuid;

use crate::ops::OpError;

pub struct AppError {
    status: StatusCode,
    source: anyhow::Error,
}

impl AppError {
    pub fn not_found(msg: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            source: anyhow::anyhow!(msg),
        }
    }


    pub fn bad_request(msg: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            source: anyhow::anyhow!(msg),
        }
    }


}

impl From<OpError> for AppError {
    fn from(e: OpError) -> Self {
        match e {
            OpError::NotFound { .. } => Self::not_found(e.to_string()),
            OpError::Rejected(_) => Self::bad_request(e.to_string()),
        }
    }
}

impl std::fmt::Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.source, self.status)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if self.status.is_client_error() {
            tracing::warn!(status = %self.status, "request rejected: {:?}", self.source);
        } else {
            tracing::error!("handler error: {:?}", self.source);
        }
        let body = if self.status.is_client_error() {
            self.source.to_string()
        } else {
            "internal server error".to_owned()
        };
        (self.status, body).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            source: e,
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug)]
pub(crate) struct MutationError {
    status: StatusCode,
    body: ApiErrorBody,
    source: Option<anyhow::Error>,
}

impl MutationError {
    pub(crate) fn new(
        status: StatusCode,
        code: ApiErrorCode,
        message: impl Into<String>,
        mutation_id: Option<Uuid>,
        resource: Option<ResourceKey>,
        current_seq: Option<i64>,
        current_dataset_id: Option<Uuid>,
    ) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                error: code,
                message: message.into(),
                mutation_id,
                resource,
                current_seq,
                current_dataset_id,
                retryable: status == StatusCode::SERVICE_UNAVAILABLE,
            },
            source: None,
        }
    }

    pub(crate) fn transient(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: ApiErrorBody {
                error: ApiErrorCode::TransientFailure,
                message: "mutation outcome is unknown; retry the exact same mutation".into(),
                mutation_id: None,
                resource: None,
                current_seq: None,
                current_dataset_id: None,
                retryable: true,
            },
            source: Some(error.into()),
        }
    }
}

impl IntoResponse for MutationError {
    fn into_response(self) -> Response {
        if let Some(source) = self.source {
            tracing::error!(?source, "transient mutation handler error");
        } else {
            tracing::warn!(status = %self.status, message = %self.body.message, "mutation refused");
        }
        (self.status, Json(self.body)).into_response()
    }
}
