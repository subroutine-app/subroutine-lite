use crate::{ConfigError, ProviderError};

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("invalid provider endpoint: {0}")]
    Endpoint(#[from] ConfigError),
    #[error("provider discovery failed: {0}")]
    Provider(#[from] ProviderError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Terminal,
    Transient,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("refresh token is not yet eligible")]
    NotReady {
        not_before: i64,
    },
    #[error("authorization transaction mismatch")]
    TransactionMismatch,
    #[error("authorization transaction is closed")]
    AuthorizationClosed,
    #[error("credential rejected (invalid_grant)")]
    InvalidGrant,
    #[error("token request refused")]
    Refused,
    #[error("token endpoint temporarily unavailable")]
    TemporarilyUnavailable,
    #[error("token request outcome is unknown")]
    Request,
    #[error("token response too large")]
    DocumentTooLarge,
    #[error("unexpected token endpoint HTTP status {0}")]
    HttpStatus(u16),
    #[error("invalid token response")]
    InvalidResponse,
    #[error("invalid issued tokens: {0}")]
    InvalidTokens(TokenValidationError),
}

impl TokenError {
    pub fn kind(&self) -> FailureKind {
        match self {
            Self::NotReady { .. } | Self::TemporarilyUnavailable => FailureKind::Transient,
            Self::TransactionMismatch
            | Self::AuthorizationClosed
            | Self::InvalidGrant
            | Self::Refused => FailureKind::Terminal,
            Self::Request
            | Self::DocumentTooLarge
            | Self::HttpStatus(_)
            | Self::InvalidResponse
            | Self::InvalidTokens(_) => FailureKind::Indeterminate,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenValidationError {
    #[error("unsupported token type")]
    UnsupportedTokenType,
    #[error("required scope not granted")]
    MissingScope,
    #[error("invalid access token lifetime")]
    InvalidLifetime,
    #[error("invalid access token")]
    InvalidAccessToken,
    #[error("missing or invalid refresh token")]
    InvalidRefreshToken,
    #[error("refresh token did not rotate")]
    UnchangedRefreshToken,
    #[error("invalid token timestamps")]
    InvalidTimestamps,
}

pub(super) fn token_error(
    error: oauth2::RequestTokenError<ProviderError, oauth2::basic::BasicErrorResponse>,
) -> TokenError {
    use oauth2::{RequestTokenError, basic::BasicErrorResponseType};
    match error {
        RequestTokenError::ServerResponse(error) => match error.error() {
            BasicErrorResponseType::InvalidGrant => TokenError::InvalidGrant,
            BasicErrorResponseType::Extension(code)
                if matches!(code.as_str(), "server_error" | "temporarily_unavailable") =>
            {
                TokenError::TemporarilyUnavailable
            }
            _ => TokenError::Refused,
        },
        RequestTokenError::Request(ProviderError::DocumentTooLarge) => TokenError::DocumentTooLarge,
        RequestTokenError::Request(ProviderError::HttpStatus(status)) => {
            TokenError::HttpStatus(status)
        }
        RequestTokenError::Request(_) => TokenError::Request,
        _ => TokenError::InvalidResponse,
    }
}
