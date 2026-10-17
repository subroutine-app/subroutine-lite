use serde::{Deserialize, Serialize};

use super::protocol::{Config, Provider, Tokens};
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Profile {
    pub subject: String,
    pub name: Option<String>,
}

impl Profile {
    pub fn reconcile(cached: Option<Self>, fetched: Option<Self>) -> Option<Self> {
        // A successful read replaces even a former override; an outage keeps the last verified
        // profile, so a name reset is not visible until the next successful same-subject read.
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
        let deadline = Instant::now() + Duration::from_secs(3);
        let request = async {
            let discovered;
            let provider = match provider {
                Some(provider) => provider,
                None => {
                    discovered = timeout_at(deadline, Provider::discover(config, http))
                        .await
                        .map_err(|_| "Identity provider discovery timed out.".to_owned())??;
                    &discovered
                }
            };
            provider
                .userinfo(tokens, deadline.saturating_duration_since(Instant::now()))
                .await
        };
        match request.await {
            Ok(info) => Some(Self {
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
        .filter(|name| {
            !name
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        })
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= 80)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(subject: &str, name: Option<&str>) -> Profile {
        Profile {
            subject: subject.into(),
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn fresh_provider_fallback_replaces_cached_override_even_when_unnamed() {
        let cached = profile("same", Some("Old override"));
        for name in [Some("Current provider name"), None] {
            let reconciled =
                Profile::reconcile(Some(cached.clone()), Some(profile("same", name))).unwrap();
            assert_eq!(reconciled.name.as_deref(), name);
        }
    }

    #[test]
    fn outages_and_other_subjects_cannot_relabel_the_cached_account() {
        for fetched in [None, Some(profile("other", Some("Other identity")))] {
            let reconciled =
                Profile::reconcile(Some(profile("same", Some("Verified name"))), fetched).unwrap();
            assert_eq!(reconciled.subject, "same");
            assert_eq!(reconciled.name.as_deref(), Some("Verified name"));
        }
    }

    #[test]
    fn labels_reject_controls_and_unicode_line_separators_before_trimming() {
        for name in ["\nName", "Name\u{2028}", "\u{2029}Name", "Name\u{7f}"] {
            assert_eq!(display_name(Some(name)), None);
        }
        assert_eq!(
            display_name(Some("  Freeform / ✨  ")),
            Some("Freeform / ✨".into())
        );
    }
}
