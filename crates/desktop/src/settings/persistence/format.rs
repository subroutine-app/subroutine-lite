use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize, de::IgnoredAny};

use super::super::{
    AppearancePreference, CalendarAppearanceSettings, DEFAULT_FOCUS_ACTION_HORIZON_HOURS,
    DEFAULT_FOCUS_HORIZON_HOURS, DEFAULT_FOCUS_TIMING_THRESHOLD_SECONDS, FocusCarouselOrientation,
    MAX_RECENT_COMMANDS, NotificationPreview, Settings, TimelineCreationKind,
    TimelineToolbarPosition, layout::PersistedDesktopLayout,
};

use crate::{
    keys::{ConfigurableCommand, KeymapConfig},
    selection::SelectionModifier,
    window_state::WindowState,
};

const SETTINGS_VERSION: u32 = 3;

const fn default_enabled() -> bool {
    true
}

const fn default_calendar_current_month_shading() -> bool {
    true
}

const fn default_focus_timing_threshold_seconds() -> i64 {
    DEFAULT_FOCUS_TIMING_THRESHOLD_SECONDS
}

const fn default_focus_horizon_hours() -> u16 {
    DEFAULT_FOCUS_HORIZON_HOURS
}

const fn default_focus_action_horizon_hours() -> u16 {
    DEFAULT_FOCUS_ACTION_HORIZON_HOURS
}

fn default_timeline_toolbar_position() -> String {
    TimelineToolbarPosition::default().id().into()
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PersistedSettings {
    version: u32,
    default_action_minutes: i64,
    granularity_minutes: i64,
    selection_toggle: String,
    selection_range: String,
    notifications_enabled: bool,
    notification_lead_seconds: i64,
    #[serde(default)]
    notification_preview: Option<String>,
    account_sync_interval_minutes: u64,
    queue_creation: String,
    #[serde(default = "default_enabled")]
    queue_batch_mode: bool,
    focus_carousel_orientation: String,
    #[serde(default = "default_focus_timing_threshold_seconds")]
    focus_timing_threshold_seconds: i64,
    #[serde(default = "default_focus_horizon_hours")]
    focus_horizon_hours: u16,
    #[serde(default = "default_focus_action_horizon_hours")]
    focus_action_horizon_hours: u16,
    timeline_double_click: String,
    timeline_force_drag: String,
    #[serde(default = "default_enabled")]
    timeline_batch_mode: bool,
    #[serde(default = "default_timeline_toolbar_position")]
    timeline_toolbar_position: String,
    #[serde(default)]
    pub(super) appearance: Option<String>,
    #[serde(default)]
    reduce_motion: bool,
    #[serde(default)]
    calendar_weekend_shading: bool,
    #[serde(
        default = "default_calendar_current_month_shading",
        alias = "calendar_alternating_month_shading"
    )]
    calendar_current_month_shading: bool,
    #[serde(default)]
    keymap: BTreeMap<String, Vec<String>>,

    #[serde(default)]
    recent_commands: Vec<String>,

    #[serde(default)]
    desktop_layout: PersistedDesktopLayout,
    #[serde(default)]
    window_state: Option<WindowState>,
    #[serde(default, skip_serializing, rename = "calendar_month_borders")]
    _legacy_calendar_month_borders: IgnoredAny,
    #[serde(default, skip_serializing, rename = "focus_carousel_gap_px")]
    _legacy_focus_carousel_gap_px: IgnoredAny,
    #[serde(
        default,
        skip_serializing,
        rename = "focus_carousel_item_scale_percent"
    )]
    _legacy_focus_carousel_item_scale_percent: IgnoredAny,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedSettingsV1 {
    version: u32,
    account_sync_interval_minutes: u64,
}

impl PersistedSettings {
    pub(super) fn from_settings(settings: &Settings) -> Self {
        Self {
            version: SETTINGS_VERSION,
            default_action_minutes: settings.default_action_minutes(),
            granularity_minutes: settings.granularity_minutes(),
            selection_toggle: settings.selection.toggle.id().into(),
            selection_range: settings.selection.range.id().into(),
            notifications_enabled: settings.notifications.enabled,
            notification_lead_seconds: settings.notifications.lead_seconds(),
            notification_preview: Some(settings.notifications.preview.id().into()),
            account_sync_interval_minutes: settings.account_sync.interval_minutes(),
            queue_creation: settings.queue_creation.id().into(),
            queue_batch_mode: settings.queue_batch_mode,
            focus_carousel_orientation: settings.focus_carousel_orientation.id().into(),
            focus_timing_threshold_seconds: settings.focus_timing_threshold_seconds(),
            focus_horizon_hours: settings.focus_horizon_hours(),
            focus_action_horizon_hours: settings.focus_action_horizon_hours(),
            timeline_double_click: settings.timeline_creation.double_click.id().into(),
            timeline_force_drag: settings.timeline_creation.force_drag.id().into(),
            timeline_batch_mode: settings.timeline_batch_mode,
            timeline_toolbar_position: settings.timeline_toolbar_position.id().into(),
            appearance: Some(settings.appearance.id().into()),
            reduce_motion: settings.reduce_motion,
            calendar_weekend_shading: settings.calendar_appearance.weekend_shading,
            calendar_current_month_shading: settings.calendar_appearance.current_month_shading,
            keymap: settings.keymap.overrides().clone(),

            recent_commands: settings.recent_commands.clone(),
            desktop_layout: PersistedDesktopLayout::from_layout(&settings.desktop_layout),
            window_state: settings.window_state,
            _legacy_calendar_month_borders: IgnoredAny,
            _legacy_focus_carousel_gap_px: IgnoredAny,
            _legacy_focus_carousel_item_scale_percent: IgnoredAny,
        }
    }

    pub(super) fn apply_to(mut self, settings: &mut Settings) -> Result<(), String> {
        self.apply_schedule_and_selection(settings)?;
        self.apply_notifications(settings)?;
        settings
            .account_sync
            .set_interval_minutes(self.account_sync_interval_minutes);
        self.apply_creation_and_focus(settings)?;
        self.apply_appearance(settings)?;
        self.keymap.remove("application.show-integrations");
        settings.keymap = KeymapConfig::from_overrides(self.keymap)?;

        settings.recent_commands = restore_recent_commands(self.recent_commands);

        self.desktop_layout.apply_to(&mut settings.desktop_layout);
        settings.window_state = self.window_state;
        Ok(())
    }

    fn apply_schedule_and_selection(&self, settings: &mut Settings) -> Result<(), String> {
        settings.set_default_action_minutes(self.default_action_minutes);
        settings.set_granularity_minutes(self.granularity_minutes);
        settings.selection.toggle = SelectionModifier::from_id(&self.selection_toggle)
            .ok_or_else(|| format!("unknown selection modifier {}", self.selection_toggle))?;
        settings.selection.range = SelectionModifier::from_id(&self.selection_range)
            .ok_or_else(|| format!("unknown selection modifier {}", self.selection_range))?;
        Ok(())
    }

    fn apply_notifications(&self, settings: &mut Settings) -> Result<(), String> {
        settings.notifications.enabled = self.notifications_enabled;
        settings
            .notifications
            .set_lead_seconds(self.notification_lead_seconds);
        settings.notifications.preview = match self.notification_preview.as_deref() {
            Some(preview) => NotificationPreview::from_id(preview)
                .ok_or_else(|| format!("unknown notification preview {preview}"))?,
            None => NotificationPreview::Generic,
        };
        Ok(())
    }

    fn apply_creation_and_focus(&self, settings: &mut Settings) -> Result<(), String> {
        settings.queue_creation = TimelineCreationKind::from_id(&self.queue_creation)
            .ok_or_else(|| format!("unknown queue creation kind {}", self.queue_creation))?;
        settings.queue_batch_mode = self.queue_batch_mode;
        settings.focus_carousel_orientation = FocusCarouselOrientation::from_id(
            &self.focus_carousel_orientation,
        )
        .ok_or_else(|| {
            format!(
                "unknown focus carousel orientation {}",
                self.focus_carousel_orientation
            )
        })?;
        settings.set_focus_timing_threshold_seconds(self.focus_timing_threshold_seconds);
        settings.set_focus_horizon_hours(self.focus_horizon_hours);
        settings.set_focus_action_horizon_hours(self.focus_action_horizon_hours);
        settings.timeline_creation.double_click =
            TimelineCreationKind::from_id(&self.timeline_double_click).ok_or_else(|| {
                format!(
                    "unknown timeline double-click kind {}",
                    self.timeline_double_click
                )
            })?;
        settings.timeline_creation.force_drag =
            TimelineCreationKind::from_id(&self.timeline_force_drag).ok_or_else(|| {
                format!(
                    "unknown timeline force-drag kind {}",
                    self.timeline_force_drag
                )
            })?;
        settings.timeline_batch_mode = self.timeline_batch_mode;
        settings.timeline_toolbar_position =
            TimelineToolbarPosition::from_id(&self.timeline_toolbar_position).ok_or_else(|| {
                format!(
                    "unknown timeline toolbar position {}",
                    self.timeline_toolbar_position
                )
            })?;
        Ok(())
    }

    fn apply_appearance(&self, settings: &mut Settings) -> Result<(), String> {
        settings.appearance = match self.appearance.as_deref() {
            Some(appearance) => AppearancePreference::from_id(appearance)
                .ok_or_else(|| format!("unknown appearance preference {appearance}"))?,
            None => AppearancePreference::System,
        };
        settings.reduce_motion = self.reduce_motion;
        settings.calendar_appearance = CalendarAppearanceSettings {
            weekend_shading: self.calendar_weekend_shading,
            current_month_shading: self.calendar_current_month_shading,
        };
        Ok(())
    }
}

fn restore_recent_commands(ids: Vec<String>) -> Vec<String> {
    let mut recent = Vec::new();
    for id in ids {
        if ConfigurableCommand::from_id(&id).is_some() && !recent.contains(&id) {
            recent.push(id);
            if recent.len() == MAX_RECENT_COMMANDS {
                break;
            }
        }
    }
    recent
}

pub(super) fn decode(
    value: serde_json::Value,
    path: &Path,
) -> Result<(PersistedSettings, bool), String> {
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("{} has no numeric settings version", path.display()))?;
    match version {
        1 => {
            let legacy: PersistedSettingsV1 = serde_json::from_value(value)
                .map_err(|error| format!("decode {}: {error}", path.display()))?;
            let mut settings = Settings::default();
            settings
                .account_sync
                .set_interval_minutes(legacy.account_sync_interval_minutes);
            let mut persisted = PersistedSettings::from_settings(&settings);
            persisted.appearance = None;
            Ok((persisted, true))
        }
        2 => {
            let mut settings: PersistedSettings = serde_json::from_value(value)
                .map_err(|error| format!("decode {}: {error}", path.display()))?;
            settings.version = SETTINGS_VERSION;
            Ok((settings, true))
        }
        version if version == u64::from(SETTINGS_VERSION) => serde_json::from_value(value)
            .map(|settings| (settings, false))
            .map_err(|error| format!("decode {}: {error}", path.display())),
        version => Err(format!("unsupported desktop settings version {version}")),
    }
}
