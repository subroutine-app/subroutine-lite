use std::collections::HashSet;

use axum::http::StatusCode;
use subroutine_core::{
    ApiErrorCode, MUTATION_PROTOCOL_VERSION, MutationOperation, MutationRequest, ResourceKey,
    ResourceValue,
};

use crate::error::MutationError;

pub(super) fn primary_resource(request: &MutationRequest) -> Result<ResourceKey, MutationError> {
    let mutation_id = Some(request.mutation_id);
    let invalid = |message: &str| {
        MutationError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ValidationFailed,
            message,
            mutation_id,
            None,
            None,
            None,
        )
    };
    if request.protocol_version != MUTATION_PROTOCOL_VERSION {
        return Err(MutationError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::UnsupportedProtocol,
            format!(
                "unsupported mutation protocol {}; expected {MUTATION_PROTOCOL_VERSION}",
                request.protocol_version
            ),
            mutation_id,
            None,
            None,
            None,
        ));
    }
    if request.dataset_id.is_nil() || request.mutation_id.is_nil() || request.client_id.is_nil() {
        return Err(invalid(
            "dataset_id, mutation_id, and client_id must not be nil",
        ));
    }
    if request.base_seq < 0 {
        return Err(invalid("base_seq must be zero or greater"));
    }

    let resource = match &request.operation {
        MutationOperation::UpsertAction { action } => ResourceKey::Action { id: action.id },
        MutationOperation::UpsertActions { actions } => primary_batch_resource(
            actions
                .iter()
                .map(|action| ResourceKey::Action { id: action.id }),
        )
        .ok_or_else(|| {
            invalid("upsert_actions requires distinct actions with non-nil identities")
        })?,
        MutationOperation::UpsertResources { resources } => {
            primary_batch_resource(resources.iter().map(ResourceValue::key)).ok_or_else(|| {
                invalid("upsert_resources requires distinct resources with non-nil identities")
            })?
        }
        MutationOperation::SetEventBusyOverride { event_id, .. } => {
            ResourceKey::Event { id: *event_id }
        }

        MutationOperation::DeleteResources { resources } => {
            primary_batch_resource(resources.iter().copied()).ok_or_else(|| {
                invalid("delete_resources requires distinct resources with non-nil identities")
            })?
        }
        MutationOperation::ReorderRoutines { routine_ids } => primary_batch_resource(
            routine_ids
                .iter()
                .map(|id| ResourceKey::Routine { id: *id }),
        )
        .ok_or_else(|| invalid("reorder_routines requires distinct, non-nil routine_ids"))?,
        MutationOperation::CompleteAction { action_id, .. }
        | MutationOperation::DeleteAction { action_id } => ResourceKey::Action { id: *action_id },
        MutationOperation::ConvertEventToMarker {
            event_id,
            marker_id,
            ..
        } => {
            if marker_id.is_nil() {
                return Err(invalid("marker_id must not be nil"));
            }
            ResourceKey::Event { id: *event_id }
        }
    };
    if resource.id().is_nil() {
        return Err(invalid("resource identity must not be nil"));
    }
    Ok(resource)
}

fn primary_batch_resource(mut keys: impl Iterator<Item = ResourceKey>) -> Option<ResourceKey> {
    let primary = keys.next()?;
    let mut seen = HashSet::new();
    std::iter::once(primary)
        .chain(keys)
        .all(|key| !key.id().is_nil() && seen.insert(key))
        .then_some(primary)
}
