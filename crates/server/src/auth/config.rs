use anyhow::{Context as _, Result};
use subroutine_auth::{Issuer, TransportSecurity};

pub(super) struct Config(pub(super) subroutine_auth::server::Config);

impl Config {
    pub fn from_env() -> Result<Self> {
        let issuer = std::env::var("SUBROUTINE_LITE_OIDC_ISSUER")
            .context("SUBROUTINE_LITE_OIDC_ISSUER is required")?;
        let clients = std::env::var("SUBROUTINE_LITE_OIDC_CLIENT_IDS")
            .context("SUBROUTINE_LITE_OIDC_CLIENT_IDS must list the allowed native client IDs")?;
        let allow_loopback = match std::env::var("SUBROUTINE_LITE_OIDC_ALLOW_INSECURE_LOOPBACK") {
            Ok(value) if value == "true" => true,
            Ok(value) if value == "false" => false,
            Err(std::env::VarError::NotPresent) => false,
            _ => {
                anyhow::bail!("SUBROUTINE_LITE_OIDC_ALLOW_INSECURE_LOOPBACK must be true or false")
            }
        };
        Self::parse(&issuer, &clients, allow_loopback)
    }

    pub fn parse(issuer: &str, clients: &str, allow_loopback: bool) -> Result<Self> {
        let security = if allow_loopback {
            TransportSecurity::InsecureLoopback
        } else {
            TransportSecurity::HttpsOnly
        };
        let issuer = Issuer::with_security(issuer, security)
            .context("Invalid SUBROUTINE_LITE_OIDC_ISSUER")?;
        subroutine_auth::server::Config::new(
            issuer,
            clients.split(',').map(|client| client.trim().to_owned()),
            ["subroutine:sync".to_owned()],
        )
        .map(Self)
        .context("SUBROUTINE_LITE_OIDC_CLIENT_IDS must contain nonempty comma-separated client IDs")
    }
}
