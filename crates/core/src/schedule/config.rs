use chrono::Duration;
use chronoutil::RelativeDuration;

#[derive(Debug, Clone, Copy)]
pub struct ScheduleConfig {
    pub default_action_duration: RelativeDuration,
    pub granularity: Duration,
}

impl ScheduleConfig {
    pub fn new(default_action_duration: RelativeDuration, granularity: Duration) -> Self {
        Self {
            default_action_duration,
            granularity,
        }
    }
}
