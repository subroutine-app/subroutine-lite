use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse as _, TokenUrl, basic::BasicClient,
};
use sha2::{Digest as _, Sha256};
use subroutine_import::provider::{CalendarSource, Provider as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const KEYRING_SERVICE: &str = "com.subroutine.SubroutineLite";
const CALLBACK_PATH: &str = "/oauth/callback";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

const DEFAULT_GOOGLE_CLIENT_ID: Option<&str> = option_env!("SUBROUTINE_GOOGLE_CLIENT_ID");
const DEFAULT_GOOGLE_CLIENT_SECRET: Option<&str> = option_env!("SUBROUTINE_GOOGLE_CLIENT_SECRET");
const DEFAULT_MICROSOFT_CLIENT_ID: Option<&str> = option_env!("SUBROUTINE_MICROSOFT_CLIENT_ID");

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";

const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const MICROSOFT_SCOPE: &str = "offline_access User.Read Calendars.Read Mail.Read";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationCredential {
    Todoist,
    Google,
    Microsoft,
}

impl IntegrationCredential {
    // Keep these keychain account IDs stable to avoid orphaning credentials.
    const fn account(self) -> &'static str {
        match self {
            Self::Todoist => "integration:todoist:api-token",
            Self::Google => "integration:google_calendar:refresh-token",
            Self::Microsoft => "integration:outlook:refresh-token",
        }
    }

    fn entry(self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, self.account()).map_err(|error| error.to_string())
    }

    pub fn load(self) -> Result<Option<String>, String> {
        match self.entry()?.get_secret() {
            Ok(secret) => String::from_utf8(secret)
                .map(Some)
                .map_err(|_| "the stored credential is not valid UTF-8".to_owned()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn is_stored(self) -> Result<bool, String> {
        self.load().map(|secret| secret.is_some())
    }

    fn lock(self) -> Result<MutexGuard<'static, ()>, String> {
        static TODOIST: Mutex<()> = Mutex::new(());
        static GOOGLE: Mutex<()> = Mutex::new(());
        static MICROSOFT: Mutex<()> = Mutex::new(());
        match self {
            Self::Todoist => &TODOIST,
            Self::Google => &GOOGLE,
            Self::Microsoft => &MICROSOFT,
        }
        .lock()
        .map_err(|_| "Credential operation failed; restart the app before trying again.".into())
    }

    pub fn save(self, secret: &str) -> Result<(), String> {
        self.entry()?
            .set_secret(secret.as_bytes())
            .map_err(|error| error.to_string())
    }

    pub fn delete(self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

pub fn todoist_token() -> Result<Option<String>, String> {
    IntegrationCredential::Todoist
        .load()
        .map(|token| token.or_else(|| optional_env("TODOIST_API_KEY")))
}

pub fn todoist_token_is_from_environment() -> Result<bool, String> {
    Ok(!IntegrationCredential::Todoist.is_stored()? && optional_env("TODOIST_API_KEY").is_some())
}

pub fn connect_todoist_blocking(token: &str) -> Result<(), String> {
    let _operation = IntegrationCredential::Todoist.lock()?;
    let token = token.trim();
    if token.is_empty() {
        return Err("Paste the API token from Todoist's settings first.".into());
    }
    let mut provider =
        subroutine_import::providers::todoist::TodoistProvider::new(token.to_owned());
    provider
        .ensure_authorized()
        .map_err(|error| format!("Could not verify the Todoist token: {error:#}"))?;
    IntegrationCredential::Todoist.save(token)
}

#[derive(Clone, PartialEq, Eq)]
pub struct OAuthProvider {
    label: &'static str,
    auth_url: &'static str,
    token_url: &'static str,
    scope: &'static str,
    extra_auth_params: &'static [(&'static str, &'static str)],
    client_id: String,
    client_secret: Option<String>,
    credential: IntegrationCredential,
}

impl OAuthProvider {
    // Google's desktop client requires a non-confidential secret even with PKCE; Microsoft's does not.
    pub fn google() -> Option<Self> {
        let client_id = registration("SUBROUTINE_GOOGLE_CLIENT_ID", DEFAULT_GOOGLE_CLIENT_ID)?;
        let client_secret = registration(
            "SUBROUTINE_GOOGLE_CLIENT_SECRET",
            DEFAULT_GOOGLE_CLIENT_SECRET,
        )?;
        Some(Self {
            label: "Google",
            auth_url: GOOGLE_AUTH_URL,
            token_url: GOOGLE_TOKEN_URL,
            scope: GOOGLE_SCOPE,
            extra_auth_params: &[("access_type", "offline"), ("prompt", "consent")],
            client_id,
            client_secret: Some(client_secret),
            credential: IntegrationCredential::Google,
        })
    }

    pub fn microsoft() -> Option<Self> {
        let client_id = registration(
            "SUBROUTINE_MICROSOFT_CLIENT_ID",
            DEFAULT_MICROSOFT_CLIENT_ID,
        )?;
        Some(Self {
            label: "Outlook / Microsoft 365",
            auth_url: MICROSOFT_AUTH_URL,
            token_url: MICROSOFT_TOKEN_URL,
            scope: MICROSOFT_SCOPE,
            extra_auth_params: &[],
            client_id,
            client_secret: None,
            credential: IntegrationCredential::Microsoft,
        })
    }

    fn client(
        &self,
        redirect: &str,
    ) -> Result<
        BasicClient<
            oauth2::EndpointSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointSet,
        >,
        String,
    > {
        let client = BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_auth_type(oauth2::AuthType::RequestBody)
            .set_auth_uri(AuthUrl::new(self.auth_url.to_owned()).map_err(stringify)?)
            .set_token_uri(TokenUrl::new(self.token_url.to_owned()).map_err(stringify)?)
            .set_redirect_uri(RedirectUrl::new(redirect.to_owned()).map_err(stringify)?);
        Ok(match &self.client_secret {
            Some(secret) => client.set_client_secret(ClientSecret::new(secret.clone())),
            None => client,
        })
    }
}

fn stringify(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn optional_env(name: &'static str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn registration(name: &'static str, compiled: Option<&str>) -> Option<String> {
    optional_env(name).or_else(|| {
        compiled
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

async fn token_request(
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, reqwest::Error> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let (parts, body) = request.into_parts();
    let response = client
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut result = oauth2::HttpResponse::new(response.bytes().await?.to_vec());
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    Ok(result)
}

fn blocking_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(stringify)
}

pub fn connect_blocking(provider: Option<OAuthProvider>) -> Result<(), String> {
    let provider = provider.ok_or_else(|| "Sign-in is not configured in this build.".to_owned())?;
    let credential = provider.credential;
    let _operation = credential.lock()?;
    let refresh_token = blocking_runtime()?.block_on(authorize(&provider, |url| {
        open::that(url).map_err(|error| format!("open system browser: {error}"))
    }))?;
    credential.save(&refresh_token)
}

pub fn access_token_blocking(provider: Option<OAuthProvider>) -> Result<String, String> {
    let provider = provider.ok_or_else(|| "Sign-in is not configured in this build.".to_owned())?;
    let _operation = provider.credential.lock()?;
    let refresh_token = provider
        .credential
        .load()?
        .ok_or_else(|| format!("{} is not connected.", provider.label))?;
    blocking_runtime()?.block_on(refresh_access_token(&provider, &refresh_token))
}

async fn authorize<F>(provider: &OAuthProvider, open_browser: F) -> Result<String, String>
where
    F: FnOnce(String) -> Result<(), String>,
{
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("bind OAuth callback listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read OAuth callback address: {error}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let client = provider.client(&redirect)?;

    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(provider.scope.to_owned()))
        .set_pkce_challenge(challenge);
    for (key, value) in provider.extra_auth_params {
        request = request.add_extra_param(*key, *value);
    }
    let (authorization_url, csrf) = request.url();

    open_browser(authorization_url.to_string())?;
    let callback = wait_for_callback(listener, csrf.secret()).await?;

    let response = client
        .exchange_code(AuthorizationCode::new(callback.code))
        .set_pkce_verifier(verifier)
        .request_async(&token_request)
        .await
        .map_err(|error| describe_token_error(provider.label, error))?;

    response
        .refresh_token()
        .map(|token| token.secret().to_owned())
        .ok_or_else(|| {
            format!(
                "{} returned no refresh token. Remove Subroutine from your account's \
                 third-party access and connect again.",
                provider.label
            )
        })
}

async fn refresh_access_token(
    provider: &OAuthProvider,
    refresh_token: &str,
) -> Result<String, String> {
    let client = provider.client(&format!("http://127.0.0.1:1{CALLBACK_PATH}"))?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_owned()))
        .request_async(&token_request)
        .await
        .map_err(|error| describe_token_error(provider.label, error))?;
    if let Some(rotated) = response.refresh_token() {
        provider.credential.save(rotated.secret())?;
    }
    Ok(response.access_token().secret().to_owned())
}

fn describe_token_error<E: std::error::Error + 'static>(
    label: &str,
    error: oauth2::RequestTokenError<
        E,
        oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    >,
) -> String {
    use oauth2::RequestTokenError;
    match &error {
        RequestTokenError::ServerResponse(response) => match response.error() {
            oauth2::basic::BasicErrorResponseType::InvalidGrant => {
                format!("{label} rejected the stored sign-in. Connect it again.")
            }
            other => {
                format!("{label} returned {other}. Check the app registration and account consent.")
            }
        },
        RequestTokenError::Request(_) => {
            format!("Could not reach {label}. Check your connection and try again.")
        }
        _ => format!("{label} returned an invalid token response. Try connecting again."),
    }
}

pub fn disconnect(credential: IntegrationCredential) -> Result<(), String> {
    let _operation = credential.lock()?;
    credential.delete()
}

pub fn google_calendars() -> Result<Vec<CalendarSource>, String> {
    let token = access_token_blocking(OAuthProvider::google())?;
    subroutine_import::providers::google_calendar::GoogleCalendarProvider::new(token)
        .list_calendars()
        .map_err(|error| format!("{error:#}"))
}

pub fn outlook_calendars() -> Result<Vec<CalendarSource>, String> {
    let token = access_token_blocking(OAuthProvider::microsoft())?;
    subroutine_import::providers::outlook::OutlookProvider::new(token)
        .list_calendars()
        .map_err(|error| format!("{error:#}"))
}

// Never derive Debug or log the live authorization code.
struct AuthorizationCallback {
    code: String,
}

async fn wait_for_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<AuthorizationCallback, String> {
    tokio::time::timeout(SIGN_IN_TIMEOUT, async {
        let (mut stream, peer) = listener.accept().await
            .map_err(|error| format!("accept OAuth callback: {error}"))?;
        if !peer.ip().is_loopback() {
            return Err("The sign-in callback was not from this device.".into());
        }
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).await
                .map_err(|error| format!("read OAuth callback: {error}"))?;
            if count == 0 || request.len() + count > 16 * 1024 {
                return Err("The sign-in callback was incomplete or too large.".into());
            }
            request.extend_from_slice(&chunk[..count]);
        }
        let parsed = parse_callback_request(&request, expected_state);
        let (status, message) = if parsed.is_ok() {
            ("200 OK", "Sign-in response received. Return to Subroutine Lite to finish connecting.")
        } else {
            ("400 Bad Request", "Subroutine Lite could not validate sign-in. Return to the app for details.")
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}", message.len());
        stream.write_all(response.as_bytes()).await
            .map_err(|error| format!("write OAuth callback response: {error}"))?;
        parsed
    }).await.map_err(|_| "Timed out waiting for browser sign-in. Try connecting again.".to_owned())?
}

fn parse_callback_request(
    request: &[u8],
    expected_state: &str,
) -> Result<AuthorizationCallback, String> {
    let request = std::str::from_utf8(request)
        .map_err(|_| "the OAuth callback was not valid HTTP".to_owned())?;
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| "the OAuth callback request was empty".to_owned())?;
    let mut parts = first_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err("the OAuth callback did not use GET".into());
    }
    let target = parts
        .next()
        .ok_or_else(|| "the OAuth callback had no request target".to_owned())?;
    if !target.starts_with('/') || target.starts_with("//") || target.contains('#') {
        return Err("The sign-in callback used an invalid request target.".into());
    }
    let url = oauth2::url::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| "The sign-in callback URL was invalid.".to_owned())?;
    if url.path() != CALLBACK_PATH {
        return Err("the OAuth callback used an unexpected path".into());
    }

    let mut query = HashMap::new();
    for (key, value) in url.query_pairs().into_owned() {
        if query.insert(key, value).is_some() {
            return Err("The sign-in callback repeated a parameter.".into());
        }
    }
    let state = query
        .get("state")
        .filter(|state| !state.is_empty())
        .ok_or_else(|| "The sign-in callback contained no state.".to_owned())?;
    if !secret_eq(state, expected_state) {
        return Err("The sign-in callback did not match this connection request.".into());
    }
    if let Some(error) = query.get("error") {
        return Err(if error == "access_denied" {
            "Sign-in was cancelled."
        } else {
            "The provider refused sign-in. Check account consent and the app registration."
        }
        .into());
    }
    Ok(AuthorizationCallback {
        code: query
            .get("code")
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| "the OAuth callback contained no authorization code".to_owned())?,
    })
}

fn secret_eq(left: &str, right: &str) -> bool {
    Sha256::digest(left.as_bytes()) == Sha256::digest(right.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_requires_matching_state_and_unambiguous_loopback_path() {
        let request = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let valid = request("/oauth/callback?code=authorization&state=expected");
        assert_eq!(
            parse_callback_request(valid.as_bytes(), "expected")
                .unwrap()
                .code,
            "authorization"
        );
        for target in [
            "/oauth/callback?code=authorization&state=wrong",
            "/oauth/callback?code=authorization",
            "/oauth/callback?code=authorization&state=expected&state=expected",
            "/oauth/callback?code=one&code=two&state=expected",
            "/other?code=authorization&state=expected",
            "//remote/oauth/callback?code=authorization&state=expected",
            "/oauth/callback?code=authorization&state=expected#fragment",
            "/oauth/callback?error=access_denied&state=expected",
        ] {
            assert!(parse_callback_request(request(target).as_bytes(), "expected").is_err());
        }
        assert!(
            parse_callback_request(
                b"POST /oauth/callback?code=x&state=expected HTTP/1.1",
                "expected"
            )
            .is_err()
        );
    }
}
