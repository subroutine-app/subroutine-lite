use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subroutine_auth::{Issuer, TransportSecurity, client};
use subroutine_core::AccountInfo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) const CALLBACK: &str = "http://127.0.0.1:43119/oidc/callback";
const CALLBACK_PATH: &str = "/oidc/callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const ACCOUNT_PATH: &str = "/v1/account";

const SERVER_URL: &str = "SUBROUTINE_LITE_SERVER_URL";
const OIDC_ISSUER: &str = "SUBROUTINE_LITE_OIDC_ISSUER";
const OIDC_CLIENT_ID: &str = "SUBROUTINE_LITE_OIDC_CLIENT_ID";
const ALLOW_INSECURE_LOOPBACK: &str = "SUBROUTINE_LITE_OIDC_ALLOW_INSECURE_LOOPBACK";
const OFFLINE_ONLY: &str = "SUBROUTINE_LITE_OFFLINE_ONLY";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Config {
    pub issuer: String,
    pub client_id: String,
    pub api: String,
    pub allow_loopback: bool,
}

impl Config {
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        Self::resolve(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid Unicode.")),
        })
    }

    pub(crate) fn resolve(
        mut read: impl FnMut(&str) -> Result<Option<String>, String>,
    ) -> Result<Option<Self>, String> {
        let offline_only = boolean(OFFLINE_ONLY, read(OFFLINE_ONLY)?)?;
        let allow_loopback = boolean(ALLOW_INSECURE_LOOPBACK, read(ALLOW_INSECURE_LOOPBACK)?)?;
        let (api, issuer, client_id) = match (
            read(SERVER_URL)?,
            read(OIDC_ISSUER)?,
            read(OIDC_CLIENT_ID)?,
        ) {
            (None, None, None) => (
                "https://lite-api.subroutineapp.com".to_owned(),
                "https://auth.subroutineapp.com/auth/v1/".to_owned(),
                "subroutine-desktop".to_owned(),
            ),
            (Some(api), Some(issuer), Some(client_id)) => (api, issuer, client_id),
            _ => {
                return Err(format!(
                    "Set {SERVER_URL}, {OIDC_ISSUER} and {OIDC_CLIENT_ID} together, or unset all three to use Fermi. Partial overrides are not allowed."
                ));
            }
        };
        for (name, value) in [
            (SERVER_URL, &api),
            (OIDC_ISSUER, &issuer),
            (OIDC_CLIENT_ID, &client_id),
        ] {
            if value.trim().is_empty() {
                return Err(format!(
                    "{name} must not be empty. Unset all three endpoint/client overrides to use Fermi."
                ));
            }
        }
        if client_id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(format!(
                "{OIDC_CLIENT_ID} must not contain whitespace or control characters."
            ));
        }
        validate_url(&issuer, allow_loopback).map_err(|error| format!("{OIDC_ISSUER}: {error}"))?;
        let api =
            validate_url(&api, allow_loopback).map_err(|error| format!("{SERVER_URL}: {error}"))?;
        if api.path() != "/" {
            return Err(format!("{SERVER_URL} must be an origin without a path."));
        }
        Ok((!offline_only).then(|| Self {
            issuer,
            client_id,
            api: api.origin().ascii_serialization(),
            allow_loopback,
        }))
    }

    pub fn credential_name(&self) -> String {
        let identity = serde_json::to_vec(&(&self.api, &self.issuer, &self.client_id)).unwrap();
        format!("oidc:v1:{:x}", Sha256::digest(identity))
    }

    pub fn accepts_api_url(&self, url: &Url) -> bool {
        url.origin().ascii_serialization() == self.api
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.path().starts_with("/v1/")
    }
}

fn boolean(name: &str, value: Option<String>) -> Result<bool, String> {
    match value.as_deref() {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(format!("{name} must be exactly true or false, or unset.")),
    }
}

fn validate_url(value: &str, allow_loopback: bool) -> Result<Url, String> {
    let security = if allow_loopback {
        TransportSecurity::InsecureLoopback
    } else {
        TransportSecurity::HttpsOnly
    };
    Issuer::with_security(value, security).map_err(|error| error.to_string())?;
    Url::parse(value).map_err(|_| "Invalid authentication/server URL.".to_owned())
}

pub(super) fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize the authentication HTTP client.".into())
}

pub(super) struct Provider(client::Provider);

impl Provider {
    pub async fn discover(config: &Config, _http: &reqwest::Client) -> Result<Self, String> {
        let security = if config.allow_loopback {
            TransportSecurity::InsecureLoopback
        } else {
            TransportSecurity::HttpsOnly
        };
        let issuer = Issuer::with_security(config.issuer.clone(), security)
            .map_err(|error| format!("Invalid identity provider configuration: {error}"))?;
        let config = client::Config::new(
            issuer,
            config.client_id.clone(),
            CALLBACK,
            vec!["openid".into(), "subroutine:sync".into()],
        )
        .map_err(|error| format!("Invalid sign-in configuration: {error}"))?;
        client::Provider::discover(config)
            .await
            .map(Self)
            .map_err(|error| format!("Cannot initialize sign-in: {error}"))
    }

    pub async fn login(
        &self,
        _config: &Config,
        _http: &reqwest::Client,
        current: impl Fn() -> bool,
    ) -> Result<Tokens, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:43119")
            .await
            .map_err(|_| {
                "Cannot listen on 127.0.0.1:43119. Close another sign-in attempt and retry."
                    .to_owned()
            })?;
        let mut pending = self.0.authorize();
        if !current() {
            return Err("Sign-in cancelled.".into());
        }
        open::that(pending.authorization_url().as_str())
            .map_err(|_| "Could not open your system browser.".to_owned())?;
        let code = tokio::time::timeout(LOGIN_TIMEOUT, async {
            loop {
                if !current() { return Err("Sign-in cancelled.".to_owned()); }
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
                };
                let (mut stream, peer) = accepted.map_err(|_| "The sign-in callback listener failed.".to_owned())?;
                if !peer.ip().is_loopback() { continue; }
                let result = tokio::time::timeout(Duration::from_secs(2), async {
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 1024];
                    while bytes.len() < 8192 {
                        let len = stream.read(&mut chunk).await.map_err(|_| ())?;
                        if len == 0 { return Err(()); }
                        bytes.extend_from_slice(&chunk[..len]);
                        if bytes.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                    }
                    let request = std::str::from_utf8(&bytes).map_err(|_| ())?;
                    match pending.validate_callback(callback(request)?) {
                        Ok(code) => Ok(Ok(code)),
                        Err(client::CallbackError::Denied | client::CallbackError::Closed) => {
                            Ok(Err("Sign-in was refused or cancelled in the browser.".to_owned()))
                        }
                        Err(_) => Err(()),
                    }
                }).await.unwrap_or(Err(()));
                let message = if result.is_ok() {
                    "Callback received. Return to Subroutine Lite to see the sign-in result."
                } else { "Invalid callback. Return to Subroutine Lite or continue the original sign-in." };
                let response = format!("HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}", if result.is_ok() { "200 OK" } else { "400 Bad Request" }, message.len());
                let _ = tokio::time::timeout(Duration::from_secs(2), stream.write_all(response.as_bytes())).await;
                if let Ok(code) = result { return code; }
            }
        }).await.map_err(|_| "Sign-in timed out after five minutes. Try again.".to_owned())??;
        if !current() {
            return Err("Sign-in cancelled.".into());
        }
        pending
            .exchange(code)
            .await
            .map(Tokens::from)
            .map_err(|error| token_error(error).to_string())
    }

    pub async fn userinfo(&self, tokens: &Tokens) -> Result<client::UserInfo, String> {
        let tokens = tokens.to_client().map_err(|error| error.to_string())?;
        self.0
            .userinfo(&tokens)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn refresh(
        &self,
        _http: &reqwest::Client,
        old: &Tokens,
    ) -> Result<Tokens, TokenError> {
        let old = old
            .to_client()
            .map_err(|error| TokenError::Terminal(format!("Invalid saved session: {error}")))?;
        self.0
            .refresh(&old)
            .await
            .map(Tokens::from)
            .map_err(token_error)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(from = "client::Tokens")]
pub(super) struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: i64,
    pub refresh_not_before: Option<i64>,
}

impl From<client::Tokens> for Tokens {
    fn from(tokens: client::Tokens) -> Self {
        Self {
            access: tokens.access_token().to_owned(),
            refresh: tokens.refresh_token().to_owned(),
            expires_at: tokens.expires_at(),
            refresh_not_before: tokens.refresh_not_before(),
        }
    }
}

impl Tokens {
    fn to_client(&self) -> Result<client::Tokens, client::TokenValidationError> {
        client::Tokens::from_stored(
            self.access.clone(),
            self.refresh.clone(),
            self.expires_at,
            self.refresh_not_before,
        )
    }

    pub fn ensure_refresh_ready(&self) -> Result<(), String> {
        self.to_client()
            .map_err(|error| format!("Invalid saved session: {error}"))?
            .ensure_refresh_ready()
            .map_err(|error| token_error(error).to_string())
    }

    pub fn fresh(&self) -> bool {
        self.expires_at > chrono::Utc::now().timestamp().saturating_add(15)
    }
}

#[derive(Debug)]
pub(super) enum TokenError {
    Terminal(String),
    Transient(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminal(message) | Self::Transient(message) => f.write_str(message),
        }
    }
}

fn token_error(error: client::TokenError) -> TokenError {
    let message = match error {
        client::TokenError::NotReady { .. } => "The token is not yet eligible for refresh. Sync is paused; keep editing this account offline or sign in again.",
        client::TokenError::InvalidGrant => "The identity provider rejected the credential (invalid_grant). Sign in again; cached account data and offline edits are retained.",
        client::TokenError::TemporarilyUnavailable => "The token endpoint is temporarily unavailable. Sync will retry; local edits are retained.",
        _ if error.kind() == client::FailureKind::Indeterminate => "The token request outcome is uncertain; the refresh token may already have rotated. Sign in again rather than retrying the old credential. Cached account data and offline edits are retained.",
        _ => "The identity provider refused the token request. Check the client configuration and sign in again; offline edits are retained.",
    }.to_owned();
    match error.kind() {
        client::FailureKind::Transient => TokenError::Transient(message),
        client::FailureKind::Terminal | client::FailureKind::Indeterminate => {
            TokenError::Terminal(message)
        }
    }
}

fn callback(request: &str) -> Result<&str, ()> {
    let mut lines = request.split("\r\n");
    let mut start = lines.next().ok_or(())?.split(' ');
    if start.next() != Some("GET") {
        return Err(());
    }
    let target = start.next().ok_or(())?;
    if start.next() != Some("HTTP/1.1")
        || start.next().is_some()
        || !target.starts_with("/oidc/callback?")
        || target.contains('#')
    {
        return Err(());
    }
    let mut hosts = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"));
    if hosts.next().map(|(_, host)| host.trim()) != Some("127.0.0.1:43119")
        || hosts.next().is_some()
    {
        return Err(());
    }
    let url = Url::parse(&format!("http://127.0.0.1:43119{target}")).map_err(|_| ())?;
    if url.path() != CALLBACK_PATH {
        return Err(());
    }
    target.split_once('?').map(|(_, query)| query).ok_or(())
}

pub(super) async fn account(
    config: &Config,
    http: &reqwest::Client,
    tokens: &Tokens,
) -> Result<AccountInfo, String> {
    if !tokens.fresh() {
        return Err("The access token expired before account verification.".into());
    }
    let response = http
        .get(format!("{}{ACCOUNT_PATH}", config.api))
        .bearer_auth(&tokens.access)
        .send()
        .await
        .map_err(|_| "Cannot reach the API to verify the account. Sync is paused.".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "Account verification failed at {ACCOUNT_PATH} (HTTP {}). Check the server's issuer, client audience, azp and subroutine:sync configuration.",
            response.status()
        ));
    }
    let account: AccountInfo = response
        .json()
        .await
        .map_err(|_| "The API returned an invalid account response.".to_owned())?;
    if account.account_id.is_nil() || account.dataset_id.is_nil() {
        return Err("The API returned an empty account or dataset identity.".into());
    }
    Ok(account)
}
