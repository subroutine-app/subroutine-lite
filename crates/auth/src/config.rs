use std::net::IpAddr;

use url::{Host, Url};

use crate::ConfigError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransportSecurity {
    #[default]
    HttpsOnly,
    InsecureLoopback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issuer {
    value: String,
    url: Url,
    security: TransportSecurity,
}

impl Issuer {
    pub fn new(value: impl Into<String>) -> Result<Self, ConfigError> {
        Self::with_security(value, TransportSecurity::HttpsOnly)
    }

    pub fn with_security(
        value: impl Into<String>,
        security: TransportSecurity,
    ) -> Result<Self, ConfigError> {
        let value = value.into();
        let url = endpoint(&value, security)?;
        Ok(Self {
            value,
            url,
            security,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn discovery_url(&self) -> Url {
        let mut url = self.url.clone();
        let path = url.path().strip_suffix('/').unwrap_or(url.path());
        url.set_path(&format!("{path}/.well-known/openid-configuration"));
        url
    }

    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn endpoint(&self, value: &str) -> Result<Url, ConfigError> {
        let url = endpoint(value, self.security)?;
        if url.origin() != self.url.origin() {
            return Err(ConfigError::UnexpectedOrigin);
        }
        Ok(url)
    }
}

fn endpoint(value: &str, security: TransportSecurity) -> Result<Url, ConfigError> {
    if value.contains('\\') || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ConfigError::InvalidUrl);
    }
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(rest))
        .filter(|authority| !authority.is_empty() && !authority.contains('@'))
        .ok_or(ConfigError::InvalidUrl)?;
    let url = Url::parse(value).map_err(|_| ConfigError::InvalidUrl)?;
    if !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidUrl);
    }
    let secure = match url.scheme() {
        "https" => true,
        "http" if security == TransportSecurity::InsecureLoopback => {
            let host = if let Some(ipv6) = authority.strip_prefix('[') {
                ipv6.split_once(']').map(|(host, _)| host)
            } else {
                authority.split(':').next()
            };
            let literal = host.and_then(|host| host.parse::<IpAddr>().ok());
            let parsed = match url.host() {
                Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
                Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
                _ => None,
            };
            literal.is_some_and(|ip| ip.is_loopback()) && literal == parsed
        }
        _ => false,
    };
    if !secure {
        return Err(ConfigError::InsecureUrl);
    }
    Ok(url)
}

#[cfg(any(feature = "client", feature = "server"))]
pub(crate) fn validate_client_id(value: &str) -> Result<(), ConfigError> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ConfigError::InvalidClientId);
    }
    Ok(())
}

#[cfg(any(feature = "client", feature = "server"))]
pub(crate) fn validate_scopes(scopes: &[String]) -> Result<(), ConfigError> {
    if scopes.iter().any(|scope| {
        scope.is_empty()
            || !scope
                .bytes()
                .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
    }) {
        return Err(ConfigError::InvalidScope);
    }
    Ok(())
}
