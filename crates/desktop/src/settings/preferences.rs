use chrono::Duration;

const DEFAULT_NOTIFICATION_LEAD_SECONDS: i64 = 30;
const DEFAULT_SYNC_INTERVAL_MINUTES: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineCreationKind {
    Action,
    Event,
}

impl TimelineCreationKind {
    pub const ALL: [Self; 2] = [Self::Action, Self::Event];

    pub fn id(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Event => "event",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Action => "Action",
            Self::Event => "Event",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimelineCreationSettings {
    pub double_click: TimelineCreationKind,
    pub force_drag: TimelineCreationKind,
}

impl Default for TimelineCreationSettings {
    fn default() -> Self {
        Self {
            double_click: TimelineCreationKind::Action,
            force_drag: TimelineCreationKind::Event,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineToolbarPosition {
    #[default]
    Bottom,
    Right,
}

impl TimelineToolbarPosition {
    pub const ALL: [Self; 2] = [Self::Bottom, Self::Right];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Right => "right",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bottom => "Bottom",
            Self::Right => "Right",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|position| position.id() == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusCarouselOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl FocusCarouselOrientation {
    pub const ALL: [Self; 2] = [Self::Horizontal, Self::Vertical];

    pub fn id(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|orientation| orientation.id() == id)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotificationPreview {
    #[default]
    Generic,
    ActionTitle,
}

impl NotificationPreview {
    pub const ALL: [Self; 2] = [Self::Generic, Self::ActionTitle];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::ActionTitle => "action-title",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Generic => "Hide action title",
            Self::ActionTitle => "Show action title",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preview| preview.id() == id)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub lead_time: Duration,
    pub preview: NotificationPreview,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            lead_time: Duration::seconds(DEFAULT_NOTIFICATION_LEAD_SECONDS),
            preview: NotificationPreview::default(),
        }
    }
}

impl NotificationSettings {
    pub fn lead_seconds(&self) -> i64 {
        self.lead_time.num_seconds().max(1)
    }

    pub fn set_lead_seconds(&mut self, seconds: i64) {
        self.lead_time = Duration::seconds(seconds.max(1));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppearancePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearancePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub const fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preference| preference.id() == id)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CalendarAppearanceSettings {
    pub weekend_shading: bool,
    pub current_month_shading: bool,
}

impl Default for CalendarAppearanceSettings {
    fn default() -> Self {
        Self {
            weekend_shading: false,
            current_month_shading: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AccountSyncSettings {
    interval: std::time::Duration,
}

impl Default for AccountSyncSettings {
    fn default() -> Self {
        Self {
            interval: std::time::Duration::from_secs(DEFAULT_SYNC_INTERVAL_MINUTES * 60),
        }
    }
}

impl AccountSyncSettings {
    pub fn interval(&self) -> std::time::Duration {
        self.interval
    }

    pub fn interval_minutes(&self) -> u64 {
        (self.interval.as_secs() / 60).max(1)
    }

    pub fn set_interval_minutes(&mut self, minutes: u64) {
        self.interval = std::time::Duration::from_secs(minutes.max(1) * 60);
    }
}
