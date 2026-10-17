package com.example.subroutine_simple.sync

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged

internal class AvailableNetworkTracker<T> {
    var availableNetwork: T? = null
        private set
    private var receivedCallback = false

    fun initialize(network: T?) {
        if (!receivedCallback) availableNetwork = network
    }

    fun available(network: T) {
        receivedCallback = true
        availableNetwork = network
    }

    fun lost(network: T) {
        receivedCallback = true
        if (availableNetwork == network) availableNetwork = null
    }
}

internal fun availableNetwork(context: Context): Flow<Network?> = callbackFlow {
    val connectivity = context.getSystemService(ConnectivityManager::class.java)
    val tracker = AvailableNetworkTracker<Network>()
    val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            synchronized(tracker) {
                tracker.available(network)
                trySend(tracker.availableNetwork)
            }
        }

        override fun onLost(network: Network) {
            synchronized(tracker) {
                tracker.lost(network)
                trySend(tracker.availableNetwork)
            }
        }
    }
    connectivity.registerDefaultNetworkCallback(callback)
    try {
        synchronized(tracker) {
            tracker.initialize(connectivity.activeNetwork)
            trySend(tracker.availableNetwork)
        }
        awaitClose()
    } finally {
        connectivity.unregisterNetworkCallback(callback)
    }
}.conflate().distinctUntilChanged()
