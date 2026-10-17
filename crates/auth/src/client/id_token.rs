use std::{fmt, time::Duration};

use openidconnect::{
    AccessToken, AccessTokenHash, AdditionalClaims, ClientId, IdToken, IssuerUrl,
    JwsSigningAlgorithm, Nonce, NonceVerifier,
    core::{
        CoreGenderClaim, CoreIdTokenVerifier, CoreJsonWebKeySet, CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{Config, Discovery};
use crate::http;

/// Raw, untrusted ID token and its nonce context, stored only with encrypted credentials.
/// No verified name is serialized: every use rechecks the signature and fresh UserInfo subject.
#[derive(Clone, Deserialize, Serialize)]
pub struct IdTokenSnapshot {
    token: Option<String>,
    nonce: NonceContext,
}

impl fmt::Debug for IdTokenSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdTokenSnapshot").finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) enum NonceContext {
    Code(String),
    Refresh(Option<String>),
}

impl NonceContext {
    fn verify(&self, actual: Option<&Nonce>) -> Result<(), String> {
        match (self, actual) {
            (Self::Code(expected), _) | (Self::Refresh(Some(expected)), Some(_)) => {
                (&Nonce::new(expected.clone())).verify(actual)
            }
            (Self::Refresh(_), None) => Ok(()),
            (Self::Refresh(None), Some(_)) => Err("unknown authorization nonce".into()),
        }
    }
}

pub(super) struct Verification {
    jwks_uri: Url,
    algorithms: Vec<CoreJwsSigningAlgorithm>,
}

impl Verification {
    pub(super) fn from_discovery(config: &Config, discovery: &Discovery) -> Option<Self> {
        let jwks_uri = config.issuer.endpoint(discovery.jwks_uri.as_str()?).ok()?;
        let algorithms: Vec<_> = discovery
            .id_token_signing_alg_values_supported
            .as_array()?
            .iter()
            .filter_map(|value| {
                serde_json::from_value::<CoreJwsSigningAlgorithm>(value.clone()).ok()
            })
            .filter(|alg| alg.key_type().is_some() && !alg.uses_shared_secret())
            .collect();
        (!algorithms.is_empty()).then_some(Self {
            jwks_uri,
            algorithms,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct DisplayClaims {
    #[serde(default)]
    custom: Value,
}

impl fmt::Debug for DisplayClaims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DisplayClaims").finish_non_exhaustive()
    }
}

impl AdditionalClaims for DisplayClaims {}

type DisplayToken = IdToken<
    DisplayClaims,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;

pub(super) struct PreparedIdToken<'a> {
    snapshot: &'a IdTokenSnapshot,
    verification: &'a Verification,
    token: DisplayToken,
    keys: CoreJsonWebKeySet,
}

impl PreparedIdToken<'_> {
    pub(super) fn display_name(
        self,
        config: &Config,
        access: &str,
        subject: &str,
    ) -> Option<String> {
        self.snapshot.verified_name(
            &self.token,
            config,
            self.verification,
            self.keys,
            access,
            subject,
        )
    }
}

impl IdTokenSnapshot {
    pub(super) fn new(token: Option<String>, nonce: NonceContext) -> Self {
        Self { token, nonce }
    }

    pub(super) fn nonce(&self) -> Option<&str> {
        match &self.nonce {
            NonceContext::Code(nonce) => Some(nonce),
            NonceContext::Refresh(nonce) => nonce.as_deref(),
        }
    }

    pub(super) async fn prepare<'a>(
        &'a self,
        client: &reqwest::Client,
        verification: &'a Verification,
    ) -> Option<PreparedIdToken<'a>> {
        let token = self.parse()?;
        let (status, _, bytes) = http::send(
            client
                .get(verification.jwks_uri.clone())
                .timeout(Duration::from_secs(1)),
        )
        .await
        .ok()?;
        if !status.is_success() {
            return None;
        }
        let keys = serde_json::from_slice(&bytes).ok()?;
        Some(PreparedIdToken {
            snapshot: self,
            verification,
            token,
            keys,
        })
    }

    fn parse(&self) -> Option<DisplayToken> {
        let token = self
            .token
            .as_ref()
            .filter(|token| token.len() <= 256 * 1024)?;
        serde_json::from_value(Value::String(token.clone())).ok()
    }

    fn verified_name(
        &self,
        token: &DisplayToken,
        config: &Config,
        verification: &Verification,
        keys: CoreJsonWebKeySet,
        access: &str,
        subject: &str,
    ) -> Option<String> {
        let now = chrono::Utc::now();
        let verifier = CoreIdTokenVerifier::new_public_client(
            ClientId::new(config.client_id.clone()),
            IssuerUrl::new(config.issuer.as_str().to_owned()).ok()?,
            keys,
        )
        .set_allowed_algs(verification.algorithms.clone())
        .set_issue_time_verifier_fn(move |issued| {
            if issued.timestamp() > 0 && issued <= now + chrono::Duration::seconds(60) {
                Ok(())
            } else {
                Err("invalid ID token issue time".into())
            }
        });
        let claims = token
            .claims(&verifier, |nonce: Option<&Nonce>| self.nonce.verify(nonce))
            .ok()?;
        // openidconnect 4 does not enforce azp itself. Its default audience policy also rejects
        // additional, untrusted audiences; an API access-token audience is never an ID audience.
        if claims.subject().as_str() != subject
            || subject.trim().is_empty()
            || subject.len() > 255
            || claims.issue_time() >= claims.expiration()
            || claims
                .authorized_party()
                .is_some_and(|party| party.as_str() != config.client_id)
            || (claims.audiences().len() > 1 && claims.authorized_party().is_none())
        {
            return None;
        }
        if let Some(actual) = claims.access_token_hash() {
            let expected = AccessTokenHash::from_token(
                &AccessToken::new(access.to_owned()),
                token.signing_alg().ok()?,
                token.signing_key(&verifier).ok()?,
            )
            .ok()?;
            if actual != &expected {
                return None;
            }
        }
        display_name(claims.additional_claims().custom.get("display_name")?)
    }
}

fn display_name(value: &Value) -> Option<String> {
    let name = value.as_str()?;
    if name.chars().count() > 80
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return None;
    }
    // ECMAScript trim also includes BOM, but not Unicode NEXT LINE (which is rejected above).
    let name = name.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests;
