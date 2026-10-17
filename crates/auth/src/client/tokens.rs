use std::fmt;

use oauth2::{TokenResponse, basic::BasicTokenType};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use subtle::ConstantTimeEq;

use super::{IdTokenSnapshot, TokenError, TokenValidationError, id_token::NonceContext};

pub(super) type Response = oauth2::StandardTokenResponse<ExtraFields, BasicTokenType>;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct ExtraFields {
    #[serde(default)]
    id_token: serde_json::Value,
}

impl oauth2::ExtraTokenFields for ExtraFields {}

impl fmt::Debug for ExtraFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtraFields").finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize)]
pub struct Tokens {
    access: String,
    refresh: String,
    expires_at: i64,
    refresh_not_before: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<IdTokenSnapshot>,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"[REDACTED]")
            .field("refresh", &"[REDACTED]")
            .field("id_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("refresh_not_before", &self.refresh_not_before)
            .finish()
    }
}

impl<'de> Deserialize<'de> for Tokens {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored {
            access: String,
            refresh: String,
            expires_at: i64,
            #[serde(default)]
            refresh_not_before: Option<i64>,
            #[serde(default)]
            id_token: serde_json::Value,
        }
        let stored = Stored::deserialize(deserializer)
            .map_err(|_| D::Error::custom("invalid stored tokens"))?;
        Self::from_stored(
            stored.access,
            stored.refresh,
            stored.expires_at,
            stored.refresh_not_before,
        )
        .map(|tokens| tokens.with_id_token_snapshot(serde_json::from_value(stored.id_token).ok()))
        .map_err(D::Error::custom)
    }
}

impl Tokens {
    pub fn from_stored(
        access: String,
        refresh: String,
        expires_at: i64,
        refresh_not_before: Option<i64>,
    ) -> Result<Self, TokenValidationError> {
        if !valid_secret(&access) {
            return Err(TokenValidationError::InvalidAccessToken);
        }
        if !valid_secret(&refresh) {
            return Err(TokenValidationError::InvalidRefreshToken);
        }
        if !valid_timestamp(expires_at)
            || refresh_not_before.is_some_and(|bound| {
                !valid_timestamp(bound) || bound < expires_at.saturating_sub(59)
            })
        {
            return Err(TokenValidationError::InvalidTimestamps);
        }
        Ok(Self {
            access,
            refresh,
            expires_at,
            refresh_not_before,
            id_token: None,
        })
    }

    pub(super) fn from_response(
        response: Response,
        required_scopes: &[String],
        started: i64,
        received: i64,
        old_refresh: Option<&str>,
        nonce: NonceContext,
    ) -> Result<Self, TokenValidationError> {
        if response.token_type() != &BasicTokenType::Bearer {
            return Err(TokenValidationError::UnsupportedTokenType);
        }
        if response.scopes().is_some_and(|scopes| {
            required_scopes
                .iter()
                .any(|required| !scopes.iter().any(|scope| scope.as_str() == required))
        }) {
            return Err(TokenValidationError::MissingScope);
        }
        let lifetime = response
            .expires_in()
            .filter(|duration| !duration.is_zero())
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .ok_or(TokenValidationError::InvalidLifetime)?;
        let expires_at = started
            .checked_add(lifetime)
            .filter(|time| valid_timestamp(*time))
            .ok_or(TokenValidationError::InvalidLifetime)?;
        let refresh_not_before = received
            .checked_add(lifetime)
            .and_then(|expiry| expiry.checked_sub(59))
            .ok_or(TokenValidationError::InvalidLifetime)?;
        if received < started {
            return Err(TokenValidationError::InvalidTimestamps);
        }
        let refresh = response
            .refresh_token()
            .ok_or(TokenValidationError::InvalidRefreshToken)?
            .secret();
        if old_refresh.is_some_and(|old| bool::from(old.as_bytes().ct_eq(refresh.as_bytes()))) {
            return Err(TokenValidationError::UnchangedRefreshToken);
        }
        Self::from_stored(
            response.access_token().secret().clone(),
            refresh.clone(),
            expires_at,
            Some(refresh_not_before),
        )
        .map(|tokens| {
            tokens.with_id_token_snapshot(Some(IdTokenSnapshot::new(
                response.extra_fields().id_token.as_str().map(str::to_owned),
                nonce,
            )))
        })
    }

    pub fn id_token_snapshot(&self) -> Option<&IdTokenSnapshot> {
        self.id_token.as_ref()
    }

    /// Restores untrusted token metadata; names are verified against fresh UserInfo before use.
    pub fn with_id_token_snapshot(mut self, snapshot: Option<IdTokenSnapshot>) -> Self {
        self.id_token = snapshot;
        self
    }

    pub fn access_token(&self) -> &str {
        &self.access
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub fn refresh_not_before(&self) -> Option<i64> {
        self.refresh_not_before
    }

    pub fn refresh_ready_at(&self) -> i64 {
        self.refresh_not_before.unwrap_or(self.expires_at)
    }

    pub fn is_fresh(&self) -> bool {
        self.expires_at > chrono::Utc::now().timestamp().saturating_add(15)
    }

    pub fn can_refresh(&self) -> bool {
        chrono::Utc::now().timestamp() >= self.refresh_ready_at()
    }

    pub fn ensure_refresh_ready(&self) -> Result<(), TokenError> {
        if self.can_refresh() {
            Ok(())
        } else {
            Err(TokenError::NotReady {
                not_before: self.refresh_ready_at(),
            })
        }
    }
}

fn valid_secret(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256 * 1024
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_timestamp(value: i64) -> bool {
    value > 0 && chrono::DateTime::from_timestamp(value, 0).is_some()
}
