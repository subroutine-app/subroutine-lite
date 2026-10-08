use std::{collections::HashMap, fmt, sync::Arc};

use oauth2::{AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, Scope};
use subtle::ConstantTimeEq;
use url::Url;

use super::{ProviderInner, TokenError, Tokens, error};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CallbackError {
    #[error("invalid callback query")]
    InvalidQuery,
    #[error("callback state mismatch")]
    StateMismatch,
    #[error("callback issuer mismatch")]
    IssuerMismatch,
    #[error("authorization denied")]
    Denied,
    #[error("authorization transaction is closed")]
    Closed,
}

enum AuthorizationState {
    Waiting {
        state: CsrfToken,
        verifier: PkceCodeVerifier,
    },
    Denied,
}

#[must_use]
pub struct PendingAuthorization {
    provider: Arc<ProviderInner>,
    transaction: Arc<()>,
    url: Url,
    state: AuthorizationState,
}

impl fmt::Debug for PendingAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingAuthorization")
            .finish_non_exhaustive()
    }
}

#[must_use]
pub struct ValidatedCode {
    transaction: Arc<()>,
    code: AuthorizationCode,
}

impl fmt::Debug for ValidatedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedCode").finish_non_exhaustive()
    }
}

impl PendingAuthorization {
    pub(super) fn new(provider: Arc<ProviderInner>) -> Self {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = provider
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scopes(
                provider
                    .config
                    .required_scopes
                    .iter()
                    .cloned()
                    .map(Scope::new),
            )
            .set_pkce_challenge(challenge)
            .url();
        Self {
            provider,
            transaction: Arc::new(()),
            url,
            state: AuthorizationState::Waiting { state, verifier },
        }
    }

    pub fn authorization_url(&self) -> &Url {
        &self.url
    }

    pub fn validate_callback(&mut self, query: &str) -> Result<ValidatedCode, CallbackError> {
        let AuthorizationState::Waiting { state, .. } = &self.state else {
            return Err(CallbackError::Closed);
        };
        let params = query_parameters(query)?;
        let actual_state = params.get("state").ok_or(CallbackError::StateMismatch)?;
        if !bool::from(actual_state.as_bytes().ct_eq(state.secret().as_bytes())) {
            return Err(CallbackError::StateMismatch);
        }
        match params.get("iss") {
            Some(issuer) if issuer != self.provider.config.issuer.as_str() => {
                return Err(CallbackError::IssuerMismatch);
            }
            None if self.provider.require_iss => return Err(CallbackError::IssuerMismatch),
            _ => {}
        }
        match (params.get("code"), params.get("error")) {
            (Some(code), None) if !code.is_empty() => Ok(ValidatedCode {
                transaction: self.transaction.clone(),
                code: AuthorizationCode::new(code.clone()),
            }),
            (None, Some(error)) if !error.is_empty() => {
                self.state = AuthorizationState::Denied;
                Err(CallbackError::Denied)
            }
            _ => Err(CallbackError::InvalidQuery),
        }
    }

    pub async fn exchange(self, code: ValidatedCode) -> Result<Tokens, TokenError> {
        if !Arc::ptr_eq(&self.transaction, &code.transaction) {
            return Err(TokenError::TransactionMismatch);
        }
        let AuthorizationState::Waiting { verifier, .. } = self.state else {
            return Err(TokenError::AuthorizationClosed);
        };

        let started = chrono::Utc::now().timestamp();
        let response = self
            .provider
            .client
            .exchange_code(code.code)
            .set_pkce_verifier(verifier)
            .request_async(self.provider.as_ref())
            .await
            .map_err(error::token_error)?;
        let received = chrono::Utc::now().timestamp();
        Tokens::from_response(
            response,
            &self.provider.config.required_scopes,
            started,
            received,
            None,
        )
        .map_err(TokenError::InvalidTokens)
    }
}

fn query_parameters(query: &str) -> Result<HashMap<String, String>, CallbackError> {
    if query.len() > 8192
        || query.starts_with('?')
        || query.contains('#')
        || query.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(CallbackError::InvalidQuery);
    }
    let mut parameters = HashMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(CallbackError::InvalidQuery)?;
        let key = decode_component(key)?;
        let value = decode_component(value)?;
        if key.is_empty() || parameters.insert(key, value).is_some() {
            return Err(CallbackError::InvalidQuery);
        }
    }
    Ok(parameters)
}

fn decode_component(value: &str) -> Result<String, CallbackError> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = bytes.next().and_then(|byte| char::from(byte).to_digit(16));
                let low = bytes.next().and_then(|byte| char::from(byte).to_digit(16));
                match (high, low) {
                    (Some(high), Some(low)) => (high * 16 + low) as u8,
                    _ => return Err(CallbackError::InvalidQuery),
                }
            }
            byte => byte,
        });
    }
    let decoded = String::from_utf8(decoded).map_err(|_| CallbackError::InvalidQuery)?;
    if decoded.chars().any(char::is_control) {
        return Err(CallbackError::InvalidQuery);
    }
    Ok(decoded)
}
