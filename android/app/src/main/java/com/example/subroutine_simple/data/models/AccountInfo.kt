package com.example.subroutine_simple.data.models

import java.util.UUID
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class AccountInfo(
    @SerialName("account_id") val accountId: String,
    @SerialName("dataset_id") val datasetId: String,
) {
    init {
        require(isAccountUuid(accountId) && isAccountUuid(datasetId)) { "Invalid API account or dataset UUID" }
    }
}

private fun isAccountUuid(value: String): Boolean = runCatching {
    val uuid = UUID.fromString(value)
    value == uuid.toString() && (uuid.mostSignificantBits != 0L || uuid.leastSignificantBits != 0L)
}.getOrDefault(false)
