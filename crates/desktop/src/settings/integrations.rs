use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_IMPORT_WINDOW_DAYS: u32 = 365;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationSettings {
    #[cfg(target_os = "macos")]
    pub eventkit: EventKitSettings,
    pub google_calendar: GoogleCalendarSettings,
    pub outlook: OutlookSettings,
    pub todoist: TodoistSettings,
    #[cfg(feature = "private-workiq")]
    pub workiq: WorkIqSettings,
}

fn window_days<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    u32::deserialize(deserializer).map(|days| days.min(MAX_IMPORT_WINDOW_DAYS))
}

#[cfg(target_os = "macos")]
fn eventkit_days<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    window_days(deserializer).map(|days| days.max(1))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleCalendarSettings {
    pub enabled: bool,
    pub accepted_events_only: bool,
    #[serde(deserialize_with = "window_days")]
    past_days: u32,
    #[serde(deserialize_with = "window_days")]
    future_days: u32,
    calendars: Option<Vec<String>>,
}

impl Default for GoogleCalendarSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            accepted_events_only: false,
            past_days: 30,
            future_days: 90,
            calendars: None,
        }
    }
}

impl GoogleCalendarSettings {
    pub fn past_days(&self) -> u32 {
        self.past_days
    }
    pub fn future_days(&self) -> u32 {
        self.future_days
    }
    pub fn set_past_days(&mut self, days: u32) {
        self.past_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn set_future_days(&mut self, days: u32) {
        self.future_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn selected_calendars(&self) -> Vec<String> {
        self.calendars.clone().unwrap_or_default()
    }
    pub fn calendar_enabled(&self, id: &str, is_default: bool) -> bool {
        calendar_enabled(&self.calendars, id, is_default)
    }
    pub fn set_calendar_enabled(&mut self, id: &str, enabled: bool, known: &[(String, bool)]) {
        select_calendar(&mut self.calendars, id, enabled, known);
    }
    pub fn reset_calendars(&mut self) {
        self.calendars = None;
    }
    pub fn imports_anything(&self) -> bool {
        self.enabled && self.calendars.as_ref().is_none_or(|ids| !ids.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutlookSettings {
    pub enabled: bool,
    pub calendar: bool,
    pub accepted_events_only: bool,
    pub flagged_mail: bool,
    #[serde(deserialize_with = "window_days")]
    past_days: u32,
    #[serde(deserialize_with = "window_days")]
    future_days: u32,
    calendars: Option<Vec<String>>,
}

impl Default for OutlookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            calendar: true,
            accepted_events_only: false,
            flagged_mail: true,
            past_days: 30,
            future_days: 90,
            calendars: None,
        }
    }
}

impl OutlookSettings {
    pub fn past_days(&self) -> u32 {
        self.past_days
    }
    pub fn future_days(&self) -> u32 {
        self.future_days
    }
    pub fn set_past_days(&mut self, days: u32) {
        self.past_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn set_future_days(&mut self, days: u32) {
        self.future_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn selected_calendars(&self) -> Vec<String> {
        self.calendars.clone().unwrap_or_default()
    }
    pub fn calendar_enabled(&self, id: &str, is_default: bool) -> bool {
        calendar_enabled(&self.calendars, id, is_default)
    }
    pub fn set_calendar_enabled(&mut self, id: &str, enabled: bool, known: &[(String, bool)]) {
        select_calendar(&mut self.calendars, id, enabled, known);
    }
    pub fn reset_calendars(&mut self) {
        self.calendars = None;
    }
    pub fn imports_calendar(&self) -> bool {
        self.calendar && self.calendars.as_ref().is_none_or(|ids| !ids.is_empty())
    }
    pub fn imports_anything(&self) -> bool {
        self.enabled && (self.imports_calendar() || self.flagged_mail)
    }
}

fn calendar_enabled(calendars: &Option<Vec<String>>, id: &str, is_default: bool) -> bool {
    calendars.as_ref().map_or(is_default, |chosen| {
        chosen.iter().any(|chosen| chosen == id)
    })
}

fn select_calendar(
    calendars: &mut Option<Vec<String>>,
    id: &str,
    enabled: bool,
    known: &[(String, bool)],
) {
    let chosen = calendars.get_or_insert_with(|| {
        known
            .iter()
            .filter(|(_, primary)| *primary)
            .map(|(id, _)| id.clone())
            .collect()
    });
    chosen.retain(|current| current != id);
    if enabled {
        chosen.push(id.to_owned());
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventKitSettings {
    pub enabled: bool,
    pub events: bool,
    pub accepted_events_only: bool,
    pub reminders: bool,
    #[serde(deserialize_with = "eventkit_days")]
    future_days: u32,
}

#[cfg(target_os = "macos")]
impl Default for EventKitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            events: true,
            accepted_events_only: false,
            reminders: true,
            future_days: 90,
        }
    }
}

#[cfg(target_os = "macos")]
impl EventKitSettings {
    pub fn future_days(&self) -> u32 {
        self.future_days
    }
    pub fn set_future_days(&mut self, days: u32) {
        self.future_days = days.clamp(1, MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn imports_anything(&self) -> bool {
        self.enabled && (self.events || self.reminders)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoistSettings {
    pub enabled: bool,
}

#[cfg(feature = "private-workiq")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkIqSettings {
    pub enabled: bool,
    pub calendar: bool,
    pub accepted_events_only: bool,
    pub flagged_mail: bool,
    #[serde(deserialize_with = "window_days")]
    past_days: u32,
    #[serde(deserialize_with = "window_days")]
    future_days: u32,
}

#[cfg(feature = "private-workiq")]
impl Default for WorkIqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            calendar: true,
            accepted_events_only: false,
            flagged_mail: true,
            past_days: 30,
            future_days: 90,
        }
    }
}

#[cfg(feature = "private-workiq")]
impl WorkIqSettings {
    pub fn past_days(&self) -> u32 {
        self.past_days
    }
    pub fn future_days(&self) -> u32 {
        self.future_days
    }
    pub fn set_past_days(&mut self, days: u32) {
        self.past_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn set_future_days(&mut self, days: u32) {
        self.future_days = days.min(MAX_IMPORT_WINDOW_DAYS);
    }
    pub fn imports_anything(&self) -> bool {
        self.enabled && (self.calendar || self.flagged_mail)
    }
}
