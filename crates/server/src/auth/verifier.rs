use anyhow::{Context as _, Result};
use subroutine_auth::server::VerifyError;

use super::{AuthError, AuthenticatedIdentity, TokenDeadline, config::Config};

#[derive(Clone)]
pub(crate) struct Verifier(subroutine_auth::server::Verifier);

pub(super) struct VerifiedToken {
    pub identity: AuthenticatedIdentity,
    pub deadline: TokenDeadline,
}

impl Verifier {
    pub(crate) async fn from_env() -> Result<Self> {
        Self::discover(Config::from_env()?).await
    }

    async fn discover(config: Config) -> Result<Self> {
        subroutine_auth::server::Verifier::discover(config.0)
            .await
            .map(Self)
            .context("OIDC discovery or signing keys unavailable")
    }

    pub(super) async fn verify(&self, token: &str) -> Result<VerifiedToken, AuthError> {
        let verified = self.0.verify(token).await.map_err(|error| match error {
            VerifyError::InvalidToken => AuthError::InvalidToken,
            VerifyError::InsufficientScope => AuthError::InsufficientScope,
            VerifyError::Unavailable => AuthError::Unavailable,
        })?;
        let (issuer, subject, deadline) = verified.into_parts();
        Ok(VerifiedToken {
            identity: AuthenticatedIdentity { issuer, subject },
            deadline: TokenDeadline(deadline),
        })
    }
}
