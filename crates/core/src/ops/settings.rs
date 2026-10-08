use chrono::Duration;
use chronoutil::RelativeDuration;

use crate::ScheduleConfig;

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub schedule: ScheduleConfig,
    pub default_step_duration: RelativeDuration,
    pub expedite_horizon: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schedule: ScheduleConfig::new(RelativeDuration::minutes(5), Duration::minutes(5)),
            default_step_duration: RelativeDuration::minutes(15),
            expedite_horizon: Duration::hours(6),
        }
    }
}
