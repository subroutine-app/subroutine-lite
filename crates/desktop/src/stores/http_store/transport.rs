use super::sync::{RecoveryBackoff, SseSignal};
use crate::auth::{AuthSession, RequestScope};
use crate::stores::{resource_counts, snapshot_summary};
use chrono::{DateTime, Utc};
use std::thread;
use std::time::{Duration, Instant};
use subroutine_core::{
    AccountInfo, Action, ActionTemplate, AllData, ApiErrorBody, ApiErrorCode, BatchPlacement,
    ChangeBatch, CompleteResult, DataDelta, Event, EventTemplate, Marker, MutationReceipt,
    MutationRequest, Routine, RoutineStep, Signal,
};
use uuid::Uuid;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

fn build_client(builder: reqwest::ClientBuilder) -> reqwest::Client {
    builder.redirect(reqwest::redirect::Policy::none()).build().unwrap_or_else(|e| {
        tracing::error!(error = %e, "could not configure HTTP client; continuing without timeouts");
        reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().expect("HTTP client")
    })
}

#[derive(Clone)]
pub(super) struct ApiClient {
    http: reqwest::Client,
    auth: AuthSession,
    scope: Option<RequestScope>,
}

impl ApiClient {
    pub(super) fn new(http: reqwest::Client, auth: AuthSession) -> Self {
        Self {
            http,
            scope: auth.scope(),
            auth,
        }
    }

    fn scoped(&self, scope: Option<RequestScope>) -> Self {
        Self {
            http: self.http.clone(),
            auth: self.auth.clone(),
            scope,
        }
    }

    fn request(&self, request: reqwest::RequestBuilder) -> ApiRequestBuilder {
        ApiRequestBuilder {
            request,
            auth: self.auth.clone(),
            scope: self.scope,
        }
    }

    pub(super) fn get(&self, url: String) -> ApiRequestBuilder {
        self.request(self.http.get(url))
    }

    fn post(&self, url: String) -> ApiRequestBuilder {
        self.request(self.http.post(url))
    }

    fn put(&self, url: String) -> ApiRequestBuilder {
        self.request(self.http.put(url))
    }

    fn delete(&self, url: String) -> ApiRequestBuilder {
        self.request(self.http.delete(url))
    }
}

pub(super) struct ApiRequestBuilder {
    request: reqwest::RequestBuilder,
    auth: AuthSession,
    scope: Option<RequestScope>,
}

impl ApiRequestBuilder {
    async fn send_empty(self) -> Result<(), String> {
        self.send()
            .await
            .and_then(|response| response.error_for_status().map_err(FetchError::from))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn send_json<T: serde::de::DeserializeOwned>(self) -> Result<T, String> {
        self.send()
            .await
            .and_then(|response| response.error_for_status().map_err(FetchError::from))
            .map_err(|error| error.to_string())?
            .json::<T>()
            .await
            .map_err(|error| error.to_string())
    }

    fn header(self, name: &'static str, value: &str) -> Self {
        Self {
            request: self.request.header(name, value),
            auth: self.auth,
            scope: self.scope,
        }
    }

    fn json<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            request: self.request.json(value),
            auth: self.auth,
            scope: self.scope,
        }
    }

    pub(super) async fn send(self) -> Result<reqwest::Response, FetchError> {
        let scope = self.scope.ok_or(FetchError::AuthenticationRequired)?;
        self.auth.send(self.request, scope).await.map_err(|error| {
            if self.auth.network_ready(scope) {
                FetchError::Request(error)
            } else {
                FetchError::AuthenticationRequired
            }
        })
    }
}

#[derive(Clone)]
pub(super) struct CommandSender {
    sender: flume::Sender<(Option<RequestScope>, Cmd)>,
    pub(super) scope: Option<RequestScope>,
}

impl CommandSender {
    pub(super) fn new(sender: flume::Sender<(Option<RequestScope>, Cmd)>) -> Self {
        Self {
            sender,
            scope: None,
        }
    }

    pub(super) fn send(&self, cmd: Cmd) -> Result<(), &'static str> {
        self.sender
            .send((self.scope, cmd))
            .map_err(|_| "The synchronization worker is unavailable.")
    }

    pub(super) fn is_disconnected(&self) -> bool {
        self.sender.is_disconnected()
    }
}

type Reply<T> = flume::Sender<Result<T, String>>;
type FetchReply<T> = flume::Sender<Result<T, FetchError>>;
type MutationReply = flume::Sender<Result<MutationReceipt, MutationSendError>>;

#[derive(Debug)]
pub(super) enum FetchError {
    AuthenticationRequired,
    Request(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => f.write_str("Sign in to synchronize."),
            Self::Request(message) => f.write_str(message),
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(error.to_string())
    }
}

#[derive(Debug)]
pub(super) enum MutationSendError {
    AuthenticationRequired,
    Api(ApiErrorBody),
    Transport(String),
}

#[derive(Debug)]
pub(super) enum RefreshData {
    Full(Box<AllData>),
    Delta(Box<DataDelta>),
}

impl RefreshData {
    pub(super) fn summary(&self) -> String {
        match self {
            Self::Full(data) => snapshot_summary(data),
            Self::Delta(data) => {
                let writes = resource_counts(&[
                    ("actions", data.actions.len()),
                    ("events", data.events.len()),
                    ("routines", data.routines.len()),
                    ("markers", data.markers.len()),
                    ("signals", data.signals.len()),
                    ("action templates", data.action_templates.len()),
                    ("event templates", data.event_templates.len()),
                    ("marker templates", data.marker_templates.len()),
                    ("signal templates", data.signal_templates.len()),
                ]);
                let tombstones = &data.tombstones;
                let deletes = resource_counts(&[
                    ("actions", tombstones.actions.len()),
                    ("events", tombstones.events.len()),
                    ("routines", tombstones.routines.len()),
                    ("markers", tombstones.markers.len()),
                    ("signals", tombstones.signals.len()),
                    ("action templates", tombstones.action_templates.len()),
                    ("event templates", tombstones.event_templates.len()),
                    ("marker templates", tombstones.marker_templates.len()),
                    ("signal templates", tombstones.signal_templates.len()),
                ]);
                format!(
                    "Applied server delta through sequence {}: upserts [{writes}], deletions [{deletes}], {} routine order entries",
                    data.seq,
                    data.routine_order.len()
                )
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct BootstrapData {
    pub(super) account: AccountInfo,
    pub(super) data: AllData,
}
#[allow(clippy::large_enum_variant, dead_code)]
pub(super) enum Cmd {
    Bootstrap(FetchReply<BootstrapData>),
    FetchAll(FetchReply<AllData>),
    FetchDelta {
        since: i64,
        expected_seq: i64,
        expected_dataset_id: Option<Uuid>,
        reply: FetchReply<RefreshData>,
    },
    SendMutation(MutationRequest, MutationReply),
    UpsertAction(Action, Reply<()>),
    BatchAction(Action, Option<DateTime<Utc>>, Reply<BatchPlacement>),
    DeleteAction(Uuid, Reply<()>),
    ReorderActionTemplates(Vec<Uuid>, Reply<()>),
    UpsertActionTemplate(ActionTemplate, Reply<()>),
    UpdateActionTemplate(ActionTemplate, Reply<()>),
    DeleteActionTemplate(Uuid, Reply<()>),
    UpsertEvent(Event, Reply<()>),
    DeleteEvent(Uuid, Reply<()>),
    UpsertMarker(Marker, Reply<()>),
    DeleteMarker(Uuid, Reply<()>),
    UpsertSignal(Signal, Reply<()>),
    DeleteSignal(Uuid, Reply<()>),
    ReorderEventTemplates(Vec<Uuid>, Reply<()>),
    UpsertEventTemplate(EventTemplate, Reply<()>),
    UpdateEventTemplate(EventTemplate, Reply<()>),
    DeleteEventTemplate(Uuid, Reply<()>),
    UpsertRoutine(Routine, Reply<()>),
    ReplaceRoutineSteps(Uuid, Vec<RoutineStep>, Reply<()>),
    ReorderRoutines(Vec<Uuid>, Reply<()>),
    DeleteRoutine(Uuid, Reply<()>),
    SaveAction(Uuid, Reply<ActionTemplate>),
    CompleteAction(Uuid, Reply<CompleteResult>),
    QueueAction(Uuid, Reply<Vec<Action>>),
    ClearActionDuration(Uuid, Reply<Action>),
    BacklogAction(Uuid, Reply<Action>),
    InstantiateRoutine(Uuid, Option<DateTime<Utc>>, Reply<Vec<Action>>),
    RefreshPipeline(Reply<Vec<Action>>),
}

async fn sse_loop(
    client: &ApiClient,
    base: &str,
    notify_tx: flume::Sender<SseSignal>,
    status_tx: flume::Sender<bool>,
    reconnect_rx: flume::Receiver<()>,
) {
    let mut last_event_id = None;
    let mut previous_scope = None;
    let mut backoff = RecoveryBackoff::default();
    loop {
        let scope = client.auth.scope();
        if scope != previous_scope {
            last_event_id = None;
            previous_scope = scope;
        }
        if !client.auth.is_signed_in() {
            let _ = status_tx.send(false);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if notify_tx.is_disconnected() {
                break;
            }
            continue;
        }
        let started = Instant::now();
        let result = connect_sse(
            client,
            base,
            &notify_tx,
            &status_tx,
            &mut last_event_id,
            &reconnect_rx,
        )
        .await;
        let _ = status_tx.send(false);
        if notify_tx.is_disconnected() {
            break;
        }
        if matches!(result, Ok(true)) {
            if reconnect_rx.is_disconnected() {
                break;
            }
            backoff.reset();
            continue;
        }
        if started.elapsed() >= SSE_IDLE_TIMEOUT {
            backoff.reset();
        }
        let delay = backoff.next_delay();
        if let Err(error) = result {
            tracing::warn!(%error, ?delay, "SSE disconnected; reconnect scheduled");
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            wake = reconnect_rx.recv_async() => {
                if wake.is_err() { break; }
                backoff.reset();
            }
        }
    }
}

pub(super) async fn connect_sse(
    client: &ApiClient,
    base: &str,
    notify_tx: &flume::Sender<SseSignal>,
    status_tx: &flume::Sender<bool>,
    last_event_id: &mut Option<String>,
    reconnect_rx: &flume::Receiver<()>,
) -> Result<bool, String> {
    let scope = client.auth.scope().ok_or("Sign in to synchronize.")?;
    let scoped = client.scoped(Some(scope));
    let mut request = scoped
        .get(format!("{base}/v1/changes/stream"))
        .header("Accept", "text/event-stream");
    if let Some(id) = last_event_id.as_deref() {
        request = request.header("Last-Event-ID", id);
    }
    let mut response = request
        .send()
        .await
        .and_then(|r| r.error_for_status().map_err(FetchError::from))
        .map_err(|e| e.to_string())?;

    let _ = status_tx.send(true);

    let mut buf = String::new();
    let mut frame_id = None;
    let mut data_lines = Vec::new();

    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.map_err(|error| error.to_string())?,
            _ = reconnect_rx.recv_async() => return Ok(true),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if !client.auth.network_ready(scope) { *last_event_id = None; return Ok(true); }
                continue;
            },
        };
        let Some(chunk) = chunk else {
            break;
        };
        if !client.auth.network_ready(scope) {
            *last_event_id = None;
            return Ok(true);
        }
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline) = buf.find('\n') {
            let line = buf[..newline].trim_end_matches('\r').to_owned();
            buf = buf[newline + 1..].to_owned();

            if line.is_empty() {
                let data = data_lines.join("\n");
                data_lines.clear();
                if data.is_empty() || data == "ping" {
                    frame_id = None;
                    continue;
                }

                let signal = sse_signal(&data);
                tracing::debug!(event = data, id = ?frame_id, ?signal, "SSE change event");
                if let Some(id) = frame_id.take() {
                    *last_event_id = Some(id);
                }
                if let Some(signal) = signal
                    && notify_tx.send(signal).is_err()
                {
                    return Ok(false);
                }
            } else if let Some(id) = line.strip_prefix("id:") {
                frame_id = Some(id.trim().to_owned());
            } else if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim_start().to_owned());
            }
        }
    }

    Ok(false)
}

pub(super) fn sse_signal(data: &str) -> Option<SseSignal> {
    match serde_json::from_str::<ChangeBatch>(data) {
        Ok(batch) if batch.reset => Some(SseSignal::Full),
        Ok(batch) if !batch.changes.is_empty() => {
            if batch.seq > 0 {
                Some(SseSignal::Delta { seq: batch.seq })
            } else {
                Some(SseSignal::Full)
            }
        }
        Ok(_) => None,
        Err(_) => Some(SseSignal::Full),
    }
}

pub(super) fn is_authentication_error(code: ApiErrorCode) -> bool {
    matches!(
        code,
        ApiErrorCode::MissingToken
            | ApiErrorCode::InvalidToken
            | ApiErrorCode::InsufficientScope
            | ApiErrorCode::WrongTenant
    )
}

fn temporary_http_failure(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

async fn fetch_response_error(response: reqwest::Response) -> FetchError {
    let status = response.status();
    if temporary_http_failure(status) {
        return FetchError::Request(format!(
            "Server temporarily unavailable ({status}); automatic retry scheduled."
        ));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return FetchError::AuthenticationRequired;
    }
    match response.json::<ApiErrorBody>().await {
        Ok(error) if is_authentication_error(error.error) => FetchError::AuthenticationRequired,
        Ok(error) => FetchError::Request(error.message),
        Err(error) => FetchError::Request(format!("server returned {status}: {error}")),
    }
}

async fn fetch_account(client: &ApiClient, base: &str) -> Result<AccountInfo, FetchError> {
    match client
        .get(format!("{base}{}", crate::auth::ACCOUNT_PATH))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response
            .json::<AccountInfo>()
            .await
            .map_err(|error| FetchError::Request(error.to_string())),
        Ok(response) => Err(fetch_response_error(response).await),
        Err(error) => Err(error),
    }
}

pub(super) async fn fetch_bootstrap(
    client: &ApiClient,
    base: &str,
) -> Result<BootstrapData, FetchError> {
    let account = fetch_account(client, base).await?;
    if client
        .scope
        .is_none_or(|scope| scope.account_id != account.account_id)
    {
        return Err(FetchError::AuthenticationRequired);
    }
    let data = fetch_all(client, base).await?;
    if account.dataset_id != data.dataset_id {
        return Err(FetchError::Request(
            "account and snapshot dataset identities do not match".into(),
        ));
    }
    Ok(BootstrapData { account, data })
}

pub(super) async fn send_mutation(
    client: &ApiClient,
    base: &str,
    request: &MutationRequest,
) -> Result<MutationReceipt, MutationSendError> {
    let response = client
        .post(format!("{base}/v1/mutations"))
        .json(request)
        .send()
        .await
        .map_err(|error| match error {
            FetchError::AuthenticationRequired => MutationSendError::AuthenticationRequired,
            FetchError::Request(error) => MutationSendError::Transport(error),
        })?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(MutationSendError::AuthenticationRequired);
    }
    if response.status().is_success() {
        return response
            .json::<MutationReceipt>()
            .await
            .map_err(|error| MutationSendError::Transport(error.to_string()));
    }

    let status = response.status();
    if temporary_http_failure(status) {
        return Err(MutationSendError::Transport(format!(
            "Server temporarily unavailable ({status})"
        )));
    }
    match response.json::<ApiErrorBody>().await {
        Ok(error) => Err(MutationSendError::Api(error)),
        Err(error) => Err(MutationSendError::Transport(format!(
            "mutation endpoint returned {status}: {error}"
        ))),
    }
}

pub(super) async fn fetch_all(client: &ApiClient, base: &str) -> Result<AllData, FetchError> {
    match client.get(format!("{base}/v1/data")).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<AllData>()
            .await
            .map_err(|error| FetchError::Request(error.to_string())),
        Ok(response) => Err(fetch_response_error(response).await),
        Err(error) => Err(error),
    }
}

pub(super) fn decode_data_delta(
    value: serde_json::Value,
    since: i64,
    expected_seq: i64,
    expected_dataset_id: Option<Uuid>,
) -> Result<DataDelta, String> {
    if value.get("tombstones").is_none() {
        return Err("response is not a data delta".to_owned());
    }

    let delta: DataDelta = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if expected_dataset_id.is_some_and(|dataset_id| delta.dataset_id != dataset_id) {
        return Err("delta belongs to a different server dataset".to_owned());
    }
    if delta.seq < since || delta.seq < expected_seq {
        return Err(format!(
            "delta sequence {} does not cover {since} through {expected_seq}",
            delta.seq
        ));
    }
    Ok(delta)
}

async fn fetch_delta_or_all(
    client: &ApiClient,
    base: &str,
    since: i64,
    expected_seq: i64,
    expected_dataset_id: Option<Uuid>,
) -> Result<RefreshData, FetchError> {
    let delta = match client
        .get(format!("{base}/v1/data?since={since}"))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())
            .and_then(|value| decode_data_delta(value, since, expected_seq, expected_dataset_id)),
        Ok(response) => return Err(fetch_response_error(response).await),
        Err(error) => return Err(error),
    };

    match delta {
        Ok(delta) => Ok(RefreshData::Delta(Box::new(delta))),
        Err(error) => {
            tracing::debug!(%error, since, "delta unavailable; falling back to full data");
            fetch_all(client, base)
                .await
                .map(|data| RefreshData::Full(Box::new(data)))
        }
    }
}

pub(super) fn spawn_worker(
    base: String,
    auth: AuthSession,
    cmd_rx: flume::Receiver<(Option<RequestScope>, Cmd)>,
    sse_tx: flume::Sender<SseSignal>,
    sse_status_tx: flume::Sender<bool>,
    sse_reconnect_rx: flume::Receiver<()>,
) -> std::thread::JoinHandle<()> {
    tracing::info!(server = %base, "starting synchronization worker");
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let client = ApiClient::new(
                build_client(
                    reqwest::Client::builder()
                        .connect_timeout(CONNECT_TIMEOUT)
                        .timeout(REQUEST_TIMEOUT),
                ),
                auth.clone(),
            );
            let sse_client = ApiClient::new(
                build_client(
                    reqwest::Client::builder()
                        .connect_timeout(CONNECT_TIMEOUT)
                        .read_timeout(SSE_IDLE_TIMEOUT),
                ),
                auth,
            );
            let sse_base = base.clone();
            tokio::join!(
                sse_loop(
                    &sse_client,
                    &sse_base,
                    sse_tx,
                    sse_status_tx,
                    sse_reconnect_rx
                ),
                async {
                    while let Ok((scope, cmd)) = cmd_rx.recv_async().await {
                        run(&client.scoped(scope), &base, cmd).await;
                    }
                }
            );
        });
    })
}

pub(super) async fn run(client: &ApiClient, base: &str, cmd: Cmd) {
    match cmd {
        Cmd::Bootstrap(tx) => {
            let _ = tx.send(fetch_bootstrap(client, base).await);
        }
        Cmd::FetchAll(tx) => {
            let _ = tx.send(fetch_all(client, base).await);
        }
        Cmd::FetchDelta {
            since,
            expected_seq,
            expected_dataset_id,
            reply,
        } => {
            let _ = reply.send(
                fetch_delta_or_all(client, base, since, expected_seq, expected_dataset_id).await,
            );
        }
        Cmd::SendMutation(request, reply) => {
            let _ = reply.send(send_mutation(client, base, &request).await);
        }

        Cmd::UpsertAction(action, tx) => {
            let result = client
                .put(format!("{base}/v1/actions/{}", action.id))
                .json(&action)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::BatchAction(action, cursor, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/batch"))
                .json(&serde_json::json!({ "action": action, "cursor": cursor }))
                .send_json::<BatchPlacement>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::DeleteAction(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/actions/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::ReorderActionTemplates(ids, tx) => {
            let result = client
                .put(format!("{base}/v1/actions/templates/order"))
                .json(&ids)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::UpsertActionTemplate(template, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/templates"))
                .json(&template)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::UpdateActionTemplate(template, tx) => {
            let result = client
                .put(format!("{base}/v1/actions/templates/{}", template.id))
                .json(&template)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::DeleteActionTemplate(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/actions/templates/{}", id))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::SaveAction(id, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/{id}/save"))
                .send_json::<ActionTemplate>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::CompleteAction(id, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/{id}/complete"))
                .send_json::<CompleteResult>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::ClearActionDuration(id, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/{id}/clear_duration"))
                .send_json::<Action>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::QueueAction(id, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/{id}/queue"))
                .send_json::<Vec<Action>>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::BacklogAction(id, tx) => {
            let result = client
                .post(format!("{base}/v1/actions/{id}/backlog"))
                .send_json::<Action>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::UpsertEvent(event, tx) => {
            let result = client
                .put(format!("{base}/v1/events/{}", event.id))
                .json(&event)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::DeleteEvent(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/events/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::UpsertMarker(marker, tx) => {
            let result = client
                .put(format!("{base}/v1/markers/{}", marker.id))
                .json(&marker)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::DeleteMarker(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/markers/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::UpsertSignal(signal, tx) => {
            let result = client
                .put(format!("{base}/v1/signals/{}", signal.id))
                .json(&signal)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::DeleteSignal(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/signals/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::ReorderEventTemplates(ids, tx) => {
            let result = client
                .put(format!("{base}/v1/events/templates/order"))
                .json(&ids)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::UpsertEventTemplate(template, tx) => {
            let result = client
                .post(format!("{base}/v1/events/templates"))
                .json(&template)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::UpdateEventTemplate(template, tx) => {
            let result = client
                .put(format!("{base}/v1/events/templates/{}", template.id))
                .json(&template)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::DeleteEventTemplate(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/events/templates/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::UpsertRoutine(routine, tx) => {
            let result = client
                .put(format!("{base}/v1/routines/{}", routine.id))
                .json(&routine)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::ReplaceRoutineSteps(id, steps, tx) => {
            let result = client
                .put(format!("{base}/v1/routines/{id}/steps"))
                .json(&steps)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::ReorderRoutines(ids, tx) => {
            let result = client
                .put(format!("{base}/v1/routines/order"))
                .json(&ids)
                .send_empty()
                .await;
            let _ = tx.send(result);
        }
        Cmd::DeleteRoutine(id, tx) => {
            let result = client
                .delete(format!("{base}/v1/routines/{id}"))
                .send_empty()
                .await;
            let _ = tx.send(result);
        }

        Cmd::InstantiateRoutine(id, start_time, tx) => {
            let body = serde_json::json!({
                "start_time": start_time,
            });
            let result = client
                .post(format!("{base}/v1/routines/{id}/instantiate"))
                .json(&body)
                .send_json::<Vec<Action>>()
                .await;
            let _ = tx.send(result);
        }

        Cmd::RefreshPipeline(tx) => {
            let result = client
                .post(format!("{base}/v1/pipeline/refresh"))
                .send_json::<Vec<Action>>()
                .await;
            let _ = tx.send(result);
        }
    }
}
