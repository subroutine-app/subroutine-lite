use std::path::{Path, PathBuf};

use gpui::{Action, App, Global, SharedString, UpdateGlobal as _, WindowAppearance};
use gpui_kit::tokens::TokenDocument;
use gpui_kit_theme::{ActiveTheme as _, Appearance, Density, ThemeRegistry, activate_theme};
use serde::{Deserialize, Serialize};

use crate::settings::{AppearancePreference, Settings};

const STATE_FILE: &str = "theme-state.json";

fn state_path() -> Option<PathBuf> {
    crate::settings::settings_path().map(|path| path.with_file_name(STATE_FILE))
}
const THEMES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/themes");
const SUBROUTINE_DARK_JSON: &str = include_str!("../../../assets/themes/subroutine-dark.json");
const SUBROUTINE_LIGHT_JSON: &str = include_str!("../../../assets/themes/subroutine-light.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
struct State {
    light_theme: SharedString,
    dark_theme: SharedString,
    #[serde(default, rename = "appearance", skip_serializing)]
    legacy_appearance: Option<Appearance>,
    #[serde(default)]
    compact: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            light_theme: DEFAULT_LIGHT.into(),
            dark_theme: DEFAULT_DARK.into(),
            legacy_appearance: None,
            compact: false,
        }
    }
}

const DEFAULT_LIGHT: &str = "subroutine-light";
const DEFAULT_DARK: &str = "subroutine-dark";

#[derive(Clone, Debug)]
struct ThemePreferences {
    light_theme: SharedString,
    dark_theme: SharedString,
}

impl Global for ThemePreferences {}

#[derive(Clone, Copy, Debug)]
struct LegacyAppearance(Option<AppearancePreference>);

impl Global for LegacyAppearance {}

pub(crate) fn legacy_appearance_preference(cx: &App) -> Option<AppearancePreference> {
    cx.try_global::<LegacyAppearance>()
        .and_then(|legacy| legacy.0)
}

impl ThemePreferences {
    fn from_state(state: &State) -> Self {
        Self {
            light_theme: state.light_theme.clone(),
            dark_theme: state.dark_theme.clone(),
        }
    }

    fn preferred(&self, appearance: Appearance) -> &SharedString {
        match appearance {
            Appearance::Light => &self.light_theme,
            Appearance::Dark => &self.dark_theme,
        }
    }

    fn remember(&mut self, id: SharedString, appearance: Appearance) {
        match appearance {
            Appearance::Light => self.light_theme = id,
            Appearance::Dark => self.dark_theme = id,
        }
    }

    fn validate(&mut self, catalog: &ThemeCatalog) {
        if catalog
            .get_for_appearance(&self.light_theme, Appearance::Light)
            .is_none()
        {
            self.light_theme = catalog.fallback(Appearance::Light);
        }
        if catalog
            .get_for_appearance(&self.dark_theme, Appearance::Dark)
            .is_none()
        {
            self.dark_theme = catalog.fallback(Appearance::Dark);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThemeEntry {
    pub id: SharedString,
    pub name: SharedString,
    pub appearance: Appearance,
}

#[derive(Clone, Debug, Default)]
pub struct ThemeCatalog {
    entries: Vec<ThemeEntry>,
}

impl Global for ThemeCatalog {}

impl ThemeCatalog {
    pub fn entries(&self) -> &[ThemeEntry] {
        &self.entries
    }

    pub fn matching(&self, appearance: Appearance) -> impl Iterator<Item = &ThemeEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.appearance == appearance)
    }

    pub fn get(&self, id: &str) -> Option<&ThemeEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn get_for_appearance(&self, id: &str, appearance: Appearance) -> Option<&ThemeEntry> {
        self.get(id).filter(|entry| entry.appearance == appearance)
    }

    fn fallback(&self, appearance: Appearance) -> SharedString {
        let default = match appearance {
            Appearance::Light => DEFAULT_LIGHT,
            Appearance::Dark => DEFAULT_DARK,
        };
        self.get_for_appearance(default, appearance)
            .or_else(|| self.matching(appearance).next())
            .map(|entry| entry.id.clone())
            .unwrap_or_else(|| default.into())
    }

    fn push(&mut self, id: String, name: String, appearance: Appearance) {
        let entry = ThemeEntry {
            id: id.into(),
            name: name.into(),
            appearance,
        };
        match self.entries.iter().position(|other| other.id == entry.id) {
            Some(index) => self.entries[index] = entry,
            None => self.entries.push(entry),
        }
        self.entries
            .sort_by_key(|entry| (entry.name.to_lowercase(), entry.id.to_lowercase()));
    }
}

pub fn init(cx: &mut App) {
    tracing::info!("Load themes...");

    let state = load_state();

    let mut catalog = ThemeCatalog::default();

    add_gpui_box_themes(&mut catalog, cx);

    register_theme_json(
        SUBROUTINE_DARK_JSON,
        "built-in Subroutine dark theme",
        &mut catalog,
        cx,
    );
    register_theme_json(
        SUBROUTINE_LIGHT_JSON,
        "built-in Subroutine light theme",
        &mut catalog,
        cx,
    );
    load_theme_dir(Path::new(THEMES_DIR), &mut catalog, cx);

    let mut preferences = ThemePreferences::from_state(&state);
    preferences.validate(&catalog);
    let active = preferences
        .preferred(state.legacy_appearance.unwrap_or(Appearance::Dark))
        .clone();

    cx.set_global(catalog);
    cx.set_global(preferences);
    cx.set_global(LegacyAppearance(
        state.legacy_appearance.map(explicit_preference),
    ));

    if !activate_theme(&active, cx) {
        tracing::warn!("theme `{active}` is not registered; keeping the default");
    }
    gpui_kit_theme::set_density(
        if state.compact {
            Density::Compact
        } else {
            Density::Comfortable
        },
        cx,
    );

    cx.refresh_windows();

    cx.observe_global::<ThemeRegistry>(|cx| {
        remember_active_theme(cx);
        save_state(cx);
    })
    .detach();

    if let Err(error) = watch_theme_dir(PathBuf::from(THEMES_DIR), cx) {
        tracing::warn!(
            directory = THEMES_DIR,
            %error,
            "failed to watch the theme directory"
        );
    }

    cx.observe_new::<crate::views::RootView>(|_, window, cx| {
        let Some(window) = window else {
            return;
        };
        synchronize_appearance(Some(window.appearance()), cx);
        window
            .observe_window_appearance(|window, cx| {
                if Settings::global(cx).appearance == AppearancePreference::System {
                    activate_preferred_for(window.appearance(), cx);
                }
            })
            .detach();
    })
    .detach();

    cx.on_action(|switch: &SwitchTheme, cx| {
        let appearance = cx
            .global::<ThemeCatalog>()
            .get(&switch.0)
            .map(|entry| entry.appearance);
        if let Some(appearance) = appearance {
            Settings::update(cx, |settings| {
                settings.appearance = explicit_preference(appearance)
            });
            cx.set_window_appearance(Some(window_appearance(appearance)));
        }
        activate_and_remember(&switch.0, cx);
    });

    cx.on_action(|switch: &SwitchThemeMode, cx| {
        let preference = explicit_preference(switch.0);
        Settings::update(cx, |settings| settings.appearance = preference);
        synchronize_appearance(None, cx);
    });

    cx.on_action(|switch: &SwitchAppearanceMode, cx| {
        Settings::update(cx, |settings| settings.appearance = switch.0);
        synchronize_appearance(None, cx);
    });

    cx.on_action(|_: &ToggleThemeMode, cx| {
        let current = cx.theme().appearance;
        Settings::update(cx, |settings| {
            toggle_appearance_preference(settings, current)
        });
        synchronize_appearance(None, cx);
    });
}

fn platform_theme(mut tokens: TokenDocument) -> TokenDocument {
    if cfg!(target_os = "macos") {
        let wash_increase = match tokens.meta.appearance {
            Appearance::Light => 0.20,
            Appearance::Dark => 0.05,
        };
        tokens.effect.glass_wash = (tokens.effect.glass_wash + wash_increase).min(1.0);
    }
    if cfg!(target_os = "windows") {
        tokens.radius.dialog = 8.0;
    }
    tokens
}

fn builtin_theme(document: &TokenDocument) -> TokenDocument {
    let mut tokens = document.clone();
    if tokens.meta.id == "solarized-light" {
        tokens.color.surface.raised = tokens.color.surface.overlay.clone();
    }
    platform_theme(tokens)
}

fn add_gpui_box_themes(catalog: &mut ThemeCatalog, cx: &mut App) {
    for document in gpui_kit::tokens::all() {
        ThemeRegistry::update_global(cx, |registry, _| {
            registry.register(builtin_theme(document));
        });
        catalog.push(
            document.meta.id.clone(),
            document.meta.name.clone(),
            document.meta.appearance,
        );
    }
}

fn register_theme_json(json: &str, source: &str, catalog: &mut ThemeCatalog, cx: &mut App) {
    let meta = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .as_ref()
        .and_then(read_meta);
    let registered = ThemeRegistry::update_global(cx, |registry, _| {
        TokenDocument::parse(json).map(|tokens| registry.register(platform_theme(tokens)))
    });

    match (registered, meta) {
        (Ok(()), Some((id, name, appearance))) => catalog.push(id, name, appearance),
        (Ok(()), None) => tracing::warn!("theme {source} registered but has no readable meta"),
        (Err(error), _) => tracing::warn!("skipping theme {source}: {error}"),
    }
}

fn load_theme_dir(dir: &Path, catalog: &mut ThemeCatalog, cx: &mut App) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::info!("no theme directory at {}", dir.display());
        return;
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    for path in paths {
        let Ok(json) = std::fs::read_to_string(&path) else {
            continue;
        };

        register_theme_json(&json, &path.display().to_string(), catalog, cx);
    }
}

fn watch_theme_dir(dir: PathBuf, cx: &mut App) -> notify::Result<()> {
    use notify::Watcher as _;

    if !dir.is_dir() {
        tracing::info!(directory = %dir.display(), "theme directory is unavailable; hot reload disabled");
        return Ok(());
    }

    let (changes_tx, changes_rx) = flume::bounded(1);
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if is_theme_file_change(&event) => {
                let _ = changes_tx.try_send(());
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "theme directory watcher error"),
        })?;
    watcher.watch(&dir, notify::RecursiveMode::NonRecursive)?;

    tracing::info!(directory = %dir.display(), "watching theme directory for changes");
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let _watcher = watcher;
        while changes_rx.recv_async().await.is_ok() {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(75))
                    .await;
                if changes_rx.try_recv().is_err() {
                    break;
                }
            }

            let dir = dir.clone();
            cx.update(|cx| reload_theme_dir(&dir, cx));
        }
    })
    .detach();

    Ok(())
}

fn is_theme_file_change(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        notify::EventKind::Create(_) | notify::EventKind::Modify(_) | notify::EventKind::Remove(_)
    ) && event.paths.iter().any(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    })
}

fn reload_theme_dir(dir: &Path, cx: &mut App) {
    let mut reloaded = ThemeCatalog::default();
    load_theme_dir(dir, &mut reloaded, cx);
    if reloaded.entries.is_empty() {
        return;
    }

    let count = reloaded.entries.len();
    ThemeCatalog::update_global(cx, |catalog, _| {
        for entry in reloaded.entries {
            catalog.push(
                entry.id.to_string(),
                entry.name.to_string(),
                entry.appearance,
            );
        }
    });
    cx.refresh_windows();
    tracing::info!(count, "reloaded themes after a filesystem change");
}

fn read_meta(value: &serde_json::Value) -> Option<(String, String, Appearance)> {
    let meta = value.get("meta")?;
    let id = meta.get("id")?.as_str()?.to_owned();
    let name = meta.get("name")?.as_str()?.to_owned();
    let appearance = match meta.get("appearance")?.as_str()? {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        _ => return None,
    };
    Some((id, name, appearance))
}

fn load_state() -> State {
    state_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str::<State>(&json).ok())
        .unwrap_or_default()
}

fn appearance(window: WindowAppearance) -> Appearance {
    match window {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

fn window_appearance(appearance: Appearance) -> WindowAppearance {
    match appearance {
        Appearance::Light => WindowAppearance::Light,
        Appearance::Dark => WindowAppearance::Dark,
    }
}

fn explicit_preference(appearance: Appearance) -> AppearancePreference {
    match appearance {
        Appearance::Light => AppearancePreference::Light,
        Appearance::Dark => AppearancePreference::Dark,
    }
}

fn toggle_appearance_preference(settings: &mut Settings, current: Appearance) {
    settings.appearance = match current {
        Appearance::Light => AppearancePreference::Dark,
        Appearance::Dark => AppearancePreference::Light,
    };
}

fn synchronize_appearance(observed: Option<WindowAppearance>, cx: &mut App) {
    let preference = Settings::global(cx).appearance;
    let requested = match preference {
        AppearancePreference::System => {
            cx.set_window_appearance(None);
            observed.unwrap_or_else(|| cx.window_appearance())
        }
        AppearancePreference::Light => {
            cx.set_window_appearance(Some(WindowAppearance::Light));
            WindowAppearance::Light
        }
        AppearancePreference::Dark => {
            cx.set_window_appearance(Some(WindowAppearance::Dark));
            WindowAppearance::Dark
        }
    };
    activate_preferred_for(requested, cx);
}

fn activate_preferred_for(window: WindowAppearance, cx: &mut App) {
    let appearance = appearance(window);
    let id = {
        let catalog = cx.global::<ThemeCatalog>();
        let preferred = cx
            .global::<ThemePreferences>()
            .preferred(appearance)
            .clone();
        if catalog.get_for_appearance(&preferred, appearance).is_some() {
            preferred
        } else {
            catalog.fallback(appearance)
        }
    };
    activate_and_remember(&id, cx);
}

fn activate_and_remember(id: &str, cx: &mut App) {
    if activate_theme(id, cx) {
        remember_active_theme(cx);
        save_state(cx);
    }
}

fn remember_active_theme(cx: &mut App) {
    let theme = cx.theme();
    let id = theme.id.clone();
    let appearance = theme.appearance;
    ThemePreferences::update_global(cx, |preferences, _| {
        preferences.remember(id, appearance);
    });
}

fn save_state(cx: &App) {
    let preferences = cx.global::<ThemePreferences>();

    let snapshot = State {
        light_theme: preferences.light_theme.clone(),
        dark_theme: preferences.dark_theme.clone(),
        legacy_appearance: None,
        compact: ThemeRegistry::global(cx).density() == Density::Compact,
    };

    if let Some(path) = state_path()
        && let Ok(json) = serde_json::to_vec_pretty(&snapshot)
    {
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, json)?;
            std::fs::rename(temporary, &path)
        })();
        if let Err(error) = result {
            tracing::warn!(%error, "could not persist Lite theme preferences");
        }
    }
}

#[derive(Action, Clone, PartialEq)]
#[action(namespace = themes, no_json)]
pub(crate) struct SwitchTheme(pub(crate) SharedString);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = themes, no_json)]
pub(crate) struct SwitchThemeMode(pub(crate) Appearance);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = themes, no_json)]
pub(crate) struct SwitchAppearanceMode(pub(crate) AppearancePreference);

#[derive(Action, Clone, Default, PartialEq)]
#[action(namespace = themes, no_json)]
pub(crate) struct ToggleThemeMode;
