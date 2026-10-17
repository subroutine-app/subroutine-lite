package com.example.subroutine_simple.sync

import android.content.Context
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.example.subroutine_simple.BuildConfig
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.local.DatasetChangedWithPendingIntentException
import com.example.subroutine_simple.data.repository.WorkspaceMergeRequiredException
import com.example.subroutine_simple.reminders.ReminderRecoveryWorker
import com.example.subroutine_simple.reminders.LocalReminderRuntime
import com.example.subroutine_simple.reminders.ReminderLedger
import com.example.subroutine_simple.reminders.ReminderScheduler
import com.example.subroutine_simple.settings.SettingsStore
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withTimeoutOrNull

class BackgroundSyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        if (BuildConfig.OFFLINE_ONLY) return Result.success()
        val authentication = AppServices.authentication(applicationContext)
        val expected = inputData.getString(ACCOUNT_ID)
        fun retryIfCurrent(): Result =
            if (shouldRetryBackgroundSync(authentication.state.value, expected)) Result.retry() else Result.success()
        return try {
            withTimeoutOrNull(60_000) {
                authentication.initialize()
                val session = authentication.state.value as? AuthenticationState.SignedIn
                    ?: return@withTimeoutOrNull retryIfCurrent()
                if (expected != null && expected != session.accountId) return@withTimeoutOrNull Result.success()
                val ledger = ReminderLedger(applicationContext)
                val epoch = ledger.epoch()
                val repository = AppServices.repository(applicationContext)

                val items = repository.fetchAll(session.accountId)
                synchronized(ReminderScheduler) {
                    if (ledger.epoch() == epoch && authentication.isCurrentSession(
                            session.accountId,
                            session.sessionGeneration
                        )
                    ) {

                        repository.activeWorkspaceDescriptor(session.accountId)?.let { workspace ->
                            if (workspace == ledger.activeWorkspace()) {
                                ReminderScheduler.reconcile(
                                    applicationContext, workspace, items.actions,
                                    SettingsStore(applicationContext).settings.value
                                )
                            }
                        }
                    }
                }
                LocalReminderRuntime.refreshCommandStatuses(applicationContext)
                ReminderRecoveryWorker.request(applicationContext)
                if (items.blockedCount == 0 && (items.syncError != null || items.pendingCount > 0)) Result.retry()
                else Result.success()
            } ?: Result.retry()
        } catch (_: WorkspaceMergeRequiredException) {
            Result.success()
        } catch (_: DatasetChangedWithPendingIntentException) {
            Result.success()
        } catch (cancelled: CancellationException) {
            currentCoroutineContext().ensureActive()
            retryIfCurrent()
        } catch (_: Exception) {
            Log.w("BackgroundSync", "Background synchronization has not converged")
            retryIfCurrent()
        } finally {
            AppServices.notifyWorkspaceChanged()
        }
    }

    companion object {
        private const val ACCOUNT_ID = "authentication-identity"
        private fun constraints() = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

        fun requestImmediate(context: Context) {
            if (BuildConfig.OFFLINE_ONLY) return
            val state = AppServices.authentication(context).state.value
            if (!shouldRetryBackgroundSync(state, null)) return
            val accountId = AppServices.accountIdentity(state) ?: return
            WorkManager.getInstance(context.applicationContext).enqueueUniqueWork(
                "subroutine-immediate-sync-$accountId", ExistingWorkPolicy.APPEND_OR_REPLACE,
                OneTimeWorkRequestBuilder<BackgroundSyncWorker>()
                    .setInputData(workDataOf(ACCOUNT_ID to accountId))
                    .setConstraints(constraints())
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build(),
            )
        }

        fun schedule(context: Context) {
            if (BuildConfig.OFFLINE_ONLY) return
            WorkManager.getInstance(context.applicationContext).enqueueUniquePeriodicWork(
                "subroutine-periodic-sync", ExistingPeriodicWorkPolicy.KEEP,
                PeriodicWorkRequestBuilder<BackgroundSyncWorker>(15, TimeUnit.MINUTES)
                    .setConstraints(constraints())
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build(),
            )
        }
    }
}
