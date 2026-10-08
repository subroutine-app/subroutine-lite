use serde::{Deserialize, Serialize};

use crate::views::SelectedMainView;

const DEFAULT_LEFT_PANEL_PROPORTION: f32 = 0.28;
const DEFAULT_RIGHT_PANEL_PROPORTION: f32 = 0.36;
const MIN_PANEL_PROPORTION: f32 = 0.05;
const MAX_PANEL_PROPORTION: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopLayoutSettings {
    pub selected_main_view: SelectedMainView,
    pub left_panel_open: bool,
    pub right_panel_open: bool,
    left_panel_proportion: f32,
    right_panel_proportion: f32,
}

impl Default for DesktopLayoutSettings {
    fn default() -> Self {
        Self {
            selected_main_view: SelectedMainView::default(),
            left_panel_open: true,
            right_panel_open: false,
            left_panel_proportion: DEFAULT_LEFT_PANEL_PROPORTION,
            right_panel_proportion: DEFAULT_RIGHT_PANEL_PROPORTION,
        }
    }
}

impl DesktopLayoutSettings {
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
            left_panel_open: layout.left_panel_open,
            right_panel_open: layout.right_panel_open,
            left_panel_proportion: layout.left_panel_proportion(),
            right_panel_proportion: layout.right_panel_proportion(),
        }
    }

    pub(super) fn apply_to(self, layout: &mut DesktopLayoutSettings) {
        layout.selected_main_view =
            SelectedMainView::from_id(&self.selected_main_view).unwrap_or_default();
        layout.left_panel_open = self.left_panel_open;
        layout.right_panel_open = self.right_panel_open;
        layout.set_left_panel_proportion(self.left_panel_proportion);
        layout.set_right_panel_proportion(self.right_panel_proportion);
    }
}
