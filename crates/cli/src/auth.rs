use std::{fs::File, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::DateTime;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subroutine_auth::{
    Issuer, TransportSecurity,
    client::{self, CallbackError, FailureKind, PendingAuthorization, Tokens, ValidatedCode},
};
use subroutine_core::AccountInfo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;
use uuid::Uuid;

use crate::{
    args::LoginArgs,
    backend::{private_directory, private_file},
};

const SERVICE: &str = "com.subroutine.SubroutineLite.cli";
const CALLBACK_HOST: &str = "127.0.0.1:43119";
const CALLBACK: &str = "http://127.0.0.1:43119/oidc/callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize, Deserialize)]
struct Session {
    issuer: String,
    client_id: String,
    account_id: Uuid,
    tokens: Tokens,
}

pub async fn login(server: &Url, args: &LoginArgs, timeout: Duration) -> Result<Value> {
    let provider = provider(&args.issuer, &args.client_id).await?;
    let listener = TcpListener::bind(CALLBACK_HOST).await.with_context(|| {
        format!("listen on {CALLBACK_HOST}; another sign-in may be in progress")
    })?;
    let mut pending = provider.authorize();
    let url = pending.authorization_url().as_str();
    eprintln!("Sign in at: {url}");
    if !args.no_browser
        && let Err(error) = open::that(url)
    {
        eprintln!("could not open a browser ({error}); open the URL above");
    }
    let code = tokio::time::timeout(LOGIN_TIMEOUT, callback(&listener, &mut pending))
        .await
        .map_err(|_| anyhow!("sign-in timed out after five minutes"))??;
    let tokens = pending
        .exchange(code)
        .await
        .context("exchange the authorization code")?
        .with_id_token_snapshot(None);
    let account = account(server, &tokens, timeout).await?;
    let session = Session {
        issuer: args.issuer.clone(),
        client_id: args.client_id.clone(),
        account_id: account.account_id,
        tokens,
    };
    let _lock = lock()?;
    save(server, &session)?;
    Ok(json!({
        "server": origin(server),
        "account_id": account.account_id,
        "dataset_id": account.dataset_id,
        "expires_at": DateTime::from_timestamp(session.tokens.expires_at(), 0),
    }))
}

pub fn logout(server: &Url) -> Result<Value> {
    let _lock = lock()?;
    let removed = match entry(server)?.delete_credential() {
        Ok(()) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(error) => return Err(error).context("remove the saved login"),
    };
    Ok(json!({ "server": origin(server), "removed": removed }))
}

pub async fn saved_token(server: &Url) -> Result<Option<String>> {
    let _lock = lock()?;
    let Some(mut session) = load(server)? else {
        return Ok(None);
    };
    if session.tokens.can_refresh() {
        match refresh(&session).await {
            Ok(tokens) => {
                session.tokens = tokens.with_id_token_snapshot(None);
                save(server, &session)?;
            }
            Err(error) if error.kind() == FailureKind::Transient && session.tokens.is_fresh() => {
                eprintln!(
                    "warning: could not refresh the saved login ({error}); using the current token"
                );
            }
            Err(error) if error.kind() == FailureKind::Transient => {
                bail!("could not refresh the saved login ({error}); try again shortly")
            }
            Err(error) => {
                let _ = entry(server)?.delete_credential();
                bail!("the saved login is no longer valid ({error}); run `sr login` again")
            }
        }
    }
    Ok(Some(session.tokens.access_token().to_owned()))
}

async fn provider(issuer: &str, client_id: &str) -> Result<client::Provider> {
    let issuer = Issuer::with_security(issuer, TransportSecurity::InsecureLoopback)
        .context("invalid issuer")?;
    let config = client::Config::new(
        issuer,
        client_id,
        CALLBACK,
        vec!["openid".into(), "subroutine:sync".into()],
    )
    .context("invalid login configuration")?;
    client::Provider::discover(config)
        .await
        .context("discover the identity provider")
}

async fn refresh(session: &Session) -> Result<Tokens, client::TokenError> {
    let provider = provider(&session.issuer, &session.client_id)
        .await
        .map_err(|_| client::TokenError::TemporarilyUnavailable)?;
    provider.refresh(&session.tokens).await
}

async fn callback(
    listener: &TcpListener,
    pending: &mut PendingAuthorization,
) -> Result<ValidatedCode> {
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .context("accept the sign-in callback")?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let outcome = tokio::time::timeout(Duration::from_secs(2), callback_query(&mut stream))
            .await
            .ok()
            .flatten()
            .map(|query| pending.validate_callback(&query));
        let (status, message) = match &outcome {
            Some(Ok(_)) => ("200 OK", "Signed in. Return to the terminal."),
            Some(Err(CallbackError::Denied | CallbackError::Closed)) => {
                ("200 OK", "Sign-in was refused. Return to the terminal.")
            }
            _ => ("400 Bad Request", "Invalid sign-in callback."),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}",
            message.len()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            stream.write_all(response.as_bytes()),
        )
        .await;
        match outcome {
            Some(Ok(code)) => return Ok(code),
            Some(Err(CallbackError::Denied | CallbackError::Closed)) => {
                bail!("sign-in was refused or cancelled in the browser")
            }
            _ => {}
        }
    }
}

async fn callback_query(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        if bytes.len() >= 8192 {
            return None;
        }
        let len = stream.read(&mut chunk).await.ok().filter(|&len| len > 0)?;
        bytes.extend_from_slice(&chunk[..len]);
    }
    let request = std::str::from_utf8(&bytes).ok()?;
    let mut lines = request.split("\r\n");
    let ["GET", target, "HTTP/1.1"] = lines.next()?.split(' ').collect::<Vec<_>>()[..] else {
        return None;
    };
    let query = target.strip_prefix("/oidc/callback?")?;
    let mut hosts = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"));
    let host = hosts.next()?.1.trim();
    (host == CALLBACK_HOST && hosts.next().is_none()).then(|| query.to_owned())
}

async fn account(server: &Url, tokens: &Tokens, timeout: Duration) -> Result<AccountInfo> {
    let response = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .https_only(server.scheme() == "https")
        .build()
        .context("build HTTP client")?
        .get(server.join("v1/account")?)
        .bearer_auth(tokens.access_token())
        .send()
        .await
        .map_err(|error| anyhow!("verify the account: {}", error.without_url()))?;
    let status = response.status();
    ensure!(
        status.is_success(),
        "the server refused the new login (HTTP {status}); check that it accepts this issuer and client ID"
    );
    let account: AccountInfo = response.json().await.context("decode the account")?;
    ensure!(
        !account.account_id.is_nil() && !account.dataset_id.is_nil(),
        "the server returned an empty account identity"
    );
    Ok(account)
}

fn load(server: &Url) -> Result<Option<Session>> {
    match entry(server)?.get_secret() {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .context("the saved login is corrupt; run `sr logout` and `sr login`"),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("read the saved login from the credential store"),
    }
}

fn save(server: &Url, session: &Session) -> Result<()> {
    entry(server)?
        .set_secret(&serde_json::to_vec(session)?)
        .context("save the login to the credential store")
}

fn entry(server: &Url) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &origin(server)).context("open the credential store")
}

fn origin(server: &Url) -> String {
    server.origin().ascii_serialization()
}

fn lock() -> Result<File> {
    let directory = dirs::data_local_dir()
        .context("cannot determine the local data directory")?
        .join("subroutine-lite");
    private_directory(&directory)?;
    let file = private_file(&directory.join("cli-auth.lock"))?;
    file.lock().context("lock the saved login")?;
    Ok(file)
}
