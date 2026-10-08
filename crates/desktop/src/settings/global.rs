use gpui::{App, Global};

use super::Settings;
use crate::keys::{KeymapConfig, apply_keymap_transition};

pub struct GlobalSettings(Settings);

impl Global for GlobalSettings {}

impl Settings {
    pub fn global(cx: &App) -> Self {
        cx.try_global::<GlobalSettings>()
            .map(|settings| settings.0.clone())
            .unwrap_or_default()
    }

    pub fn initialize_global(cx: &mut App) {
        let settings = Self::load(crate::themes::legacy_appearance_preference(cx));
        let keymap = settings.keymap.clone();
        cx.set_reduce_motion(settings.reduce_motion);
        cx.set_global(GlobalSettings(settings));
        apply_keymap_transition(cx, &KeymapConfig::default(), &keymap);
    }

    pub fn update<R>(cx: &mut App, f: impl FnOnce(&mut Self) -> R) -> R {
        let mut settings = Self::global(cx);
        let previous_keymap = settings.keymap.clone();
        let result = f(&mut settings);
        let next_keymap = settings.keymap.clone();
        if let Err(error) = settings.persist() {
            tracing::warn!(%error, "could not save desktop settings");
        }
        cx.set_reduce_motion(settings.reduce_motion);
        cx.set_global(GlobalSettings(settings));
        apply_keymap_transition(cx, &previous_keymap, &next_keymap);
        cx.refresh_windows();
        result
    }

    pub fn retry_persistence(cx: &mut App) {
        let mut settings = Self::global(cx);
        if let Err(error) = settings.persist() {
            tracing::warn!(%error, "could not save desktop settings on retry");
        }
        cx.set_global(GlobalSettings(settings));
        cx.refresh_windows();
    }
}
