use anyhow::{Context, Result, bail};
use chrono::Utc;
use reqwest::{Client, StatusCode};
use serde_json::Value;

use crate::provider::{CalendarSource, Provider, SyncOutcome};
use crate::providers::graph;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

const ME_PATH: &str = "/me?$select=id,displayName";

const DEFAULT_CALENDAR: &str = "<default>";

pub struct OutlookProvider {
    client: Client,
    access_token: String,
    past_days: i64,
    future_days: i64,
    calendar_ids: Vec<String>,
    calendar_enabled: bool,
    accepted_events_only: bool,
    flagged_mail_enabled: bool,
    runtime: tokio::runtime::Runtime,
}

impl OutlookProvider {
    pub fn new(access_token: String) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for Outlook provider");
        let client = {
            let _guard = runtime.enter();
            Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to create HTTP client for Outlook provider")
        };
        Self {
            client,
            access_token,
            past_days: graph::DEFAULT_PAST_DAYS,
            future_days: graph::DEFAULT_FUTURE_DAYS,
            calendar_ids: Vec::new(),
            calendar_enabled: true,
            accepted_events_only: false,
            flagged_mail_enabled: true,
            runtime,
        }
    }

    pub fn with_window(mut self, past_days: i64, future_days: i64) -> Self {
        self.past_days = past_days.max(0);
        self.future_days = future_days.max(0);
        self
    }

    pub fn with_calendars(mut self, calendar_ids: Vec<String>) -> Self {
        self.calendar_ids = calendar_ids;
        self
    }

    pub fn accepted_events_only(mut self) -> Self {
        self.accepted_events_only = true;
        self
    }

    pub fn calendar_only(mut self) -> Self {
        self.flagged_mail_enabled = false;
        self
    }

    pub fn flagged_mail_only(mut self) -> Self {
        self.calendar_enabled = false;
        self
    }

    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    async fn get(&self, url: &str) -> Result<Value> {
        tracing::debug!(%url, "outlook get");
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .with_context(|| format!("could not reach {url}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("could not read the response from {url}"))?;
        if !status.is_success() {
            bail!("{}", request_error(status, &body));
        }

        serde_json::from_str(&body)
            .with_context(|| format!("outlook returned output that is not JSON: {}", clip(&body)))
    }

    async fn fetch_collection(&self, path: &str) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut next = Some(absolute(path));

        for page in 1..=graph::MAX_PAGES {
            let Some(url) = next.take() else {
                return Ok(items);
            };
            let payload = self.get(&url).await?;
            if let Some(values) = payload.get("value").and_then(Value::as_array) {
                items.extend(values.iter().cloned());
            }
            next = payload
                .get("@odata.nextLink")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if next.is_some() && page == graph::MAX_PAGES {
                tracing::warn!(
                    "outlook stopped paging after {} pages ({} items); results are truncated",
                    graph::MAX_PAGES,
                    items.len(),
                );
            }
        }

        Ok(items)
    }
}

impl Provider for OutlookProvider {
    fn id(&self) -> &'static str {
        "outlook"
    }

    fn name(&self) -> &'static str {
        "Outlook"
    }

    fn ensure_authorized(&mut self) -> Result<()> {
        let me = self
            .block_on(self.get(&absolute(ME_PATH)))
            .map_err(|error| {
                tracing::warn!(%error, "outlook is not authorized");
                error.context("could not read the signed-in Outlook account")
            })?;

        let display_name = me.get("displayName").and_then(Value::as_str);
        if me.get("id").and_then(Value::as_str).is_none() {
            bail!("Microsoft returned no signed-in user — sign in to Outlook again");
        }
        tracing::info!(
            user = display_name.unwrap_or("<unknown>"),
            "outlook authorized"
        );
        Ok(())
    }

    fn sync(&mut self) -> Result<SyncOutcome> {
        let mut outcome = SyncOutcome::new(self.id());

        if self.calendar_enabled {
            let now = Utc::now();
            let calendars = selected_calendars(&self.calendar_ids);
            tracing::info!(
                "outlook calendar sync starting ({} calendars, window: -{} to +{} days)",
                calendars.len(),
                self.past_days,
                self.future_days,
            );

            let mut fetched = 0;
            let mut failures = Vec::new();
            for calendar_id in &calendars {
                let url =
                    graph::calendar_view_url(*calendar_id, self.past_days, self.future_days, now);
                match self.block_on(self.fetch_collection(&url)) {
                    Ok(events) => {
                        tracing::debug!(
                            calendar = calendar_id.unwrap_or(DEFAULT_CALENDAR),
                            "outlook fetched {} calendar entries",
                            events.len(),
                        );
                        fetched += events.len();
                        for raw in &events {
                            graph::import_calendar_entry(
                                &mut outcome,
                                raw,
                                self.id(),
                                self.accepted_events_only,
                            );
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            calendar = calendar_id.unwrap_or(DEFAULT_CALENDAR),
                            %error,
                            "skipping an outlook calendar that could not be read",
                        );
                        failures.push(error);
                    }
                }
            }

            if failures.len() == calendars.len() {
                return Err(failures.remove(0));
            }
            tracing::info!(
                "outlook fetched {fetched} calendar entries from {} calendars",
                calendars.len() - failures.len(),
            );
        }

        if self.flagged_mail_enabled {
            tracing::info!("outlook flagged mail sync starting");
            let messages = self.block_on(self.fetch_collection(&graph::messages_url()))?;
            tracing::info!("outlook fetched {} flagged messages", messages.len());

            for raw in &messages {
                graph::import_flagged_message(&mut outcome, raw, self.id());
            }
        }

        tracing::info!(
            "outlook sync complete — {} actions, {} events, {} markers",
            outcome.actions().count(),
            outcome.events().count(),
            outcome.markers().count(),
        );
        Ok(outcome)
    }

    fn list_calendars(&mut self) -> Result<Vec<CalendarSource>> {
        let entries = self.block_on(self.fetch_collection(&graph::calendars_url()))?;
        let calendars: Vec<_> = entries.iter().filter_map(graph::convert_calendar).collect();
        tracing::info!("outlook listed {} calendars", calendars.len());
        Ok(calendars)
    }
}

fn selected_calendars(calendar_ids: &[String]) -> Vec<Option<&str>> {
    if calendar_ids.is_empty() {
        return vec![None];
    }
    calendar_ids.iter().map(|id| Some(id.as_str())).collect()
}

fn absolute(path: &str) -> String {
    format!("{GRAPH_BASE}{path}")
}

fn request_error(status: StatusCode, body: &str) -> String {
    let summary = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => format!(
            "Microsoft rejected the access token (status {status}) — \
             sign in to Outlook again to reconnect the account"
        ),
        _ => format!("outlook request failed with status {status}"),
    };
    match graph_message(body) {
        Some(message) => format!("{summary}: {message}"),
        None => summary,
    }
}

fn graph_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
}

fn clip(text: &str) -> String {
    text.chars().take(200).collect()
}
