use axum::{Json, Router, extract::rejection::JsonRejection, http::StatusCode, routing::post};
use subroutine_core::{ApiErrorCode, MutationReceipt, MutationRequest};

use crate::{auth::Tenant, error::MutationError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/mutations", post(execute))
}

async fn execute(
    Tenant(state): Tenant,
    request: Result<Json<MutationRequest>, JsonRejection>,
) -> Result<Json<MutationReceipt>, MutationError> {
    let Json(request) = request.map_err(|error| {
        MutationError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ValidationFailed,
            error.body_text(),
            None,
            None,
            None,
            None,
        )
    })?;
    Ok(Json(state.execute_mutation(request).await?))
}
