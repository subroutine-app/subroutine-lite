use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::provider::{Provider, SyncOutcome};
use crate::providers::graph;

// Windows only appends .exe during lookup; the npm .cmd shim must be named explicitly.
#[cfg(windows)]
const BINARY_NAMES: &[&str] = &["workiq.cmd", "workiq.exe", "workiq.bat", "workiq"];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["workiq"];

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

pub struct WorkIqProvider {
    binary: PathBuf,
    account: Option<String>,
    past_days: i64,
    future_days: i64,
    calendar_enabled: bool,
    accepted_events_only: bool,
    flagged_mail_enabled: bool,
    runtime: tokio::runtime::Runtime,
}

impl WorkIqProvider {
    pub fn new() -> Result<Self> {
        let binary = discover_binary().context(
            "`workiq` CLI not found on PATH — install it with \
             `npm install -g @microsoft/workiq`, or point WORKIQ_BIN at it",
        )?;
        tracing::debug!(binary = %binary.display(), "located workiq CLI");
        Ok(Self {
            binary,
            account: std::env::var("WORKIQ_ACCOUNT")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            past_days: graph::DEFAULT_PAST_DAYS,
            future_days: graph::DEFAULT_FUTURE_DAYS,
            calendar_enabled: true,
            accepted_events_only: false,
            flagged_mail_enabled: true,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("could not create Tokio runtime for Work IQ provider")?,
        })
    }

    pub fn with_account(mut self, account: impl Into<String>) -> Self {
        self.account = Some(account.into());
        self
    }

    pub fn with_window(mut self, past_days: i64, future_days: i64) -> Self {
        self.past_days = past_days.max(0);
        self.future_days = future_days.max(0);
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

    fn fetch(&self, url: &str) -> Result<Value> {
        tracing::debug!(%url, "workiq fetch");
        let mut command = Command::new(&self.binary);
        #[cfg(windows)]
        {
            // Piped output alone does not prevent the npm .cmd shim from opening a console.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.args(["fetch", "--log-level", "None"]);
        if let Some(account) = &self.account {
            command.args(["--account", account]);
        }
        command.args(["--urls", url]);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output = self.runtime.block_on(async {
            let mut child = command
                .spawn()
                .with_context(|| format!("could not run `{}`", self.binary.display()))?;
            capture_output(&mut child, COMMAND_TIMEOUT).await
        })?;

        parse_output(output).with_context(|| format!("workiq fetch {url}"))
    }

    fn fetch_collection(&self, url: &str) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut next = Some(url.to_string());

        for page in 1..=graph::MAX_PAGES {
            let Some(url) = next.take() else {
                return Ok(items);
            };
            let payload = self.fetch(&url)?;
            if let Some(values) = payload.get("value").and_then(Value::as_array) {
                items.extend(values.iter().cloned());
            }
            next = payload
                .get("@odata.nextLink")
                .and_then(Value::as_str)
                .and_then(graph::relative_graph_path);
            if next.is_some() && page == graph::MAX_PAGES {
                tracing::warn!(
                    "workiq stopped paging after {} pages ({} items); results are truncated",
                    graph::MAX_PAGES,
                    items.len(),
                );
            }
        }

        Ok(items)
    }

    fn calendar_url(&self) -> String {
        graph::calendar_url(self.past_days, self.future_days, Utc::now())
    }
}

impl Provider for WorkIqProvider {
    fn id(&self) -> &'static str {
        "workiq"
    }

    fn name(&self) -> &'static str {
        "Microsoft 365"
    }

    fn ensure_authorized(&mut self) -> Result<()> {
        let me = self.fetch("/me?$select=id,displayName").map_err(|error| {
            tracing::warn!(%error, "workiq is not signed in");
            error.context("workiq is not authorized — run `workiq auth login`")
        })?;

        let display_name = me.get("displayName").and_then(Value::as_str);
        if me.get("id").and_then(Value::as_str).is_none() {
            bail!("workiq returned no signed-in user — run `workiq auth login`");
        }
        tracing::info!(
            user = display_name.unwrap_or("<unknown>"),
            "workiq authorized"
        );
        Ok(())
    }

    fn sync(&mut self) -> Result<SyncOutcome> {
        let mut outcome = SyncOutcome::new(self.id());

        if self.calendar_enabled {
            tracing::info!(
                "workiq calendar sync starting (window: -{} to +{} days)",
                self.past_days,
                self.future_days,
            );
            let events = self.fetch_collection(&self.calendar_url())?;
            tracing::info!("workiq fetched {} calendar entries", events.len());

            for raw in &events {
                graph::import_calendar_entry(
                    &mut outcome,
                    raw,
                    self.id(),
                    self.accepted_events_only,
                );
            }
        }

        if self.flagged_mail_enabled {
            tracing::info!("workiq flagged mail sync starting");
            let messages = self.fetch_collection(&graph::messages_url())?;
            tracing::info!("workiq fetched {} flagged messages", messages.len());

            for raw in &messages {
                graph::import_flagged_message(&mut outcome, raw, self.id());
            }
        }

        tracing::info!(
            "workiq sync complete — {} actions, {} events, {} markers",
            outcome.actions().count(),
            outcome.events().count(),
            outcome.markers().count(),
        );
        Ok(outcome)
    }
}

async fn capture_output(child: &mut Child, timeout: Duration) -> Result<Output> {
    let mut stdout_pipe = child.stdout.take().context("workiq stdout was not piped")?;
    let mut stderr_pipe = child.stderr.take().context("workiq stderr was not piped")?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = tokio::time::timeout(timeout, async {
        let (status, _, _) = tokio::try_join!(
            child.wait(),
            stdout_pipe.read_to_end(&mut stdout),
            stderr_pipe.read_to_end(&mut stderr),
        )?;
        Ok::<_, std::io::Error>(status)
    })
    .await;

    let status = match status {
        Ok(status) => status.context("could not capture workiq command output")?,
        Err(_) => {
            // Kill and reap the direct child; this does not terminate its descendants.
            child
                .kill()
                .await
                .context("workiq command timed out and its process could not be terminated")?;
            bail!("workiq command timed out after {timeout:?}; its process was terminated");
        }
    };
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn parse_output(output: Output) -> Result<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let response = parse_response(&stdout, &stderr);
    if output.status.success() {
        return response;
    }

    match response {
        Err(error) => Err(error.context(format!("workiq exited with {}", output.status))),
        Ok(_) => {
            let detail = if stderr.trim().is_empty() {
                "no error output"
            } else {
                stderr.trim()
            };
            bail!("workiq exited with {}: {}", output.status, clip(detail));
        }
    }
}

fn discover_binary() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("WORKIQ_BIN") {
        return Some(PathBuf::from(explicit));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        BINARY_NAMES
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

// WorkIQ can exit 0 on rejection, reporting an error envelope on stderr
// instead of the successful payload on stdout.
fn parse_response(stdout: &str, stderr: &str) -> Result<Value> {
    let body = match stdout.trim() {
        "" => stderr.trim(),
        stdout => stdout,
    };
    if body.is_empty() {
        bail!("workiq produced no output");
    }

    let value: Value = serde_json::from_str(body)
        .with_context(|| format!("workiq returned output that is not JSON: {}", clip(body)))?;

    let Some(results) = value.get("results").and_then(Value::as_array) else {
        return Ok(value);
    };
    let first = results
        .first()
        .context("workiq returned an empty result set")?;
    if let Some(error) = first.get("error").and_then(Value::as_str) {
        let status = first
            .get("statusCode")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        bail!("rejected with status {status}: {error}");
    }
    Ok(first.get("data").cloned().unwrap_or(Value::Null))
}

fn clip(text: &str) -> String {
    text.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_child(mode: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "providers::workiq::tests::command_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("SUBROUTINE_IMPORT_CHILD_FIXTURE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    #[test]
    #[ignore = "offline subprocess fixture, launched only by process tests"]
    fn command_fixture() {
        match std::env::var("SUBROUTINE_IMPORT_CHILD_FIXTURE")
            .unwrap()
            .as_str()
        {
            "wait" => std::thread::sleep(Duration::from_secs(120)),
            mode => panic!("unknown child fixture mode: {mode}"),
        }
    }

    #[tokio::test]
    async fn a_timed_out_command_is_killed_and_reaped() {
        let mut child = fixture_child("wait");
        let error = capture_output(&mut child, Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(
            !child
                .try_wait()
                .unwrap()
                .expect("child must be reaped")
                .success()
        );
    }
}
