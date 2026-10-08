use anyhow::Result;
use gpui::{
    App, AppContext, AsyncApp, Bounds, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions, WindowToolbarStyle, actions, point, px, size,
};
mod native_command_views;
use gpui_kit::controls::input;
use gpui_kit_theme::ActiveTheme;
use gpui_kit_theme::Appearance;
use gpui_kit_theme::ThemeRegistry;
use native_command_views::{AboutView, ShortcutsView};

const WEBSITE_URL: &str = "https://github.com/subroutine-app/subroutine-lite";

use crate::views::SettingsDestination;
use crate::{
    assets::AppAssets,
    auth::{AuthSession, Config},
    components,
    item_manager::ItemManager,
    keys::key,
    notifications,
    selection::{self, SelectionManager},
    settings::Settings,
    stores::AppDatabaseStore,
    themes::{self, SwitchTheme, SwitchThemeMode, ThemeCatalog},
    views::{self, Redo, RootView, Undo},
};

actions!(
    app,
    [
        About,
        OpenWebsite,
        Quit,
        SignIn,
        SignOut,
        ShowAccountSettings,
        ShowSettings,
        ShowShortcuts,
        ToggleSearch,
    ]
);

pub fn run() -> Result<()> {
    #[cfg(target_os = "macos")]
    let platform = std::rc::Rc::new(gpui_macos::MacPlatform::new(false));
    #[cfg(target_os = "windows")]
    let platform = std::rc::Rc::new(gpui_windows::WindowsPlatform::new(false)?);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let platform = gpui_linux::current_platform(false);

    gpui::Application::with_platform(platform)
        .with_assets(AppAssets)
        .run(init);
    Ok(())
}

pub fn init(cx: &mut App) {
    cx.activate(true);

    gpui_kit::install(cx);
    themes::init(cx);
    components::init(cx);
    selection::init(cx);
    views::init(cx);

    let config = Config::from_env();
    if let Err(error) = &config {
        tracing::error!(%error, "invalid network configuration; authentication and synchronization disabled");
    }
    let server_url = config
        .as_ref()
        .ok()
        .and_then(|config| config.as_ref())
        .map(|config| config.api.clone());

    cx.bind_keys([
        key("cmd-,", ShowSettings, None),
        key("cmd-shift-f", ToggleSearch, None),
        key("cmd-q", Quit, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([gpui::KeyBinding::new(
        "ctrl-shift-w",
        Quit,
        Some("!SettingsOverlay"),
    )]);

    let auth_session = AuthSession::initialize_global(config, cx);
    Settings::initialize_global(cx);
    let app_store =
        AppDatabaseStore::initialize_global(server_url.clone(), auth_session.clone(), cx);
    let auth = auth_session.clone();
    let restore_store = app_store.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_spawn(async move { auth.restore_blocking() })
            .await;
        restore_store.update(cx, |store, cx| store.finish_session_restore(cx));
    })
    .detach();

    let auth_menu_changes = auth_session.subscribe();
    cx.spawn(async move |cx: &mut AsyncApp| {
        while auth_menu_changes.recv_async().await.is_ok() {
            cx.update(|cx| {
                app_store.update(cx, |store, cx| store.authentication_changed(cx));
                update_app_menu(cx);
            });
        }
    })
    .detach();
    ItemManager::initialize_global(cx);
    SelectionManager::initialize_global(cx);
    notifications::init(cx);

    let titlebar = if cfg!(target_os = "macos") {
        let options = TitlebarOptions {
            traffic_light_position: Some(point(px(20.), px(20.))),
            appears_transparent: true,
            toolbar_style: Some(WindowToolbarStyle::Unified),
            ..Default::default()
        };
        Some(options)
    } else if cfg!(target_os = "windows") {
        Some(TitlebarOptions {
            title: Some("Subroutine Lite".into()),
            appears_transparent: true,
            ..Default::default()
        })
    } else {
        None
    };

    let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
    let mut window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar,
        app_owns_titlebar_drag: true,

        app_id: Some(crate::branding::APP_ID.into()),
        focus: true,
        show: true,
        window_min_size: Some(size(px(720.0), px(600.0))),
        ..Default::default()
    };

    let restored_window_state = crate::window_state::restore(&mut window_options, cx);

    update_app_menu(cx);

    cx.observe_global::<ThemeRegistry>({
        move |cx| {
            update_app_menu(cx);
        }
    })
    .detach();

    let _window = cx
        .open_window(window_options, move |window, cx| {
            crate::window_state::install(window, restored_window_state, cx);
            cx.new(|cx| RootView::new(window, cx))
        })
        .unwrap();

    if let Err(error) = _window.update(cx, |_, window, _| window.activate_window()) {
        tracing::error!(%error, "could not activate the main window");
    }

    cx.on_action(move |_action: &Quit, cx: &mut App| {
        cx.quit();
    });

    cx.on_action(move |_action: &OpenWebsite, cx: &mut App| {
        cx.open_url(WEBSITE_URL);
    });

    cx.on_action(move |_action: &About, cx: &mut App| {
        show_about_window(cx);
    });

    cx.on_action(move |_action: &ShowShortcuts, cx: &mut App| {
        show_shortcuts_window(cx);
    });

    cx.on_action(move |_action: &ShowSettings, cx: &mut App| {
        show_settings(SettingsDestination::Current, cx);
    });

    cx.on_action(move |_action: &ShowAccountSettings, cx: &mut App| {
        show_settings(SettingsDestination::Account, cx);
    });

    cx.on_action(move |_action: &ToggleSearch, cx: &mut App| {
        cx.defer(|cx| {
            let root = cx
                .windows()
                .into_iter()
                .find_map(|window| window.downcast::<RootView>());
            if let Some(root) = root
                && let Err(error) = root.update(cx, |root, window, cx| {
                    window.activate_window();
                    root.show_search(window, cx);
                })
            {
                tracing::warn!(%error, "Failed to open search");
            }
        });
    });

    cx.on_action(move |_action: &SignIn, cx: &mut App| {
        let session = AuthSession::global(cx);
        if let Some(generation) = session.begin_sign_in() {
            AppDatabaseStore::global(cx).update(cx, |store, cx| store.authentication_changed(cx));
            show_settings(SettingsDestination::Account, cx);
            cx.background_spawn(async move { session.sign_in_blocking(generation) })
                .detach();
        }
    });

    cx.on_action(move |_action: &SignOut, cx: &mut App| {
        let session = AuthSession::global(cx);
        let store = AppDatabaseStore::global(cx);
        let generation = session.begin_sign_out();
        store.update(cx, |store, cx| store.authentication_changed(cx));
        cx.background_spawn(async move { session.sign_out_blocking(generation) })
            .detach();
    });
}

fn show_about_window(cx: &mut App) {
    if activate_existing_window::<AboutView>(cx) {
        return;
    }

    let options = command_window_options(
        cx,
        "About Subroutine Lite",
        size(px(480.), px(320.)),
        size(px(400.), px(280.)),
    );
    if let Err(error) = cx.open_window(options, |window, cx| {
        cx.new(|cx| AboutView::new(window, cx))
    }) {
        tracing::error!(%error, "could not open About window");
    }
}

fn show_shortcuts_window(cx: &mut App) {
    if activate_existing_window::<ShortcutsView>(cx) {
        return;
    }

    let options = command_window_options(
        cx,
        "Keyboard Shortcuts",
        size(px(820.), px(720.)),
        size(px(620.), px(480.)),
    );
    if let Err(error) = cx.open_window(options, |window, cx| {
        cx.new(|cx| ShortcutsView::new(window, cx))
    }) {
        tracing::error!(%error, "could not open Keyboard Shortcuts window");
    }
}

fn activate_existing_window<V: 'static>(cx: &mut App) -> bool {
    cx.windows()
        .into_iter()
        .find_map(|window| window.downcast::<V>())
        .is_some_and(|window| {
            window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        })
}

fn command_window_options(
    cx: &App,
    title: &str,
    window_size: gpui::Size<gpui::Pixels>,
    minimum_size: gpui::Size<gpui::Pixels>,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        focus: true,
        show: true,
        window_min_size: Some(minimum_size),
        app_id: Some(crate::branding::APP_ID.into()),
        ..Default::default()
    }
}

fn show_settings(destination: SettingsDestination, cx: &mut App) {
    cx.defer(move |cx| {
        let root = cx
            .windows()
            .into_iter()
            .find_map(|window| window.downcast::<RootView>());
        if let Some(root) = root
            && let Err(error) = root.update(cx, |root, window, cx| {
                window.activate_window();
                root.show_settings(destination, window, cx);
            })
        {
            tracing::warn!(%error, "Failed to open settings");
        }
    });
}

fn theme_menu(cx: &App) -> MenuItem {
    let catalog = cx.global::<ThemeCatalog>();
    let active = cx.theme().id.clone();

    MenuItem::Submenu(Menu {
        disabled: false,
        name: "Theme".into(),
        items: catalog
            .entries()
            .iter()
            .map(|entry| {
                MenuItem::action(entry.name.clone(), SwitchTheme(entry.id.clone()))
                    .checked(active == entry.id)
            })
            .collect(),
    })
}

fn application_menu_items(
    mode: Appearance,
    signed_in: bool,
    can_sign_in: bool,
    theme: MenuItem,
) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::action("About", About),
        MenuItem::Separator,
        MenuItem::action("Settings...", ShowSettings),
    ];
    if signed_in {
        items.extend([MenuItem::Separator, MenuItem::action("Sign Out", SignOut)]);
    } else if can_sign_in {
        items.extend([MenuItem::Separator, MenuItem::action("Sign In...", SignIn)]);
    }
    items.extend([
        MenuItem::Separator,
        MenuItem::Submenu(Menu {
            disabled: false,
            name: "Appearance".into(),
            items: vec![
                MenuItem::action("Light", SwitchThemeMode(Appearance::Light))
                    .checked(mode == Appearance::Light),
                MenuItem::action("Dark", SwitchThemeMode(Appearance::Dark))
                    .checked(mode == Appearance::Dark),
            ],
        }),
        theme,
        MenuItem::Separator,
        MenuItem::action("Quit", Quit),
    ]);
    items
}

fn window_menu() -> Menu {
    Menu {
        disabled: false,
        name: "Window".into(),
        items: vec![MenuItem::action("Search All Items", ToggleSearch)],
    }
}

fn help_menu() -> Menu {
    Menu {
        disabled: false,
        name: "Help".into(),
        items: vec![
            MenuItem::action("Keyboard Shortcuts...", ShowShortcuts),
            MenuItem::Separator,
            MenuItem::action("Open Subroutine Lite Repository", OpenWebsite),
        ],
    }
}

pub(crate) fn update_app_menu(cx: &App) {
    let mode = cx.theme().appearance;
    let auth = AuthSession::global(cx);
    let signed_in = auth.can_sign_out();
    let can_sign_in = auth.can_sign_in();
    let store = AppDatabaseStore::global(cx);
    let store = store.read(cx);
    let can_undo = store.can_undo();
    let can_redo = store.can_redo();
    cx.set_menus(vec![
        Menu {
            disabled: false,
            name: "Subroutine Lite".into(),
            items: application_menu_items(mode, signed_in, can_sign_in, theme_menu(cx)),
        },
        Menu {
            disabled: false,
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", Undo).disabled(!can_undo),
                MenuItem::action("Redo", Redo).disabled(!can_redo),
                MenuItem::separator(),
                MenuItem::action("Cut", input::Cut),
                MenuItem::action("Copy", input::Copy),
                MenuItem::action("Paste", input::Paste),
                MenuItem::separator(),
                MenuItem::action("Delete", input::Delete),
                MenuItem::action("Delete Previous Word", input::DeleteWordLeft),
                MenuItem::action("Delete Next Word", input::DeleteWordRight),
                MenuItem::separator(),
                MenuItem::action("Select All", input::SelectAll),
            ],
        },
        window_menu(),
        help_menu(),
    ]);
}
