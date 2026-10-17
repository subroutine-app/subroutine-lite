package com.example.subroutine_simple.ui

enum class SyncConnectionStatus {
    Connecting,
    Connected,
    Offline,
    LocalOnly,
    SignInRequired,
}

internal enum class SyncConnectionSignal {
    Checking,
    Connected,
    Disconnected,
    LocalOnly,
    SignInRequired,
}

internal class SyncConnectionTracker {
    private var displayedStatus = SyncConnectionStatus.Connecting
    private var pendingTransition: PendingTransition? = null

    val status: SyncConnectionStatus
        get() = displayedStatus

    val nextUpdateAtMillis: Long?
        get() = pendingTransition?.atMillis

    fun observe(signal: SyncConnectionSignal, nowMillis: Long) {
        when (signal) {
            SyncConnectionSignal.LocalOnly -> show(SyncConnectionStatus.LocalOnly)
            SyncConnectionSignal.SignInRequired -> show(SyncConnectionStatus.SignInRequired)
            else -> {
                if (displayedStatus == SyncConnectionStatus.LocalOnly ||
                    displayedStatus == SyncConnectionStatus.SignInRequired
                ) {
                    show(SyncConnectionStatus.Connecting)
                }

                when (signal) {
                    SyncConnectionSignal.Checking -> {
                        if (displayedStatus == SyncConnectionStatus.Connecting &&
                            pendingTransition == null
                        ) {
                            schedule(SyncConnectionStatus.Offline, nowMillis)
                        }
                    }
                    SyncConnectionSignal.Connected -> schedule(SyncConnectionStatus.Connected, nowMillis)
                    SyncConnectionSignal.Disconnected -> schedule(SyncConnectionStatus.Offline, nowMillis)
                    else -> Unit
                }
            }
        }
        advance(nowMillis)
    }

    fun advance(nowMillis: Long) {
        val pending = pendingTransition ?: return
        if (nowMillis >= pending.atMillis) {
            show(pending.target)
        }
    }

    private fun schedule(target: SyncConnectionStatus, nowMillis: Long) {
        if (target == displayedStatus) {
            pendingTransition = null
        } else if (pendingTransition?.target != target) {
            val stabilityMillis = if (target == SyncConnectionStatus.Connected) {
                CONNECTED_STABILITY_MILLIS
            } else {
                OFFLINE_STABILITY_MILLIS
            }
            pendingTransition = PendingTransition(target, nowMillis + stabilityMillis)
        }
    }

    private fun show(target: SyncConnectionStatus) {
        displayedStatus = target
        pendingTransition = null
    }

    private data class PendingTransition(
        val target: SyncConnectionStatus,
        val atMillis: Long,
    )

    private companion object {
        const val CONNECTED_STABILITY_MILLIS = 2_000L
        const val OFFLINE_STABILITY_MILLIS = 10_000L
    }
}
