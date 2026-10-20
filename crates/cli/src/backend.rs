use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use local_store::{LocalStore, WorkspaceSpec};
use reqwest::{
    Client, Response,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use subroutine_core::{
    AllData, ApiErrorBody, MUTATION_PROTOCOL_VERSION, MutationEffect, MutationOperation,
    MutationReceipt, MutationRequest, MutationResult, OptimisticPatch, ResourceKey, ResourceValue,
};
use url::Url;
use uuid::Uuid;

const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_ERROR_BYTES: usize = 8 * 1024;
const CLI_DB_MARKER: &[u8] = b"subroutine-lite-cli-db-v1\n";

pub enum Backend {
    Local(LocalBackend),
    Remote(RemoteBackend),
    DryRun { _local: Option<LocalBackend> },
}

pub struct LocalBackend {
    store: LocalStore,
    _lock: File,
}

pub struct RemoteBackend {
    client: Client,
    origin: Url,
    token: String,
    dataset_id: Uuid,
    seq: i64,
    client_id: Uuid,
    dry_run: bool,
}

impl Backend {
    pub async fn open(
        db: Option<&Path>,
        server: Option<&str>,
        token_file: Option<&Path>,
        timeout: Duration,
        dry_run: bool,
    ) -> Result<(Self, AllData)> {
        ensure!(
            db.is_none() || server.is_none(),
            "--db and --server cannot be used together"
        );
        ensure!(
            token_file.is_none() || server.is_some(),
            "--token-file requires --server"
        );
        ensure!(
            !db.is_some_and(|path| path.as_os_str().is_empty()),
            "database path must not be empty"
        );
        if let Some(server) = server {
            let (remote, data) = RemoteBackend::open(server, token_file, timeout, dry_run).await?;
            return Ok((Self::Remote(remote), data));
        }

        let path = match db {
            Some(path) => path.to_path_buf(),
            None => dirs::data_local_dir()
                .context("cannot determine the local data directory; supply --db")?
                .join("subroutine-lite")
                .join("cli.sqlite3"),
        };
        let exists = path
            .try_exists()
            .with_context(|| format!("inspect database {}", path.display()))?;
        if dry_run && !exists {
            return Ok((Self::DryRun { _local: None }, AllData::default()));
        }

        let path = if exists {
            path.canonicalize()
                .with_context(|| format!("resolve database {}", path.display()))?
        } else {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            private_directory(parent)?;
            parent
                .canonicalize()
                .with_context(|| format!("resolve database directory {}", parent.display()))?
                .join(path.file_name().context("database path must name a file")?)
        };
        let lock_path = with_suffix(&path, ".lock");
        let mut lock = private_file(&lock_path)?;
        lock.try_lock().map_err(|error| {
            anyhow!(
                "cannot lock database {}: {error}; another CLI process may be using it",
                path.display()
            )
        })?;

        let mut marker = Vec::new();
        (&mut lock)
            .take(CLI_DB_MARKER.len() as u64 + 1)
            .read_to_end(&mut marker)
            .context("read CLI database ownership marker")?;
        if marker != CLI_DB_MARKER {
            let empty = match fs::metadata(&path) {
                Ok(metadata) => metadata.is_file() && metadata.len() == 0,
                Err(error) if error.kind() == io::ErrorKind::NotFound => true,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("inspect database {}", path.display()));
                }
            };
            ensure!(
                !dry_run && empty && marker.is_empty(),
                "--db {} is not a CLI-owned workspace (missing or unsupported ownership marker); use a separate CLI workspace and import data instead of pointing --db at a desktop database",
                path.display()
            );
            lock.write_all(CLI_DB_MARKER)
                .context("write CLI database ownership marker")?;
            lock.sync_all()
                .context("persist CLI database ownership marker")?;
        }

        private_file(&path)?;
        let store = LocalStore::open(&path, WorkspaceSpec::Local)
            .with_context(|| format!("open local database {}", path.display()))?;
        let data = store
            .load_projection()
            .with_context(|| format!("read local database {}", path.display()))?
            .data;
        let local = LocalBackend { store, _lock: lock };
        let backend = if dry_run {
            Self::DryRun {
                _local: Some(local),
            }
        } else {
            Self::Local(local)
        };
        Ok((backend, data))
    }

    pub async fn commit(&self, base: &AllData, patch: OptimisticPatch) -> Result<()> {
        if patch.writes.is_empty() && patch.deletes.is_empty() && patch.routine_order.is_none() {
            return Ok(());
        }
        validate_patch(&patch)?;
        match self {
            Self::Local(local) => {
                local.store.apply_local_patch(patch).context(
                    "commit local patch; if the outcome is unclear, inspect the database before retrying",
                )?;
                Ok(())
            }
            Self::DryRun { .. } => Ok(()),
            Self::Remote(remote) => remote.commit(base, patch).await,
        }
    }
}

impl RemoteBackend {
    async fn open(
        server: &str,
        token_file: Option<&Path>,
        timeout: Duration,
        dry_run: bool,
    ) -> Result<(Self, AllData)> {
        let origin = server_origin(server)?;
        ensure!(
            !timeout.is_zero() && Instant::now().checked_add(timeout).is_some(),
            "HTTP timeout must be positive and representable"
        );
        let token = access_token(token_file)?;
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("access token is not a valid bearer token"))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let mut builder = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(timeout)
            .connect_timeout(timeout)
            .https_only(origin.scheme() == "https");
        if origin.scheme() == "http" {
            builder = builder.no_proxy();
        }
        let client = builder.build().context("build HTTP client")?;
        let mut remote = Self {
            client,
            origin,
            token,
            dataset_id: Uuid::nil(),
            seq: 0,
            client_id: Uuid::new_v4(),
            dry_run,
        };
        let response = remote
            .client
            .get(remote.origin.join("v1/data")?)
            .send()
            .await
            .map_err(|error| remote.transport_error("GET /v1/data", error))?;
        let response = remote.success(response).await?;
        let data: AllData = response
            .json()
            .await
            .map_err(|error| remote.transport_error("decode GET /v1/data as AllData", error))?;
        ensure!(
            !data.dataset_id.is_nil(),
            "server snapshot has a nil dataset_id"
        );
        ensure!(data.seq >= 0, "server snapshot has a negative sequence");
        remote.dataset_id = data.dataset_id;
        remote.seq = data.seq;
        Ok((remote, data))
    }

    async fn commit(&self, base: &AllData, patch: OptimisticPatch) -> Result<()> {
        ensure!(
            base.dataset_id == self.dataset_id && base.seq == self.seq,
            "mutation base does not match the opened server snapshot"
        );
        ensure!(
            patch.routine_order.is_none(),
            "remote routine ordering is not supported by this CLI backend"
        );
        ensure!(
            patch.writes.is_empty() || patch.deletes.is_empty(),
            "a remote patch cannot mix writes and deletes; refusing to split an atomic operation"
        );
        let operation = if patch.deletes.is_empty() {
            MutationOperation::UpsertResources {
                resources: patch.writes,
            }
        } else {
            MutationOperation::DeleteResources {
                resources: patch.deletes,
            }
        };
        let request = MutationRequest {
            protocol_version: MUTATION_PROTOCOL_VERSION,
            dataset_id: self.dataset_id,
            mutation_id: Uuid::new_v4(),
            client_id: self.client_id,
            base_seq: self.seq,
            operation,
        };
        ensure!(
            request.base_seq.checked_add(1).is_some(),
            "server sequence cannot advance beyond i64::MAX"
        );
        if self.dry_run {
            return Ok(());
        }
        let http_request = self
            .client
            .post(self.origin.join("v1/mutations")?)
            .json(&request)
            .build()
            .map_err(|error| self.transport_error("prepare mutation request", error))?;
        let uncertain = || {
            format!(
                "mutation {} has no verified success receipt; it may already have committed. No retry was attempted; inspect server data before retrying",
                request.mutation_id
            )
        };
        let response = self
            .client
            .execute(http_request)
            .await
            .map_err(|error| self.transport_error("POST /v1/mutations", error))
            .with_context(uncertain)?;
        let status = response.status();
        let response = self.success(response).await.with_context(|| {
            if status.is_server_error() {
                uncertain()
            } else {
                format!(
                    "mutation {} refused; no retry was attempted",
                    request.mutation_id
                )
            }
        })?;
        let receipt: MutationReceipt = response
            .json()
            .await
            .map_err(|error| self.transport_error("decode mutation receipt", error))
            .with_context(uncertain)?;
        validate_receipt(base, &request, &receipt).with_context(uncertain)
    }

    fn transport_error(&self, operation: &str, error: reqwest::Error) -> anyhow::Error {
        anyhow!(
            "{operation}: {}",
            self.safe_message(&error.without_url().to_string())
        )
    }

    fn safe_message(&self, message: &str) -> String {
        let redacted = message.replace(&self.token, "[redacted]");
        let redacted = redacted.replace(&self.token.replace('/', "\\/"), "[redacted]");
        let mut end = redacted.len().min(MAX_ERROR_BYTES);
        while !redacted.is_char_boundary(end) {
            end -= 1;
        }
        let mut text: String = redacted[..end]
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        if end < redacted.len() {
            text.push_str(" [truncated]");
        }
        text
    }

    async fn success(&self, mut response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let limit = MAX_ERROR_BYTES + 2 * self.token.len() + 1;
        let mut body = Vec::new();
        let mut incomplete = false;
        let mut read_error = None;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let remaining = limit - body.len();
                    body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if chunk.len() >= remaining {
                        incomplete = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    incomplete = true;
                    read_error = Some(self.safe_message(&error.without_url().to_string()));
                    break;
                }
            }
        }
        let message = match serde_json::from_slice::<ApiErrorBody>(&body) {
            Ok(error) => format!("{:?}: {}", error.error, error.message),
            Err(_) => match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(value) => value.to_string(),
                Err(_) => String::from_utf8_lossy(&body).into_owned(),
            },
        };
        let mut message = self.safe_message(&message);
        if message.is_empty() {
            message.push_str("empty response body");
        }
        if incomplete {
            message.push_str(" [incomplete response body]");
        }
        if let Some(error) = read_error {
            message.push_str(&format!("; body read failed: {error}"));
        }
        bail!("HTTP {status}: {message}")
    }
}

fn server_origin(value: &str) -> Result<Url> {
    let (scheme, authority) = value
        .split_once("://")
        .context("server must be an absolute HTTPS origin or literal-loopback HTTP origin")?;
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    ensure!(
        matches!(scheme, "http" | "https")
            && !authority.is_empty()
            && !authority.contains(['/', '\\', '@', '?', '#'])
            && !value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control()),
        "server must be an origin only, without credentials, path, query, or fragment"
    );
    let origin = Url::parse(value).map_err(|_| anyhow!("invalid server origin"))?;
    ensure!(origin.host().is_some(), "server origin must have a host");
    if scheme == "http" {
        let host = if let Some(host) = authority.strip_prefix('[') {
            host.split_once(']').map(|(host, _)| host)
        } else {
            authority.split(':').next()
        };
        ensure!(
            host.and_then(|host| host.parse::<IpAddr>().ok())
                .is_some_and(|address| address.is_loopback()),
            "HTTP requires a literal loopback IP address (127.0.0.1 or [::1]); use HTTPS for other hosts"
        );
    }
    Ok(origin)
}

fn access_token(path: Option<&Path>) -> Result<String> {
    let token = match path {
        Some(path) => {
            ensure!(
                !path.as_os_str().is_empty(),
                "token-file path must not be empty"
            );
            let mut bytes = Vec::new();
            File::open(path)
                .context("open access token file")?
                .take(MAX_TOKEN_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .context("read access token file")?;
            ensure!(
                bytes.len() <= MAX_TOKEN_BYTES,
                "access token file is too large"
            );
            String::from_utf8(bytes).map_err(|_| anyhow!("access token file must contain UTF-8"))?
        }
        None => std::env::var("SUBROUTINE_LITE_ACCESS_TOKEN").map_err(|_| {
            anyhow!("set SUBROUTINE_LITE_ACCESS_TOKEN or supply --token-file for remote access")
        })?,
    };
    let token = token.trim();
    ensure!(!token.is_empty(), "access token must not be empty");
    ensure!(token.len() <= MAX_TOKEN_BYTES, "access token is too large");
    let unpadded = token.trim_end_matches('=');
    ensure!(
        !unpadded.is_empty()
            && unpadded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte)),
        "access token is not a valid bearer token"
    );
    Ok(token.to_owned())
}

fn validate_patch(patch: &OptimisticPatch) -> Result<()> {
    let mut keys = HashSet::new();
    for key in patch
        .writes
        .iter()
        .map(ResourceValue::key)
        .chain(patch.deletes.iter().copied())
    {
        ensure!(!key.id().is_nil(), "patch contains a nil resource identity");
        ensure!(
            keys.insert(key),
            "patch contains duplicate resource {key:?}"
        );
    }
    if let Some(order) = &patch.routine_order {
        let mut ids = HashSet::new();
        ensure!(
            order.iter().all(|id| !id.is_nil() && ids.insert(*id)),
            "routine order contains duplicate or nil identities"
        );
    }
    Ok(())
}

fn validate_receipt(
    base: &AllData,
    request: &MutationRequest,
    receipt: &MutationReceipt,
) -> Result<()> {
    ensure!(
        receipt.protocol_version == request.protocol_version
            && receipt.dataset_id == request.dataset_id
            && receipt.mutation_id == request.mutation_id
            && receipt.client_id == request.client_id
            && receipt.base_seq == request.base_seq,
        "mutation receipt identities do not match the request"
    );
    let expected_seq = match receipt.effect {
        MutationEffect::Applied => request.base_seq.checked_add(1),
        MutationEffect::NoOp => Some(request.base_seq),
    };
    ensure!(
        expected_seq == Some(receipt.commit_seq),
        "mutation receipt sequence does not match its effect"
    );
    ensure!(
        !receipt.replayed,
        "unexpected replay receipt for a new mutation"
    );
    match (&request.operation, &receipt.result) {
        (
            MutationOperation::UpsertResources {
                resources: expected,
            },
            MutationResult::ResourcesUpserted { resources: actual },
        ) => {
            let by_key: HashMap<_, _> = actual.iter().map(|value| (value.key(), value)).collect();
            ensure!(
                expected.len() == actual.len() && by_key.len() == actual.len(),
                "mutation receipt has missing, extra, or duplicate resources"
            );
            for expected in expected {
                let actual = by_key
                    .get(&expected.key())
                    .context("mutation receipt has an unexpected resource identity")?;
                let mut expected = expected.clone();
                match &mut expected {
                    ResourceValue::Event(event) => event.preserve_missing_availability(
                        base.events.iter().find(|previous| previous.id == event.id),
                    ),
                    ResourceValue::EventTemplate(template) => template
                        .preserve_missing_availability(
                            base.event_templates
                                .iter()
                                .find(|previous| previous.id == template.id),
                        ),
                    _ => {}
                }
                ensure!(
                    serde_json::to_value(&expected)? == serde_json::to_value(actual)?,
                    "mutation receipt resource does not match the requested write"
                );
            }
        }
        (
            MutationOperation::DeleteResources {
                resources: expected,
            },
            MutationResult::ResourcesDeleted { resources: actual },
        ) => {
            let expected: HashSet<ResourceKey> = expected.iter().copied().collect();
            let actual_keys: HashSet<ResourceKey> = actual.iter().copied().collect();
            ensure!(
                expected == actual_keys && actual_keys.len() == actual.len(),
                "mutation receipt does not match the requested deletions"
            );
        }
        _ => bail!("mutation receipt result does not match the requested operation"),
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("create private directory {}", path.display()))
}

fn new_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn private_file(path: &Path) -> Result<File> {
    match new_private_file(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::symlink_metadata(path)
                    .with_context(|| format!("inspect {}", path.display()))?
                    .file_type()
                    .is_file(),
                "expected a regular file, not a directory, symlink, or device: {}",
                path.display()
            );
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .with_context(|| format!("open {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("create {}", path.display())),
    }
}
