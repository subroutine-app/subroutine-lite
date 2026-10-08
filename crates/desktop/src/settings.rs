use chrono::Duration;
use chronoutil::RelativeDuration;
use subroutine_core::ScheduleConfig;

use crate::{
    keys::{ConfigurableCommand, KeymapConfig},
    selection::SelectionConfig,
    window_state::WindowState,
};

mod global;

mod layout;
mod persistence;
mod preferences;

pub use global::GlobalSettings;

pub use layout::DesktopLayoutSettings;
pub use persistence::SettingsPersistenceIssue;
pub(crate) use persistence::settings_path;
pub use preferences::{
    AccountSyncSettings, AppearancePreference, CalendarAppearanceSettings,
    FocusCarouselOrientation, NotificationPreview, NotificationSettings, TimelineCreationKind,
    TimelineCreationSettings, TimelineToolbarPosition,
};

const DEFAULT_ACTION_MINUTES: i64 = 5;
const DEFAULT_GRANULARITY_MINUTES: i64 = 5;

const DEFAULT_FOCUS_TIMING_THRESHOLD_SECONDS: i64 = 15 * 60;
const MIN_FOCUS_TIMING_THRESHOLD_SECONDS: i64 = 60;
const MAX_FOCUS_TIMING_THRESHOLD_SECONDS: i64 = 2 * 60 * 60;
const DEFAULT_FOCUS_HORIZON_HOURS: u16 = 24;
const DEFAULT_FOCUS_ACTION_HORIZON_HOURS: u16 = 1;
const MIN_FOCUS_HORIZON_HOURS: u16 = 1;
const MAX_FOCUS_HORIZON_HOURS: u16 = 7 * 24;

const MAX_RECENT_COMMANDS: usize = 6;

#[derive(Debug, Clone)]
pub struct Settings {
    pub schedule: ScheduleConfig,
    pub selection: SelectionConfig,
    pub notifications: NotificationSettings,
    pub account_sync: AccountSyncSettings,
    pub queue_creation: TimelineCreationKind,
    pub queue_batch_mode: bool,
    pub focus_carousel_orientation: FocusCarouselOrientation,
    focus_timing_threshold: Duration,
    focus_horizon: Duration,
    focus_action_horizon: Duration,
    pub timeline_creation: TimelineCreationSettings,
    pub timeline_batch_mode: bool,
    pub timeline_toolbar_position: TimelineToolbarPosition,
    pub appearance: AppearancePreference,
    pub reduce_motion: bool,
    pub calendar_appearance: CalendarAppearanceSettings,
    pub keymap: KeymapConfig,

    recent_commands: Vec<String>,

    pub desktop_layout: DesktopLayoutSettings,
    pub window_state: Option<WindowState>,
    persistence_issue: Option<SettingsPersistenceIssue>,
    default_action_minutes: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schedule: ScheduleConfig::new(
                RelativeDuration::minutes(DEFAULT_ACTION_MINUTES),
                Duration::minutes(DEFAULT_GRANULARITY_MINUTES),
            ),
            selection: SelectionConfig::default(),
            notifications: NotificationSettings::default(),
            account_sync: AccountSyncSettings::default(),
            queue_creation: TimelineCreationKind::Action,
            queue_batch_mode: true,
            focus_carousel_orientation: FocusCarouselOrientation::default(),
            focus_timing_threshold: Duration::seconds(DEFAULT_FOCUS_TIMING_THRESHOLD_SECONDS),
            focus_horizon: Duration::hours(i64::from(DEFAULT_FOCUS_HORIZON_HOURS)),
            focus_action_horizon: Duration::hours(i64::from(DEFAULT_FOCUS_ACTION_HORIZON_HOURS)),
            timeline_creation: TimelineCreationSettings::default(),
            timeline_batch_mode: true,
            timeline_toolbar_position: TimelineToolbarPosition::default(),
            appearance: AppearancePreference::default(),
            reduce_motion: false,
            calendar_appearance: CalendarAppearanceSettings::default(),
            keymap: KeymapConfig::default(),

            recent_commands: Vec::new(),

            desktop_layout: DesktopLayoutSettings::default(),
            window_state: None,
            persistence_issue: None,
            default_action_minutes: DEFAULT_ACTION_MINUTES,
        }
    }
}

impl Settings {
    pub(crate) fn recent_commands(&self) -> &[String] {
        &self.recent_commands
    }

    pub(crate) fn remember_command(&mut self, command: ConfigurableCommand) {
        let id = command.id();
        self.recent_commands.retain(|recent| recent != id);
        self.recent_commands.insert(0, id.to_owned());
        self.recent_commands.truncate(MAX_RECENT_COMMANDS);
    }

    pub fn focus_timing_threshold(&self) -> Duration {
        self.focus_timing_threshold
    }

    pub fn focus_timing_threshold_seconds(&self) -> i64 {
        self.focus_timing_threshold.num_seconds()
    }

    pub fn set_focus_timing_threshold_seconds(&mut self, seconds: i64) {
        self.focus_timing_threshold = Duration::seconds(seconds.clamp(
            MIN_FOCUS_TIMING_THRESHOLD_SECONDS,
            MAX_FOCUS_TIMING_THRESHOLD_SECONDS,
        ));
    }

    pub fn focus_horizon(&self) -> Duration {
        self.focus_horizon
    }

    pub fn focus_horizon_hours(&self) -> u16 {
        self.focus_horizon.num_hours() as u16
    }

    pub fn set_focus_horizon_hours(&mut self, hours: u16) {
        self.focus_horizon = Duration::hours(i64::from(
            hours.clamp(MIN_FOCUS_HORIZON_HOURS, MAX_FOCUS_HORIZON_HOURS),
        ));
    }

    pub fn focus_action_horizon(&self) -> Duration {
        self.focus_action_horizon
    }

    pub fn focus_action_horizon_hours(&self) -> u16 {
        self.focus_action_horizon.num_hours() as u16
    }

    pub fn set_focus_action_horizon_hours(&mut self, hours: u16) {
        self.focus_action_horizon = Duration::hours(i64::from(
            hours.clamp(MIN_FOCUS_HORIZON_HOURS, MAX_FOCUS_HORIZON_HOURS),
        ));
    }

    pub fn default_action_minutes(&self) -> i64 {
        self.default_action_minutes
    }

    pub fn set_default_action_minutes(&mut self, minutes: i64) {
        let minutes = minutes.max(1);
        self.default_action_minutes = minutes;
        self.schedule.default_action_duration = RelativeDuration::minutes(minutes);
    }

    pub fn granularity_minutes(&self) -> i64 {
        self.schedule.granularity.num_minutes().max(1)
    }

    pub fn set_granularity_minutes(&mut self, minutes: i64) {
        self.schedule.granularity = Duration::minutes(minutes.max(1));
    }
}
