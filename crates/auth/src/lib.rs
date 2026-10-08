#![forbid(unsafe_code)]

mod config;
mod error;
#[cfg(any(feature = "client", feature = "server"))]
mod http;

pub use config::{Issuer, TransportSecurity};
pub use error::{ConfigError, ProviderError};

#[cfg(any(feature = "client", feature = "server"))]
use config::{validate_client_id, validate_scopes};

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;
