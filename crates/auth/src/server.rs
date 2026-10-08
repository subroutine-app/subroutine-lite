
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use jsonwebtoken::jwk::{AlgorithmParameters, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header, get_current_timestamp,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::{ConfigError, Issuer, ProviderError, http, validate_client_id, validate_scopes};

const KEY_TTL: Duration = Duration::from_secs(300);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Config {
    issuer: Issuer,
    clients: HashSet<String>,
    required_scopes: Vec<String>,
}

impl Config {
    pub fn new(
        issuer: Issuer,
        clients: impl IntoIterator<Item = String>,
        required_scopes: impl IntoIterator<Item = String>,
    ) -> Result<Self, ConfigError> {
        let clients: HashSet<_> = clients.into_iter().collect();
        if clients.is_empty() {
            return Err(ConfigError::InvalidClientId);
        }
        for client in &clients {
            validate_client_id(client)?;
        }
        let required_scopes: Vec<_> = required_scopes.into_iter().collect();
        validate_scopes(&required_scopes)?;
        Ok(Self {
            issuer,
            clients,
            required_scopes,
        })
    }

    pub fn issuer(&self) -> &Issuer {
        &self.issuer
    }

    pub fn clients(&self) -> impl Iterator<Item = &str> {
        self.clients.iter().map(String::as_str)
    }

    pub fn required_scopes(&self) -> &[String] {
        &self.required_scopes
    }
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
}

struct Keys {
    values: HashMap<String, DecodingKey>,
    fetched_at: Instant,
    last_attempt: Option<(Instant, RefreshOutcome)>,
}

enum RefreshOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone)]
pub struct Verifier(Arc<Inner>);

struct Inner {
    config: Config,
    http: reqwest::Client,
    jwks: Url,
    keys: Mutex<Keys>,
}

pub struct VerifiedToken {
    issuer: String,
    subject: String,
    deadline: tokio::time::Instant,
}

impl VerifiedToken {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn deadline(&self) -> tokio::time::Instant {
        self.deadline
    }

    pub fn into_parts(self) -> (String, String, tokio::time::Instant) {
        (self.issuer, self.subject, self.deadline)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("invalid access token")]
    InvalidToken,
    #[error("insufficient access-token scope")]
    InsufficientScope,
    #[error("access-token verification unavailable")]
    Unavailable,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    azp: String,
    typ: String,
    scope: String,
    exp: u64,
    iat: u64,
    nbf: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, client: &str) -> bool {
        match self {
            Self::One(audience) => audience == client,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == client),
        }
    }
}

impl Verifier {
    pub async fn discover(config: Config) -> Result<Self, ProviderError> {
        let http = http::client()?;
        let discovery: Discovery = http::document(&http, &config.issuer.discovery_url()).await?;
        if discovery.issuer != config.issuer.as_str() {
            return Err(ProviderError::IssuerMismatch);
        }
        let jwks = config
            .issuer
            .endpoint(&discovery.jwks_uri)
            .map_err(|_| ProviderError::UnsupportedProvider)?;
        let values = fetch_keys(&http, &jwks).await?;
        Ok(Self(Arc::new(Inner {
            config,
            http,
            jwks,
            keys: Mutex::new(Keys {
                values,
                fetched_at: Instant::now(),
                last_attempt: None,
            }),
        })))
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, VerifyError> {
        let mut keys = self.0.keys.lock().await;
        if keys.fetched_at.elapsed() < KEY_TTL
            && let Some(key) = keys.values.get(kid)
        {
            return Ok(key.clone());
        }
        if let Some((attempt, outcome)) = &keys.last_attempt
            && attempt.elapsed() < REFRESH_INTERVAL
        {
            return Err(
                if matches!(outcome, RefreshOutcome::Succeeded)
                    && keys.fetched_at.elapsed() < KEY_TTL
                {
                    VerifyError::InvalidToken
                } else {
                    VerifyError::Unavailable
                },
            );
        }
        match fetch_keys(&self.0.http, &self.0.jwks).await {
            Ok(values) => {
                let now = Instant::now();
                keys.last_attempt = Some((now, RefreshOutcome::Succeeded));
                keys.values = values;
                keys.fetched_at = now;
                keys.values
                    .get(kid)
                    .cloned()
                    .ok_or(VerifyError::InvalidToken)
            }
            Err(_) => {
                keys.last_attempt = Some((Instant::now(), RefreshOutcome::Failed));
                Err(VerifyError::Unavailable)
            }
        }
    }

    pub async fn verify(&self, token: &str) -> Result<VerifiedToken, VerifyError> {
        if token.len() > 16 * 1024 {
            return Err(VerifyError::InvalidToken);
        }
        let header = decode_header(token).map_err(|_| VerifyError::InvalidToken)?;
        if header.alg != Algorithm::RS256 || header.typ.as_deref() != Some("at+jwt") {
            return Err(VerifyError::InvalidToken);
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty() && kid.len() <= 256)
            .ok_or(VerifyError::InvalidToken)?;
        let key = self.key(&kid).await?;
        self.verify_with_key(token, &key)
    }

    fn verify_with_key(
        &self,
        token: &str,
        key: &DecodingKey,
    ) -> Result<VerifiedToken, VerifyError> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["exp", "nbf", "iss", "sub", "aud"]);
        validation.set_issuer(&[self.0.config.issuer.as_str()]);
        validation.set_audience(&self.0.config.clients.iter().collect::<Vec<_>>());
        let claims = decode::<Claims>(token, key, &validation)
            .map_err(|_| VerifyError::InvalidToken)?
            .claims;
        let now = get_current_timestamp();
        if claims.sub.trim().is_empty()
            || claims.typ != "Bearer"
            || !self.0.config.clients.contains(&claims.azp)
            || !claims.aud.contains(&claims.azp)
            || claims.exp <= now
            || claims.iat > now
            || claims.exp <= claims.iat
            || claims.nbf > claims.exp
        {
            return Err(VerifyError::InvalidToken);
        }
        if !self
            .0
            .config
            .required_scopes
            .iter()
            .all(|required| claims.scope.split(' ').any(|scope| scope == required))
        {
            return Err(VerifyError::InsufficientScope);
        }
        let monotonic_now = tokio::time::Instant::now();
        let remaining = std::time::UNIX_EPOCH
            .checked_add(Duration::from_secs(claims.exp))
            .and_then(|expiry| expiry.duration_since(std::time::SystemTime::now()).ok())
            .ok_or(VerifyError::InvalidToken)?;
        let deadline = monotonic_now
            .checked_add(remaining)
            .ok_or(VerifyError::InvalidToken)?;
        Ok(VerifiedToken {
            issuer: claims.iss,
            subject: claims.sub,
            deadline,
        })
    }
}

async fn fetch_keys(
    http: &reqwest::Client,
    url: &Url,
) -> Result<HashMap<String, DecodingKey>, ProviderError> {
    let set: JwkSet = http::document(http, url).await?;
    let mut keys = HashMap::new();
    for jwk in set.keys {
        if jwk.common.key_algorithm != Some(KeyAlgorithm::RS256)
            || !matches!(jwk.algorithm, AlgorithmParameters::RSA(_))
            || jwk
                .common
                .public_key_use
                .as_ref()
                .is_some_and(|usage| *usage != PublicKeyUse::Signature)
            || jwk
                .common
                .key_operations
                .as_ref()
                .is_some_and(|ops| !ops.contains(&KeyOperations::Verify))
        {
            continue;
        }
        let kid = jwk
            .common
            .key_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or(ProviderError::InvalidSigningKeys)?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| ProviderError::InvalidSigningKeys)?;
        if keys.insert(kid.clone(), key).is_some() {
            return Err(ProviderError::InvalidSigningKeys);
        }
    }
    if keys.is_empty() {
        return Err(ProviderError::InvalidSigningKeys);
    }
    Ok(keys)
}
