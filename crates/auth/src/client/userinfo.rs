use std::fmt;

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
