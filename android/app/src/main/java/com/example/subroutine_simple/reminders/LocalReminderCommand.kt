package com.example.subroutine_simple.reminders

import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import java.time.Instant
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

@Serializable
internal data class LocalReminderCommand(
    val commandId: String,
    val workspaceId: String,
    val accountId: String?,
    val epoch: String,
    val occurrence: String,
    val actionId: String,
    val expectedStart: String,
    val operation: ReminderOperation,
    @Serializable(with = ReminderInstantSerializer::class) val requestedAt: Instant,
    val datasetId: String? = null,
) {
    val workspace: WorkspaceDescriptor get() = accountId?.let { WorkspaceDescriptor.Remote(workspaceId, it) } ?: WorkspaceDescriptor.Local(workspaceId)
    val retryUntil: Instant get() = requestedAt.plusSeconds(86_400)
}

internal data class StoredLocalReminderCommand(val command: LocalReminderCommand, val state: CommandState)

internal enum class CommandState {
    Pending, Queued, Retryable, AuthenticationRequired, Conflict, Confirmed, Superseded, Expired;
    val terminal: Boolean get() = this in setOf(Confirmed, Superseded, Expired)
}

internal enum class ReminderCommandResult {
    Confirmed, Queued, Retryable, AuthenticationRequired, Conflict, Superseded,
}

internal object ReminderInstantSerializer : KSerializer<Instant> {
    override val descriptor = PrimitiveSerialDescriptor("ReminderInstant", PrimitiveKind.STRING)
    override fun serialize(encoder: Encoder, value: Instant) = encoder.encodeString(value.toString())
    override fun deserialize(decoder: Decoder): Instant = Instant.parse(decoder.decodeString())
}
