
use std::{fmt, future::Future, pin::Pin, sync::Arc};

use oauth2::{
    AuthType, AuthUrl, ClientId, RedirectUrl, RefreshToken, TokenUrl, basic::BasicClient,
};
use serde::Deserialize;
use url::Url;

use crate::{ConfigError, Issuer, ProviderError, TransportSecurity, http};

mod callback;
mod error;
mod tokens;
mod userinfo;

pub use callback::{CallbackError, PendingAuthorization, ValidatedCode};
pub use error::{DiscoveryError, FailureKind, TokenError, TokenValidationError};
pub use tokens::Tokens;
pub use userinfo::UserInfo;

#[derive(Clone, Debug)]
pub struct Config {
    issuer: Issuer,
    client_id: String,
    redirect_uri: RedirectUrl,
    required_scopes: Vec<String>,
}

impl Config {
    pub fn new(
        issuer: Issuer,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
        required_scopes: Vec<String>,
    ) -> Result<Self, ConfigError> {
        let client_id = client_id.into();
        crate::validate_client_id(&client_id)?;
        crate::validate_scopes(&required_scopes)?;
        let redirect_uri = redirect_uri.into();
        validate_redirect(&redirect_uri)?;
        Ok(Self {
            issuer,
            client_id,
            redirect_uri: RedirectUrl::new(redirect_uri)
                .map_err(|_| ConfigError::InvalidRedirect)?,
            required_scopes,
        })
    }

    pub fn issuer(&self) -> &Issuer {
        &self.issuer
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn redirect_uri(&self) -> &str {
        self.redirect_uri.as_str()
    }

    pub fn required_scopes(&self) -> &[String] {
        &self.required_scopes
    }
}

fn validate_redirect(value: &str) -> Result<(), ConfigError> {
    let url = Url::parse(value).map_err(|_| ConfigError::InvalidRedirect)?;
    if value
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().is_empty()
    {
        return Err(ConfigError::InvalidRedirect);
    }
    match url.scheme() {
        "https" | "http" => {
            Issuer::with_security(value, TransportSecurity::InsecureLoopback)
                .map_err(|_| ConfigError::InvalidRedirect)?;
        }
        scheme
            if scheme.split('.').count() >= 2 && scheme.split('.').all(|part| !part.is_empty()) => {
        }
        _ => return Err(ConfigError::InvalidRedirect),
    }
    Ok(())
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    userinfo_endpoint: serde_json::Value,
    response_types_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
}

type OAuthClient = BasicClient<
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

struct ProviderInner {
    config: Config,
    client: OAuthClient,
    http: reqwest::Client,
    userinfo_endpoint: Option<Url>,
    require_iss: bool,
}

#[derive(Clone)]
pub struct Provider(Arc<ProviderInner>);

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider").finish_non_exhaustive()
    }
}

impl Provider {
    pub async fn discover(config: Config) -> Result<Self, DiscoveryError> {
        let http = http::client()?;
        let metadata: Discovery = http::document(&http, &config.issuer.discovery_url()).await?;
        if metadata.issuer != config.issuer.as_str() {
            return Err(ProviderError::IssuerMismatch.into());
        }
        if !metadata
            .response_types_supported
            .iter()
            .any(|value| value == "code")
            || !metadata
                .code_challenge_methods_supported
                .iter()
                .any(|value| value == "S256")
        {
            return Err(ProviderError::UnsupportedProvider.into());
        }
        let authorization = config.issuer.endpoint(&metadata.authorization_endpoint)?;
        let token = config.issuer.endpoint(&metadata.token_endpoint)?;
        let userinfo_endpoint = metadata
            .userinfo_endpoint
            .as_str()
            .and_then(|endpoint| config.issuer.endpoint(endpoint).ok());
        let client = BasicClient::new(ClientId::new(config.client_id.clone()))
            .set_auth_type(AuthType::RequestBody)
            .set_auth_uri(AuthUrl::from_url(authorization))
            .set_token_uri(TokenUrl::from_url(token))
            .set_redirect_uri(config.redirect_uri.clone());
        Ok(Self(Arc::new(ProviderInner {
            config,
            client,
            http,
            userinfo_endpoint,
            require_iss: metadata.authorization_response_iss_parameter_supported,
        })))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn authorize(&self) -> PendingAuthorization {
        PendingAuthorization::new(self.0.clone())
    }

    pub async fn refresh(&self, old: &Tokens) -> Result<Tokens, TokenError> {
        old.ensure_refresh_ready()?;
        let refresh = RefreshToken::new(old.refresh_token().to_owned());

        let started = chrono::Utc::now().timestamp();
        let response = self
            .0
            .client
            .exchange_refresh_token(&refresh)
            .request_async(self.0.as_ref())
            .await
            .map_err(error::token_error)?;
        let received = chrono::Utc::now().timestamp();
        Tokens::from_response(
            response,
            &self.0.config.required_scopes,
            started,
            received,
            Some(old.refresh_token()),
        )
        .map_err(TokenError::InvalidTokens)
    }
}

impl<'c> oauth2::AsyncHttpClient<'c> for ProviderInner {
    type Error = ProviderError;
    type Future =
        Pin<Box<dyn Future<Output = Result<oauth2::HttpResponse, Self::Error>> + Send + 'c>>;

    fn call(&'c self, request: oauth2::HttpRequest) -> Self::Future {
        Box::pin(self.send(request))
    }
}

impl ProviderInner {
    async fn send(
        &self,
        request: oauth2::HttpRequest,
    ) -> Result<oauth2::HttpResponse, ProviderError> {
        let (parts, body) = request.into_parts();
        let url = Url::parse(&parts.uri.to_string()).map_err(|_| ProviderError::InvalidDocument)?;
        let request = self
            .http
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body);
        let (status, headers, body) = http::send(request).await?;
        if status.is_redirection() {
            return Err(ProviderError::HttpStatus(status.as_u16()));
        }
        let mut response = oauth2::HttpResponse::new(body);
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        Ok(response)
    }
}
