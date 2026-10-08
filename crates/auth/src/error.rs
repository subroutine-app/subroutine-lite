#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("invalid authentication URL")]
    InvalidUrl,
    #[error("authentication requires HTTPS or explicitly allowed literal-loopback HTTP")]
    InsecureUrl,
    #[error("provider endpoints must share the issuer origin")]
    UnexpectedOrigin,
    #[error("client IDs must be nonempty and contain no whitespace or control characters")]
    InvalidClientId,
    #[error("scopes must be nonempty OAuth scope tokens")]
    InvalidScope,
    #[error("invalid native-client redirect URI")]
    InvalidRedirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    #[error("could not initialize the authentication HTTP client")]
    HttpClient,
    #[error("provider request failed")]
    Request,
    #[error("provider returned HTTP {0}")]
    HttpStatus(u16),
    #[error("provider response exceeds the size limit")]
    DocumentTooLarge,
    #[error("invalid provider document")]
    InvalidDocument,
    #[error("discovery issuer does not exactly match the configured issuer")]
    IssuerMismatch,
    #[error("unsupported provider configuration")]
    UnsupportedProvider,
    #[error("invalid or unavailable RS256 signing keys")]
    InvalidSigningKeys,
}
