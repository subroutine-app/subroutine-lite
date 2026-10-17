package com.example.subroutine_simple.auth

import android.app.Application
import android.content.Intent
import android.os.Bundle
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewModelScope
import com.example.subroutine_simple.sync.AppServices
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import net.openid.appauth.AuthorizationException
import net.openid.appauth.AuthorizationResponse

class AuthorizationActivity : ComponentActivity() {
    private val model: AuthorizationFlowModel by viewModels()
    private val browser = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        model.result(it.data)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            addView(ProgressBar(this@AuthorizationActivity))
            addView(TextView(this@AuthorizationActivity).apply { text = "Connecting to sign-in…" })
            addView(Button(this@AuthorizationActivity).apply {
                text = "Cancel"
                setOnClickListener { model.cancel() }
            })
        })
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = model.cancel()
        })
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                model.state.collect { state ->
                    when (state) {
                        is AuthorizationFlow.Launch -> {
                            model.launched()
                            try {
                                browser.launch(AppServices.authentication(applicationContext).authorizationIntent(state.request))
                            } catch (_: Exception) {
                                model.cancel("No compatible Custom Tabs browser is available.")
                            }
                        }
                        is AuthorizationFlow.Finished -> {
                            state.message?.let { Toast.makeText(this@AuthorizationActivity, it, Toast.LENGTH_LONG).show() }
                            finish()
                        }
                        AuthorizationFlow.Waiting -> Unit
                    }
                }
            }
        }
    }

    companion object {
        internal const val ATTEMPT_ID = "auth_attempt_id"
    }
}

internal sealed interface AuthorizationFlow {
    class Launch(val request: String) : AuthorizationFlow
    data object Waiting : AuthorizationFlow
    data class Finished(val message: String? = null) : AuthorizationFlow
}

internal class AuthorizationFlowModel(application: Application, private val savedState: SavedStateHandle) :
    AndroidViewModel(application) {
    private val manager = AppServices.authentication(application)
    private val id = requireNotNull(savedState.get<String>(AuthorizationActivity.ATTEMPT_ID))
    val state = MutableStateFlow<AuthorizationFlow>(AuthorizationFlow.Waiting)

    init {
        when (savedState.get<String>("phase")) {
            "waiting" -> viewModelScope.launch {
                val pending = manager.hasPendingSignIn(id)
                if (!pending && savedState.get<String>("phase") == "waiting") {
                    cancel("Sign-in was interrupted or expired. Please try again.")
                }
            }
            "completing" -> cancel("Sign-in was interrupted. Please try again.")
            "finished" -> state.value = AuthorizationFlow.Finished()
            else -> viewModelScope.launch {
                try {
                    val request = manager.prepareSignIn(id)
                    if (savedState.get<String>("phase") != "finished") state.value = AuthorizationFlow.Launch(request)
                } catch (_: CancellationException) {
                    cancel()
                } catch (_: Exception) {
                    cancel("Could not start sign-in. Check the issuer configuration and connection.")
                }
            }
        }
    }

    fun launched() {
        savedState["phase"] = "waiting"
        state.value = AuthorizationFlow.Waiting
    }

    fun result(data: Intent?) {
        if (savedState.get<String>("phase") != "waiting") return
        val response = runCatching { data?.let(AuthorizationResponse::fromIntent) }.getOrNull()
        if (response == null) {
            val error = runCatching { data?.let(AuthorizationException::fromIntent) }.getOrNull()
            val cancelled = data == null || error?.code == AuthorizationException.GeneralErrors.USER_CANCELED_AUTH_FLOW.code
            cancel(if (cancelled) null else "Sign-in was refused or the callback was invalid. Please try again.")
            return
        }
        savedState["phase"] = "completing"
        viewModelScope.launch {
            try {
                manager.completeSignIn(id, response.jsonSerializeString())
                finish()
            } catch (_: CancellationException) {
                cancel()
            } catch (_: Exception) {
                cancel("Sign-in could not be verified by the API. Your saved workspace has not changed.")
            }
        }
    }

    fun cancel(message: String? = null) {
        savedState["phase"] = "finished"
        viewModelScope.launch {
            try {
                manager.cancelSignIn(id)
                finish(message)
            } catch (_: Exception) {
                finish("Could not clear the pending sign-in. Sign out before trying again.")
            }
        }
    }

    private fun finish(message: String? = null) {
        savedState["phase"] = "finished"
        state.value = AuthorizationFlow.Finished(message)
    }
}
