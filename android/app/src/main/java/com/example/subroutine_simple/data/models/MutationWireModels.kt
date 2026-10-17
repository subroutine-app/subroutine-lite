@file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)

package com.example.subroutine_simple.data.models

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import com.example.subroutine_simple.data.models.Action as ActionModel
import com.example.subroutine_simple.data.models.ActionTemplate as ActionTemplateModel
import com.example.subroutine_simple.data.models.Event as EventModel
import com.example.subroutine_simple.data.models.EventTemplate as EventTemplateModel
import com.example.subroutine_simple.data.models.Marker as MarkerModel
import com.example.subroutine_simple.data.models.MarkerTemplate as MarkerTemplateModel
import com.example.subroutine_simple.data.models.Routine as RoutineModel
import com.example.subroutine_simple.data.models.Signal as SignalModel
import com.example.subroutine_simple.data.models.SignalTemplate as SignalTemplateModel

const val MUTATION_PROTOCOL_VERSION: Int = 1

@Serializable
data class MutationRequest(
    @SerialName("protocol_version")
    val protocolVersion: Int,
    @SerialName("dataset_id")
    val datasetId: String,
    @SerialName("mutation_id")
    val mutationId: String,
    @SerialName("client_id")
    val clientId: String,
    @SerialName("base_seq")
    val baseSeq: Long,
    val operation: MutationOperation,

)


@Serializable
sealed interface MutationOperation {
    @Serializable
    @SerialName("upsert_action")
    data class UpsertAction(val action: ActionModel) : MutationOperation

    @Serializable
    @SerialName("upsert_actions")
    data class UpsertActions(val actions: List<ActionModel>) : MutationOperation

    @Serializable
    @SerialName("upsert_resources")
    data class UpsertResources(val resources: List<ResourceValue>) : MutationOperation

    @Serializable
    @SerialName("delete_resources")
    data class DeleteResources(val resources: List<ResourceKey>) : MutationOperation

    @Serializable
    @SerialName("reorder_routines")
    data class ReorderRoutines(
        @SerialName("routine_ids")
        val routineIds: List<String>,
    ) : MutationOperation

    @Serializable
    @SerialName("complete_action")
    data class CompleteAction(
        @SerialName("action_id")
        val actionId: String,
        @SerialName("completed_at")
        val completedAt: String,
    ) : MutationOperation

    @Serializable
    @SerialName("delete_action")
    data class DeleteAction(
        @SerialName("action_id")
        val actionId: String,
    ) : MutationOperation

    @Serializable
    @SerialName("convert_event_to_marker")
    data class ConvertEventToMarker(
        @SerialName("event_id")
        val eventId: String,
        @SerialName("marker_id")
        val markerId: String,
        val date: String,
        @EncodeDefault(EncodeDefault.Mode.NEVER)
        @SerialName("local_end_date")
        val localEndDate: String? = null,
    ) : MutationOperation
}

@Serializable
data class ClientMutation(
    val request: MutationRequest,
    @SerialName("optimistic_patch")
    val optimisticPatch: OptimisticPatch,
)

@Serializable
sealed interface ResourceKey {
    @Serializable
    @SerialName("action")
    data class Action(val id: String) : ResourceKey

    @Serializable
    @SerialName("event")
    data class Event(val id: String) : ResourceKey

    @Serializable
    @SerialName("routine")
    data class Routine(val id: String) : ResourceKey

    @Serializable
    @SerialName("marker")
    data class Marker(val id: String) : ResourceKey

    @Serializable
    @SerialName("signal")
    data class Signal(val id: String) : ResourceKey

    @Serializable
    @SerialName("action_template")
    data class ActionTemplate(val id: String) : ResourceKey

    @Serializable
    @SerialName("event_template")
    data class EventTemplate(val id: String) : ResourceKey

    @Serializable
    @SerialName("marker_template")
    data class MarkerTemplate(val id: String) : ResourceKey

    @Serializable
    @SerialName("signal_template")
    data class SignalTemplate(val id: String) : ResourceKey
}

@Serializable
sealed interface ResourceValue {
    @Serializable
    @SerialName("action")
    data class Action(val value: ActionModel) : ResourceValue

    @Serializable
    @SerialName("event")
    data class Event(val value: EventModel) : ResourceValue

    @Serializable
    @SerialName("routine")
    data class Routine(val value: RoutineModel) : ResourceValue

    @Serializable
    @SerialName("marker")
    data class Marker(val value: MarkerModel) : ResourceValue

    @Serializable
    @SerialName("signal")
    data class Signal(val value: SignalModel) : ResourceValue

    @Serializable
    @SerialName("action_template")
    data class ActionTemplate(val value: ActionTemplateModel) : ResourceValue

    @Serializable
    @SerialName("event_template")
    data class EventTemplate(val value: EventTemplateModel) : ResourceValue

    @Serializable
    @SerialName("marker_template")
    data class MarkerTemplate(val value: MarkerTemplateModel) : ResourceValue

    @Serializable
    @SerialName("signal_template")
    data class SignalTemplate(val value: SignalTemplateModel) : ResourceValue
}

@Serializable
data class OptimisticPatch(
    val writes: List<ResourceValue> = emptyList(),
    val deletes: List<ResourceKey> = emptyList(),
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    @SerialName("routine_order")
    val routineOrder: List<String>? = null,
)

@Serializable
enum class MutationEffect {
    @SerialName("applied")
    APPLIED,

    @SerialName("no_op")
    NO_OP,
}

@Serializable
sealed interface MutationResult {
    @Serializable
    @SerialName("action_upserted")
    data class ActionUpserted(val action: ActionModel) : MutationResult

    @Serializable
    @SerialName("actions_upserted")
    data class ActionsUpserted(val actions: List<ActionModel>) : MutationResult

    @Serializable
    @SerialName("resources_upserted")
    data class ResourcesUpserted(val resources: List<ResourceValue>) : MutationResult

    @Serializable
    @SerialName("resources_deleted")
    data class ResourcesDeleted(val resources: List<ResourceKey>) : MutationResult

    @Serializable
    @SerialName("routines_reordered")
    data class RoutinesReordered(
        @SerialName("routine_ids")
        val routineIds: List<String>,
    ) : MutationResult

    @Serializable
    @SerialName("action_completed")
    data class ActionCompleted(
        val completed: ActionModel,
        val next: ActionModel? = null,
    ) : MutationResult

    @Serializable
    @SerialName("action_deleted")
    data class ActionDeleted(
        @SerialName("action_id")
        val actionId: String,
    ) : MutationResult

    @Serializable
    @SerialName("event_converted_to_marker")
    data class EventConvertedToMarker(
        @SerialName("event_id")
        val eventId: String,
        val marker: MarkerModel,
    ) : MutationResult
}

@Serializable
data class MutationReceipt(
    @SerialName("protocol_version")
    val protocolVersion: Int,
    @SerialName("dataset_id")
    val datasetId: String,
    @SerialName("mutation_id")
    val mutationId: String,
    @SerialName("client_id")
    val clientId: String,
    @SerialName("base_seq")
    val baseSeq: Long,
    @SerialName("commit_seq")
    val commitSeq: Long,
    val effect: MutationEffect,
    val replayed: Boolean,
    val result: MutationResult,
)

@Serializable
enum class ApiErrorCode {
    @SerialName("missing_token")
    MISSING_TOKEN,

    @SerialName("invalid_token")
    INVALID_TOKEN,

    @SerialName("insufficient_scope")
    INSUFFICIENT_SCOPE,

    @SerialName("wrong_tenant")
    WRONG_TENANT,

    @SerialName("authentication_unavailable")
    AUTHENTICATION_UNAVAILABLE,

    @SerialName("unsupported_protocol")
    UNSUPPORTED_PROTOCOL,

    @SerialName("validation_failed")
    VALIDATION_FAILED,

    @SerialName("resource_not_found")
    RESOURCE_NOT_FOUND,

    @SerialName("dataset_mismatch")
    DATASET_MISMATCH,

    @SerialName("stale_base")
    STALE_BASE,


    @SerialName("mutation_id_reuse")
    MUTATION_ID_REUSE,

    @SerialName("domain_conflict")
    DOMAIN_CONFLICT,

    @SerialName("transient_failure")
    TRANSIENT_FAILURE,

    @SerialName("internal_failure")
    INTERNAL_FAILURE,
}

@Serializable
data class ApiErrorBody(
    val error: ApiErrorCode,
    val message: String,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    @SerialName("mutation_id")
    val mutationId: String? = null,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val resource: ResourceKey? = null,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    @SerialName("current_seq")
    val currentSeq: Long? = null,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    @SerialName("current_dataset_id")
    val currentDatasetId: String? = null,
    val retryable: Boolean,
)
