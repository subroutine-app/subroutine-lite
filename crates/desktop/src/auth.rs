use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod profile;
mod protocol;
use profile::Profile;
pub(crate) use protocol::{ACCOUNT_PATH, Config};
use protocol::{Provider, TokenError, Tokens};

pub(crate) const SIGN_IN_UNAVAILABLE: &str = "Sign-in unavailable";
const REAUTHENTICATION_REQUIRED: &str =
    "Sign in again to synchronize. This account and its queued edits remain available offline.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AuthenticationState {
    Restoring,
    #[default]
    SignedOut,
    SigningIn,
    SigningOut,
    SignedIn,
    Refreshing,
    Offline(String),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestScope {
    generation: u64,
    pub account_id: Uuid,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Persistence {
    #[default]
    Durable,
    Pending,
}

#[derive(Serialize, Deserialize, Clone)]
struct SavedSession {
    config: Config,
    account_id: Uuid,
    tokens: Option<Tokens>,
    #[serde(default)]
    profile: Option<Profile>,
    #[serde(skip)]
    persistence: Persistence,
}

#[derive(Default)]
struct SessionState {
    authentication: AuthenticationState,
    generation: u64,
    revision: u64,
    saved: Option<SavedSession>,
    verified: bool,
    retry_at: Option<Instant>,
    subscribers: Vec<flume::Sender<AuthenticationState>>,
}

impl SessionState {
    fn publish(&mut self, authentication: AuthenticationState) {
        self.authentication = authentication;
        self.subscribers
            .retain(|subscriber| subscriber.send(self.authentication.clone()).is_ok());
    }

    fn scope(&self) -> Option<RequestScope> {
        self.saved.as_ref().map(|saved| RequestScope {
            generation: self.generation,
            account_id: saved.account_id,
        })
    }

    fn advance(&mut self) -> u64 {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("auth generation exhausted");
        self.verified = false;
        self.retry_at = None;
        self.generation
    }
}

trait Credentials: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, String>;
    fn save(&self, bytes: &[u8]) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

struct OsCredentials(String);
impl OsCredentials {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new("com.subroutine.SubroutineLite", &self.0)
            .map_err(|_| "Cannot open the OS credential store.".into())
    }
}
impl Credentials for OsCredentials {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        match self.entry()?.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(
                "Cannot read the OS credential store. Unlock it and restart or sign in again."
                    .into(),
            ),
        }
    }
    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        self.entry()?.set_secret(bytes).map_err(|_| "Cannot save the session in the OS credential store. Sync is paused; unlock the store and retry.".into())
    }
    fn delete(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Signed out in memory, but the saved OS credential could not be removed. Unlock the credential store and use Sign Out again before quitting.".into()),
        }
    }
}

struct Inner {
    config: Result<Option<Config>, String>,
    credentials: Option<Arc<dyn Credentials>>,
    state: Mutex<SessionState>,
    operation: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub(crate) struct AuthSession(Arc<Inner>);

impl Default for AuthSession {
    fn default() -> Self {
        Self::new(Ok(None))
    }
}

struct GlobalAuthSession(AuthSession);
impl Global for GlobalAuthSession {}

impl AuthSession {
    fn new(config: Result<Option<Config>, String>) -> Self {
        let authentication = match &config {
            Ok(Some(_)) => AuthenticationState::Restoring,
            Ok(None) => AuthenticationState::SignedOut,
            Err(error) => AuthenticationState::Error(error.clone()),
        };
        let credentials = config.as_ref().ok().and_then(|c| c.as_ref()).map(|config| {
            Arc::new(OsCredentials(config.credential_name())) as Arc<dyn Credentials>
        });
        Self(Arc::new(Inner {
            config,
            credentials,
            state: Mutex::new(SessionState {
                authentication,
                ..Default::default()
            }),
            operation: tokio::sync::Mutex::new(()),
        }))
    }

    pub(crate) fn initialize_global(config: Result<Option<Config>, String>, cx: &mut App) -> Self {
        if let Some(existing) = cx.try_global::<GlobalAuthSession>() {
            return existing.0.clone();
        }
        let session = Self::new(config);
        cx.set_global(GlobalAuthSession(session.clone()));
        session
    }

    pub(crate) fn global(cx: &App) -> Self {
        cx.global::<GlobalAuthSession>().0.clone()
    }
    pub(crate) fn state(&self) -> AuthenticationState {
        self.0.state.lock().unwrap().authentication.clone()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.0.state.lock().unwrap().generation
    }
    pub(crate) fn scope(&self) -> Option<RequestScope> {
        self.0.state.lock().unwrap().scope()
    }
    pub(crate) fn scope_is_current(&self, scope: RequestScope) -> bool {
        self.scope() == Some(scope)
    }
    pub(crate) fn account_id(&self) -> Option<Uuid> {
        self.scope().map(|scope| scope.account_id)
    }

    pub(crate) fn subscribe(&self) -> flume::Receiver<AuthenticationState> {
        let (tx, rx) = flume::unbounded();
        let mut state = self.0.state.lock().unwrap();
        let _ = tx.send(state.authentication.clone());
        state.subscribers.push(tx);
        rx
    }

    pub(crate) fn is_interactive_sign_in_configured(&self) -> bool {
        matches!(&self.0.config, Ok(Some(_)))
    }
    pub(crate) fn is_signed_in(&self) -> bool {
        matches!(
            self.state(),
            AuthenticationState::SignedIn
                | AuthenticationState::Refreshing
                | AuthenticationState::Offline(_)
        )
    }
    pub(crate) fn can_sign_out(&self) -> bool {
        self.account_id().is_some()
            || matches!(
                self.state(),
                AuthenticationState::Restoring
                    | AuthenticationState::SigningIn
                    | AuthenticationState::Error(_)
            ) && self.is_interactive_sign_in_configured()
    }
    pub(crate) fn display_name(&self) -> Option<String> {
        self.0.state.lock().unwrap().saved.as_ref().map(|saved| {
            saved
                .profile
                .as_ref()
                .and_then(|profile| profile.name.clone())
                .unwrap_or_else(|| format!("Account {}", saved.account_id))
        })
    }
    pub(crate) fn can_sign_in(&self) -> bool {
        self.is_interactive_sign_in_configured()
            && matches!(
                self.state(),
                AuthenticationState::SignedOut
                    | AuthenticationState::Error(_)
                    | AuthenticationState::Offline(_)
            )
    }

    fn config(&self) -> Result<&Config, String> {
        self.0
            .config
            .as_ref()
            .map_err(Clone::clone)?
            .as_ref()
            .ok_or_else(|| SIGN_IN_UNAVAILABLE.into())
    }

    fn fail(&self, generation: u64, error: String, retry: bool) {
        let mut state = self.0.state.lock().unwrap();
        if state.generation != generation {
            return;
        }
        state.verified = false;

        let retryable_session = retry
            && state
                .saved
                .as_ref()
                .is_some_and(|saved| saved.tokens.is_some());
        state.retry_at = retryable_session.then(|| Instant::now() + Duration::from_secs(5));
        state.publish(if retryable_session {
            AuthenticationState::Offline(error)
        } else {
            AuthenticationState::Error(error)
        });
    }

    async fn credential<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&dyn Credentials) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let credentials = self.0.credentials.clone().ok_or(SIGN_IN_UNAVAILABLE)?;
        tokio::task::spawn_blocking(move || operation(credentials.as_ref()))
            .await
            .map_err(|_| "The OS credential operation failed.".to_owned())?
    }

    async fn save(&self, saved: SavedSession) -> Result<(), String> {
        let bytes = serde_json::to_vec(&saved)
            .map_err(|_| "Cannot encode the saved session.".to_owned())?;
        self.credential(move |store| store.save(&bytes)).await
    }

    fn blocking(
        &self,
        future: impl std::future::Future<Output = Result<(), String>>,
    ) -> Result<(), String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Cannot start the authentication runtime.".to_owned())?
            .block_on(future)
    }

    pub(crate) fn restore_blocking(&self) {
        if !self.is_interactive_sign_in_configured() {
            return;
        }
        let generation = {
            let state = self.0.state.lock().unwrap();
            if state.authentication != AuthenticationState::Restoring {
                return;
            }
            state.generation
        };
        if let Err(error) = self.blocking(self.restore(generation)) {
            self.fail(generation, error, true);
        }
    }

    async fn restore(&self, generation: u64) -> Result<(), String> {
        {
            let _operation = self.0.operation.lock().await;
            if self.generation() != generation {
                return Ok(());
            }
            let saved = self.credential(|store| store.load()).await?;
            let mut state = self.0.state.lock().unwrap();
            if state.generation != generation {
                return Ok(());
            }
            let Some(bytes) = saved else {
                state.publish(AuthenticationState::SignedOut);
                return Ok(());
            };
            let saved: SavedSession = serde_json::from_slice(&bytes).map_err(|_| {
                "The saved session is invalid. Sign out to remove it, then sign in again."
                    .to_owned()
            })?;
            if &saved.config != self.config()? || saved.account_id.is_nil() {
                return Err(
                    "The saved session does not match this API, issuer and client. Sign in again."
                        .into(),
                );
            }
            let needs_sign_in = saved.tokens.is_none();
            state.saved = Some(saved);
            state.verified = false;
            if needs_sign_in {
                state.publish(AuthenticationState::Error(REAUTHENTICATION_REQUIRED.into()));
                return Ok(());
            }
            state.publish(AuthenticationState::Offline(
                "Verifying the saved account; local edits remain available.".into(),
            ));
        }
        self.ensure_fresh().await
    }

    pub(crate) fn begin_sign_in(&self) -> Option<u64> {
        if !self.can_sign_in() {
            return None;
        }
        let mut state = self.0.state.lock().unwrap();
        let generation = state.advance();
        state.publish(AuthenticationState::SigningIn);
        Some(generation)
    }

    pub(crate) fn sign_in_blocking(&self, generation: u64) {
        if let Err(error) = self.blocking(self.sign_in(generation)) {
            self.fail(generation, error, false);
        }
    }

    async fn sign_in(&self, generation: u64) -> Result<(), String> {
        let config = self.config()?;
        {
            let _operation = self.0.operation.lock().await;
            if self.generation() != generation {
                return Ok(());
            }
            if self.account_id().is_none() {
                self.credential(|store| store.delete()).await?;
            }
        }
        let http = protocol::http_client()?;
        let provider = Provider::discover(config, &http).await?;
        let tokens = provider
            .login(config, &http, || self.generation() == generation)
            .await?;
        if self.generation() != generation {
            return Ok(());
        }
        let account = protocol::account(config, &http, &tokens).await?;
        let profile = Profile::fetch(config, &http, &tokens, Some(&provider)).await;
        let _operation = self.0.operation.lock().await;
        if self.generation() != generation {
            return Ok(());
        }
        let cached = self
            .0
            .state
            .lock()
            .unwrap()
            .saved
            .as_ref()
            .filter(|saved| saved.account_id == account.account_id)
            .and_then(|saved| saved.profile.clone());
        let profile = Profile::reconcile(cached, profile);
        let saved = SavedSession {
            config: config.clone(),
            account_id: account.account_id,
            tokens: Some(tokens),
            profile,
            persistence: Persistence::Durable,
        };
        self.save(saved.clone()).await?;
        let mut state = self.0.state.lock().unwrap();
        if state.generation == generation {
            state.saved = Some(saved);
            state.verified = true;
            state.revision += 1;
            state.publish(AuthenticationState::SignedIn);
        }
        Ok(())
    }

    pub(crate) fn begin_sign_out(&self) -> u64 {
        let mut state = self.0.state.lock().unwrap();
        let generation = state.advance();
        state.saved = None;
        state.publish(AuthenticationState::SigningOut);
        generation
    }

    pub(crate) fn sign_out_blocking(&self, generation: u64) {
        let result = self.blocking(async {
            let _operation = self.0.operation.lock().await;
            if self.generation() != generation {
                return Ok(());
            }
            self.credential(|store| store.delete()).await?;
            let mut state = self.0.state.lock().unwrap();
            if state.generation == generation {
                state.publish(AuthenticationState::SignedOut);
            }
            Ok(())
        });
        if let Err(error) = result {
            self.fail(generation, error, false);
        }
    }

    pub(crate) async fn ensure_fresh(&self) -> Result<(), String> {
        let scope = self.scope().ok_or("Sign in to synchronize.")?;
        self.fresh(scope, None).await
    }

    async fn fresh(
        &self,
        scope: RequestScope,
        rejected_revision: Option<u64>,
    ) -> Result<(), String> {
        let _operation = self.0.operation.lock().await;
        let (saved, refresh) = {
            let mut state = self.0.state.lock().unwrap();
            if state.scope() != Some(scope) {
                return Err("The account session changed; stale request discarded.".into());
            }
            if !matches!(
                state.authentication,
                AuthenticationState::SignedIn
                    | AuthenticationState::Refreshing
                    | AuthenticationState::Offline(_)
            ) {
                return Err(
                    "Authentication is not ready; this account remains available offline.".into(),
                );
            }
            let force = rejected_revision == Some(state.revision);
            let saved = state.saved.as_ref().unwrap();
            let tokens = saved.tokens.as_ref().ok_or(REAUTHENTICATION_REQUIRED)?;
            if !force
                && state.verified
                && saved.persistence == Persistence::Durable
                && tokens.fresh()
            {
                return Ok(());
            }
            if state
                .retry_at
                .is_some_and(|deadline| Instant::now() < deadline)
            {
                return Err("Authentication is unavailable; sync is paused and will retry.".into());
            }
            let refresh = force || !tokens.fresh();
            let saved = saved.clone();
            state.verified = false;
            state.publish(AuthenticationState::Refreshing);
            (saved, refresh)
        };
        match self.renew(scope, saved, refresh).await {
            Ok(()) => Ok(()),
            Err(TokenError::Transient(error)) => {
                self.fail(scope.generation, error.clone(), true);
                Err(error)
            }
            Err(TokenError::Terminal(error)) => Err(self.require_sign_in(scope, error).await),
        }
    }

    async fn require_sign_in(&self, scope: RequestScope, mut error: String) -> String {
        let saved = {
            let mut state = self.0.state.lock().unwrap();
            if state.scope() != Some(scope) {
                return error;
            }
            let saved = state.saved.as_mut().unwrap();
            saved.tokens = None;
            let saved = saved.clone();
            state.verified = false;
            state.retry_at = None;
            state.publish(AuthenticationState::Error(error.clone()));
            saved
        };
        if let Err(storage_error) = self.save(saved).await {
            error = format!(
                "{error} Could not persist the sign-in requirement: {storage_error} Remove the saved session with Sign Out before quitting."
            );
        }
        self.fail(scope.generation, error.clone(), false);
        error
    }

    async fn persist(
        &self,
        scope: RequestScope,
        saved: &mut SavedSession,
    ) -> Result<(), TokenError> {
        if !self.scope_is_current(scope) {
            return Err(TokenError::Transient("The account session changed.".into()));
        }
        self.save(saved.clone())
            .await
            .map_err(TokenError::Transient)?;
        let mut state = self.0.state.lock().unwrap();
        if state.scope() != Some(scope) {
            return Err(TokenError::Transient("The account session changed.".into()));
        }
        saved.persistence = Persistence::Durable;
        state.saved.as_mut().unwrap().persistence = Persistence::Durable;
        Ok(())
    }

    async fn checkpoint_refresh(
        &self,
        scope: RequestScope,
        saved: &SavedSession,
    ) -> Result<(), TokenError> {
        if !self.scope_is_current(scope) {
            return Err(TokenError::Transient("The account session changed.".into()));
        }
        let checkpoint = SavedSession {
            tokens: None,
            ..saved.clone()
        };
        self.save(checkpoint.clone()).await.map_err(|error| {
            TokenError::Transient(format!(
                "Cannot checkpoint the session; no refresh token was submitted. {error}"
            ))
        })?;
        let mut state = self.0.state.lock().unwrap();
        if state.scope() != Some(scope) {
            return Err(TokenError::Transient("The account session changed.".into()));
        }
        state.saved = Some(checkpoint);
        Ok(())
    }

    async fn renew(
        &self,
        scope: RequestScope,
        mut saved: SavedSession,
        refresh: bool,
    ) -> Result<(), TokenError> {
        if saved.persistence == Persistence::Pending {
            self.persist(scope, &mut saved).await?;
        }
        let config = self.config().map_err(TokenError::Terminal)?;
        let http = protocol::http_client().map_err(TokenError::Transient)?;
        let tokens = saved
            .tokens
            .as_ref()
            .ok_or_else(|| TokenError::Terminal(REAUTHENTICATION_REQUIRED.into()))?;
        let provider = if refresh {
            tokens
                .ensure_refresh_ready()
                .map_err(TokenError::Transient)?;
            let provider = Provider::discover(config, &http)
                .await
                .map_err(TokenError::Transient)?;
            self.checkpoint_refresh(scope, &saved).await?;
            let rotation = match provider.refresh(&http, tokens).await {
                Ok(tokens) => {
                    saved.tokens = Some(tokens);
                    Ok(())
                }
                Err(error @ TokenError::Transient(_)) => Err(error),
                Err(error) => return Err(error),
            };
            saved.persistence = Persistence::Pending;
            {
                let mut state = self.0.state.lock().unwrap();
                if state.scope() != Some(scope) {
                    return Err(TokenError::Transient("The account session changed.".into()));
                }
                state.saved = Some(saved.clone());
                if rotation.is_ok() {
                    state.revision += 1;
                }
            }
            self.persist(scope, &mut saved).await?;
            rotation?;
            Some(provider)
        } else {
            None
        };
        let tokens = saved
            .tokens
            .as_ref()
            .ok_or_else(|| TokenError::Terminal(REAUTHENTICATION_REQUIRED.into()))?;
        let account = protocol::account(config, &http, tokens)
            .await
            .map_err(TokenError::Transient)?;
        if account.account_id != scope.account_id {
            return Err(TokenError::Terminal("The API account changed during session restore/refresh. Sync is blocked; sign out and sign in explicitly. Cached data and queued edits have not been reassigned.".into()));
        }
        let profile = Profile::fetch(config, &http, tokens, provider.as_ref()).await;
        let mut state = self.0.state.lock().unwrap();
        if state.scope() != Some(scope) {
            return Err(TokenError::Transient("The account session changed.".into()));
        }
        state.saved.as_mut().unwrap().profile = Profile::reconcile(saved.profile, profile);
        state.verified = true;
        state.retry_at = None;
        state.publish(AuthenticationState::SignedIn);
        Ok(())
    }

    pub(crate) fn network_ready(&self, scope: RequestScope) -> bool {
        let state = self.0.state.lock().unwrap();
        state.scope() == Some(scope)
            && state.verified
            && state
                .saved
                .as_ref()
                .filter(|saved| saved.persistence == Persistence::Durable)
                .and_then(|saved| saved.tokens.as_ref())
                .is_some_and(Tokens::fresh)
    }

    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
        scope: RequestScope,
    ) -> Result<reqwest::Response, String> {
        let request = request
            .build()
            .map_err(|_| "Cannot build the API request.".to_owned())?;
        if !self.config()?.accepts_api_url(request.url()) {
            return Err("Refusing to send account credentials outside the configured API.".into());
        }
        let streaming = request.url().path() == "/v1/changes/stream";
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(45));
        if !streaming {
            builder = builder.timeout(Duration::from_secs(30));
        }
        let http = builder
            .build()
            .map_err(|_| "Cannot initialize the API HTTP client.".to_owned())?;
        self.fresh(scope, None).await?;
        for attempt in 0..2 {
            let (token, revision) = {
                let state = self.0.state.lock().unwrap();
                if state.scope() != Some(scope) || !state.verified {
                    return Err("The account session changed.".into());
                }
                let tokens = state
                    .saved
                    .as_ref()
                    .unwrap()
                    .tokens
                    .as_ref()
                    .ok_or(REAUTHENTICATION_REQUIRED)?;
                if !tokens.fresh() {
                    return Err("Authentication expired; sync is paused.".into());
                }
                (tokens.access.clone(), state.revision)
            };
            let mut sending = request
                .try_clone()
                .ok_or("The API request cannot be retried safely.")?;
            let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| "The provider issued an invalid bearer token.".to_owned())?;
            header.set_sensitive(true);
            sending
                .headers_mut()
                .insert(reqwest::header::AUTHORIZATION, header);
            let changes = self.subscribe();
            let changed = async {
                loop {
                    if !self.scope_is_current(scope) {
                        break;
                    }
                    if changes.recv_async().await.is_err() {
                        break;
                    }
                }
            };
            let response = tokio::select! {
                biased;
                _ = changed => return Err("The account session changed; the request was cancelled.".into()),
                response = http.execute(sending) => response.map_err(|_| "Cannot reach the API. Local edits are retained.".to_owned())?,
            };
            if !self.scope_is_current(scope) {
                return Err("Discarded a response from an obsolete account session.".into());
            }
            if response.status() == reqwest::StatusCode::FORBIDDEN {
                self.fail(scope.generation, "The API refused access. Check the client's subroutine:sync scope and account authorization; offline edits are retained.".into(), false);
                return Err("The API refused access.".into());
            }
            if response.status() != reqwest::StatusCode::UNAUTHORIZED {
                return Ok(response);
            }
            if attempt == 0 {
                self.fresh(scope, Some(revision)).await?;
            } else {
                self.invalidate(scope, revision);
            }
        }
        Err("The API rejected authentication after refresh. Sign in again.".into())
    }

    fn invalidate(&self, scope: RequestScope, revision: u64) {
        let mut state = self.0.state.lock().unwrap();
        if state.scope() == Some(scope) && state.revision == revision {
            state.verified = false;
            state.retry_at = Some(Instant::now() + Duration::from_secs(5));
            state.publish(AuthenticationState::Error("The API rejected authentication after refresh. Sign in again; local edits are retained.".into()));
        }
    }
}
