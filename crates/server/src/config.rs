
use std::net::SocketAddr;

use anyhow::{Context as _, Result};
use chrono::Duration;
use chronoutil::RelativeDuration;

use subroutine_core::ScheduleConfig;

use crate::ops::Settings;

pub(crate) struct ListenConfig {
    pub(crate) bind_addr: SocketAddr,
}

impl ListenConfig {
    pub(crate) fn from_env() -> Result<Self> {
        let optional = |name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(anyhow::anyhow!("invalid {name}: {error}")),
        };
        Self::parse(optional("SUBROUTINE_LITE_BIND_ADDR")?.as_deref())
    }

    fn parse(bind_addr: Option<&str>) -> Result<Self> {
        let bind_addr: SocketAddr = bind_addr.unwrap_or("127.0.0.1:3300").parse().context(
            "SUBROUTINE_LITE_BIND_ADDR must be a numeric IP:port socket (IPv6: [::1]:3300)",
        )?;
        Ok(Self { bind_addr })
    }
}

pub(crate) fn settings_from_env() -> Settings {
    let defaults = Settings::default();
    Settings {
        schedule: ScheduleConfig::new(
            minutes("SUBROUTINE_LITE_DEFAULT_ACTION_MINUTES")
                .map(RelativeDuration::minutes)
                .unwrap_or(defaults.schedule.default_action_duration),
            minutes("SUBROUTINE_LITE_SCHEDULE_GRANULARITY_MINUTES")
                .map(Duration::minutes)
                .unwrap_or(defaults.schedule.granularity),
        ),
        default_step_duration: minutes("SUBROUTINE_LITE_DEFAULT_STEP_MINUTES")
            .map(RelativeDuration::minutes)
            .unwrap_or(defaults.default_step_duration),
        expedite_horizon: minutes("SUBROUTINE_LITE_EXPEDITE_HORIZON_MINUTES")
            .map(Duration::minutes)
            .unwrap_or(defaults.expedite_horizon),
    }
}

fn minutes(var: &str) -> Option<i64> {
    let raw = std::env::var(var).ok()?;
    match raw.trim().parse::<i64>() {
        Ok(minutes) if minutes > 0 => Some(minutes),
        _ => {
            tracing::warn!(%var, value = %raw, "ignoring invalid setting");
            None
        }
    }
}
