use serde::{Deserialize, Serialize};

use super::protocol::{Config, Provider, Tokens};
use std::time::Duration;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Profile {
    pub subject: String,
    pub name: Option<String>,
}

impl Profile {
    pub fn reconcile(cached: Option<Self>, fetched: Option<Self>) -> Option<Self> {
        fetched
            .filter(|profile| {
                cached
                    .as_ref()
                    .is_none_or(|old| old.subject == profile.subject)
            })
            .or(cached)
    }

    pub async fn fetch(
        config: &Config,
        http: &reqwest::Client,
        tokens: &Tokens,
        provider: Option<&Provider>,
    ) -> Option<Self> {
        let request = async {
            let discovered;
            let provider = match provider {
                Some(provider) => provider,
                None => {
                    discovered = Provider::discover(config, http).await?;
                    &discovered
                }
            };
            provider.userinfo(tokens).await
        };
        match tokio::time::timeout(Duration::from_secs(3), request).await {
            Ok(Ok(info)) => Some(Self {
                subject: info.subject().to_owned(),
                name: display_name(info.name()),
            }),
            _ => {
                tracing::debug!("Identity profile unavailable; retaining the cached name");
                None
            }
        }
    }
}

fn display_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control)
        })
        .map(str::to_owned)
}
