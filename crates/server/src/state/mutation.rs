use axum::http::StatusCode;
use subroutine_core::{
    ApiErrorCode, ChangeBatch, MutationEffect, MutationOperation, MutationReceipt, MutationRequest,
    ResourceKey,
};
use uuid::Uuid;

use super::{TenantState, operations, validation};
use crate::{db, error::MutationError, ops::Settings};

pub(super) struct MutationContext {
    pub(super) mutation_id: Uuid,
    pub(super) dataset_id: Uuid,
    pub(super) current_seq: i64,
}

impl MutationContext {
    pub(super) fn error(
        &self,
        status: StatusCode,
        code: ApiErrorCode,
        message: impl Into<String>,
        resource: Option<ResourceKey>,
    ) -> MutationError {
        MutationError::new(
            status,
            code,
            message,
            Some(self.mutation_id),
            resource,
            Some(self.current_seq),
            Some(self.dataset_id),
        )
    }
}

impl TenantState {
    pub(crate) async fn execute_mutation(
        &self,
        request: MutationRequest,
    ) -> Result<MutationReceipt, MutationError> {
        let resource = validation::primary_resource(&request)?;
        let request_hash = db::mutations::request_hash(&request);
        let mut mutation = self
            .scope
            .begin_mutation()
            .await
            .map_err(MutationError::transient)?;
        let (dataset_id, current_seq) = mutation
            .identity()
            .await
            .map_err(MutationError::transient)?;
        let context = MutationContext {
            mutation_id: request.mutation_id,
            dataset_id,
            current_seq,
        };
        if request.dataset_id != dataset_id {
            mutation
                .rollback()
                .await
                .map_err(MutationError::transient)?;
            return Err(context.error(
                StatusCode::CONFLICT,
                ApiErrorCode::DatasetMismatch,
                "this change belongs to different server data",
                None,
            ));
        }

        if let Some(mut stored) = db::mutations::fetch_receipt(
            mutation.connection(),
            self.scope.user_id,
            dataset_id,
            request.mutation_id,
        )
        .await
        .map_err(MutationError::transient)?
        {
            mutation
                .rollback()
                .await
                .map_err(MutationError::transient)?;
            if stored.request_hash.as_slice() != request_hash {
                return Err(context.error(
                    StatusCode::CONFLICT,
                    ApiErrorCode::MutationIdReuse,
                    "mutation_id was already used for different content",
                    None,
                ));
            }
            stored.receipt.replayed = true;
            return Ok(stored.receipt);
        }

        let mutation =
            validate_pending_mutation(mutation, &request, &context, resource, self.settings)
                .await?;
        let operations::MutationOutcome {
            mut mutation,
            result,
            effect,
            commit_seq,
            events,
        } = operations::apply(mutation, request.operation, &context).await?;

        let receipt = MutationReceipt {
            protocol_version: request.protocol_version,
            dataset_id,
            mutation_id: request.mutation_id,
            client_id: request.client_id,
            base_seq: request.base_seq,
            commit_seq,
            effect,
            replayed: false,
            result,
        };
        db::mutations::insert_receipt(
            mutation.connection(),
            self.scope.user_id,
            &request_hash,
            &receipt,
        )
        .await
        .map_err(MutationError::transient)?;
        mutation.commit().await.map_err(MutationError::transient)?;
        if effect == MutationEffect::Applied {
            self.app.announce(
                self.scope.user_id,
                ChangeBatch::committed(commit_seq, events),
            );
        }
        Ok(receipt)
    }
}

async fn validate_pending_mutation(
    mutation: db::TenantMutation,
    request: &MutationRequest,
    context: &MutationContext,
    resource: ResourceKey,
    settings: Settings,
) -> Result<db::TenantMutation, MutationError> {
    if request.base_seq != context.current_seq {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::CONFLICT,
            ApiErrorCode::StaleBase,
            "server data changed; refresh before retrying this change",
            None,
        ));
    }
    if matches!(
        &request.operation,
        MutationOperation::CompleteAction { completed_at, .. }
            if *completed_at > chrono::Utc::now() + chrono::Duration::minutes(5)
    ) {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ValidationFailed,
            "completed_at must not be more than five minutes in the future",
            Some(resource),
        ));
    }
    if let Err(error) = validation::durations(&request.operation, settings) {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ValidationFailed,
            error,
            Some(resource),
        ));
    }
    Ok(mutation)
}
