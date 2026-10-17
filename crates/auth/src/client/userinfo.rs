use std::{fmt, time::Duration};

use tokio::time::{Instant, timeout_at};

use serde::Deserialize;

use super::{Provider, Tokens};
use crate::{ProviderError, http};

#[derive(Clone)]
pub struct UserInfo {
    subject: String,
    name: Option<String>,
}

impl fmt::Debug for UserInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserInfo").finish_non_exhaustive()
    }
}

impl UserInfo {
    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl Provider {
    pub async fn userinfo(&self, tokens: &Tokens) -> Result<UserInfo, ProviderError> {
        self.userinfo_with_timeout(tokens, Duration::from_secs(20))
            .await
    }

    /// Shares one budget between required UserInfo and optional ID-token enrichment.
    /// Enrichment timing out preserves completed UserInfo; UserInfo timing out is an error.
    pub async fn userinfo_with_timeout(
        &self,
        tokens: &Tokens,
        budget: Duration,
    ) -> Result<UserInfo, ProviderError> {
        if budget.is_zero() {
            return Err(ProviderError::Request);
        }
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or(ProviderError::Request)?;
        let identity = async {
            let snapshot = tokens.id_token_snapshot()?;
            let verification = self.0.identity.as_ref()?;
            snapshot.prepare(&self.0.http, verification).await
        };
        let (info, identity) = tokio::join!(
            timeout_at(deadline, self.fetch_userinfo(tokens)),
            timeout_at(deadline, identity),
        );
        let info = info.map_err(|_| ProviderError::Request)??;
        let name = identity.ok().flatten().and_then(|identity| {
            identity.display_name(&self.0.config, tokens.access_token(), &info.subject)
        });
        Ok(UserInfo {
            subject: info.subject,
            name: name.or(info.name),
        })
    }

    async fn fetch_userinfo(&self, tokens: &Tokens) -> Result<UserInfo, ProviderError> {
        let endpoint = self
            .0
            .userinfo_endpoint
            .as_ref()
            .ok_or(ProviderError::UnsupportedProvider)?;
        let request = self
            .0
            .http
            .get(endpoint.clone())
            .bearer_auth(tokens.access_token());
        let (status, _, bytes) = http::send(request).await?;
        if !status.is_success() {
            return Err(ProviderError::HttpStatus(status.as_u16()));
        }
        #[derive(Deserialize)]
        struct Claims {
            sub: String,
            name: Option<String>,
        }
        let claims: Claims =
            serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidDocument)?;
        if claims.sub.trim().is_empty() || claims.sub.len() > 255 {
            return Err(ProviderError::InvalidDocument);
        }
        Ok(UserInfo {
            subject: claims.sub,
            name: claims.name,
        })
    }
}
