use serde::{Deserialize, Serialize};

use crate::views::SelectedMainView;

const DEFAULT_LEFT_PANEL_PROPORTION: f32 = 0.28;
const DEFAULT_RIGHT_PANEL_PROPORTION: f32 = 0.36;
const MIN_PANEL_PROPORTION: f32 = 0.05;
const MAX_PANEL_PROPORTION: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryView {
    Home,
    Main(SelectedMainView),
}

impl Default for PrimaryView {
    fn default() -> Self {
        Self::Main(SelectedMainView::default())
    }
}

impl PrimaryView {
    pub fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Main(view) => view.id(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "home" => Some(Self::Home),
            id => SelectedMainView::from_id(id)
                .filter(|view| SelectedMainView::TABS.contains(view))
                .map(Self::Main),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopLayoutSettings {
    primary_view: PrimaryView,
    selected_main_view: SelectedMainView,
    pub left_panel_open: bool,
    pub right_panel_open: bool,
    left_panel_proportion: f32,
    right_panel_proportion: f32,
}

impl Default for DesktopLayoutSettings {
    fn default() -> Self {
        Self {
            primary_view: PrimaryView::default(),
            selected_main_view: SelectedMainView::default(),
            left_panel_open: true,
            right_panel_open: false,
            left_panel_proportion: DEFAULT_LEFT_PANEL_PROPORTION,
            right_panel_proportion: DEFAULT_RIGHT_PANEL_PROPORTION,
        }
    }
}

impl DesktopLayoutSettings {
    pub fn primary_view(&self) -> PrimaryView {
        self.primary_view
    }

    pub fn selected_main_view(&self) -> SelectedMainView {
        self.selected_main_view
    }

    pub fn select_primary_view(&mut self, view: PrimaryView) {
        if view == PrimaryView::Main(SelectedMainView::Focus) {
            return;
        }
        self.primary_view = view;
        if let PrimaryView::Main(view) = view {
            self.selected_main_view = view;
        }
    }

    pub fn left_panel_proportion(&self) -> f32 {
        self.left_panel_proportion
    }

    pub fn right_panel_proportion(&self) -> f32 {
        self.right_panel_proportion
    }

    pub fn set_left_panel_proportion(&mut self, proportion: f32) {
        self.left_panel_proportion =
            sanitize_panel_proportion(proportion, DEFAULT_LEFT_PANEL_PROPORTION);
    }

    pub fn set_right_panel_proportion(&mut self, proportion: f32) {
        self.right_panel_proportion =
            sanitize_panel_proportion(proportion, DEFAULT_RIGHT_PANEL_PROPORTION);
    }
}

fn sanitize_panel_proportion(proportion: f32, fallback: f32) -> f32 {
    if proportion.is_finite() {
        proportion.clamp(MIN_PANEL_PROPORTION, MAX_PANEL_PROPORTION)
    } else {
        fallback
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistedDesktopLayout {
    selected_main_view: String,
    #[serde(default)]
    primary_view: Option<String>,
    left_panel_open: bool,
    right_panel_open: bool,
    left_panel_proportion: f32,
    right_panel_proportion: f32,
}

impl Default for PersistedDesktopLayout {
    fn default() -> Self {
        Self::from_layout(&DesktopLayoutSettings::default())
    }
}

impl PersistedDesktopLayout {
    pub(super) fn from_layout(layout: &DesktopLayoutSettings) -> Self {
        Self {
            selected_main_view: layout.selected_main_view.id().into(),
            primary_view: Some(layout.primary_view.id().into()),
            left_panel_open: layout.left_panel_open,
            right_panel_open: layout.right_panel_open,
            left_panel_proportion: layout.left_panel_proportion(),
            right_panel_proportion: layout.right_panel_proportion(),
        }
    }

    pub(super) fn apply_to(self, layout: &mut DesktopLayoutSettings) {
        layout.selected_main_view = SelectedMainView::from_id(&self.selected_main_view)
            .filter(|view| SelectedMainView::TABS.contains(view))
            .unwrap_or_default();
        layout.select_primary_view(
            self.primary_view
                .as_deref()
                .and_then(PrimaryView::from_id)
                .unwrap_or(PrimaryView::Main(layout.selected_main_view)),
        );
        layout.left_panel_open = self.left_panel_open;
        layout.right_panel_open = self.right_panel_open;
        layout.set_left_panel_proportion(self.left_panel_proportion);
        layout.set_right_panel_proportion(self.right_panel_proportion);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fresh_layout_starts_in_timeline() {
        let layout = DesktopLayoutSettings::default();
        assert_eq!(
            layout.primary_view(),
            PrimaryView::Main(SelectedMainView::Timeline)
        );
        assert_eq!(layout.selected_main_view(), SelectedMainView::Timeline);
    }

    #[test]
    fn primary_choices_survive_focus_and_round_trip_with_stable_ids() {
        for (primary, primary_id, main_id) in [
            (PrimaryView::Home, "home", "calendar"),
            (
                PrimaryView::Main(SelectedMainView::Upcoming),
                "queue",
                "queue",
            ),
            (
                PrimaryView::Main(SelectedMainView::Timeline),
                "timeline",
                "timeline",
            ),
            (
                PrimaryView::Main(SelectedMainView::Calendar),
                "calendar",
                "calendar",
            ),
        ] {
            let mut layout = DesktopLayoutSettings::default();
            layout.select_primary_view(PrimaryView::Main(SelectedMainView::Calendar));
            layout.select_primary_view(primary);
            layout.select_primary_view(PrimaryView::Main(SelectedMainView::Focus));
            assert_eq!(layout.primary_view(), primary);
            assert_eq!(layout.selected_main_view().id(), main_id);
            let encoded =
                serde_json::to_value(PersistedDesktopLayout::from_layout(&layout)).unwrap();
            assert_eq!(encoded["primary_view"], primary_id);
            assert_eq!(encoded["selected_main_view"], main_id);

            let persisted: PersistedDesktopLayout = serde_json::from_value(encoded).unwrap();
            let mut restored = DesktopLayoutSettings::default();
            persisted.apply_to(&mut restored);
            assert_eq!(restored, layout);
        }
    }

    #[test]
    fn legacy_layout_restores_main_view_without_resetting_panels() {
        for (id, view) in [
            ("queue", SelectedMainView::Upcoming),
            ("execution-queue", SelectedMainView::Timeline),
            ("timeline", SelectedMainView::Timeline),
            ("calendar", SelectedMainView::Calendar),
            ("focus", SelectedMainView::Timeline),
            ("unknown", SelectedMainView::Timeline),
        ] {
            let persisted: PersistedDesktopLayout = serde_json::from_value(json!({
                "selected_main_view": id,
                "left_panel_open": false,
                "right_panel_open": true,
                "left_panel_proportion": 0.4,
                "right_panel_proportion": 0.5,
            }))
            .unwrap();
            let mut restored = DesktopLayoutSettings::default();
            persisted.apply_to(&mut restored);
            assert_eq!(restored.primary_view(), PrimaryView::Main(view));
            assert_eq!(restored.selected_main_view(), view);
            assert!(!restored.left_panel_open);
            assert!(restored.right_panel_open);
            assert_eq!(restored.left_panel_proportion(), 0.4);
            assert_eq!(restored.right_panel_proportion(), 0.5);
        }
    }

    #[test]
    fn primary_hint_wins_or_falls_back_to_legacy_main_view() {
        for (primary, legacy, expected, expected_main) in [
            (
                Some("home"),
                "calendar",
                PrimaryView::Home,
                SelectedMainView::Calendar,
            ),
            (
                Some("home"),
                "focus",
                PrimaryView::Home,
                SelectedMainView::Timeline,
            ),
            (
                Some("home"),
                "execution-queue",
                PrimaryView::Home,
                SelectedMainView::Timeline,
            ),
            (
                Some("queue"),
                "execution-queue",
                PrimaryView::Main(SelectedMainView::Upcoming),
                SelectedMainView::Upcoming,
            ),
            (
                Some("calendar"),
                "focus",
                PrimaryView::Main(SelectedMainView::Calendar),
                SelectedMainView::Calendar,
            ),
            (
                Some("execution-queue"),
                "queue",
                PrimaryView::Main(SelectedMainView::Upcoming),
                SelectedMainView::Upcoming,
            ),
            (
                Some("focus"),
                "queue",
                PrimaryView::Main(SelectedMainView::Upcoming),
                SelectedMainView::Upcoming,
            ),
            (
                Some("unknown"),
                "calendar",
                PrimaryView::Main(SelectedMainView::Calendar),
                SelectedMainView::Calendar,
            ),
            (
                None,
                "focus",
                PrimaryView::Main(SelectedMainView::Timeline),
                SelectedMainView::Timeline,
            ),
            (
                Some("focus"),
                "execution-queue",
                PrimaryView::Main(SelectedMainView::Timeline),
                SelectedMainView::Timeline,
            ),
            (
                Some("execution-queue"),
                "focus",
                PrimaryView::Main(SelectedMainView::Timeline),
                SelectedMainView::Timeline,
            ),
            (
                Some("unknown"),
                "unknown",
                PrimaryView::Main(SelectedMainView::Timeline),
                SelectedMainView::Timeline,
            ),
        ] {
            let persisted = PersistedDesktopLayout {
                primary_view: primary.map(str::to_owned),
                selected_main_view: legacy.into(),
                ..PersistedDesktopLayout::default()
            };
            let mut restored = DesktopLayoutSettings::default();
            persisted.apply_to(&mut restored);
            assert_eq!(restored.primary_view(), expected);
            assert_eq!(restored.selected_main_view(), expected_main);
        }
    }
}
