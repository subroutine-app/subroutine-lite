use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, ScrollHandle, StatefulInteractiveElement,
    Styled, Window, actions, div, prelude::FluentBuilder, px,
};

use gpui_kit::controls::button::IconButton;
use gpui_kit::controls::keymap_editor::{KeymapCommand, KeymapEditor, KeymapEditorEvent};
use gpui_kit::controls::search::{SearchInput, SearchInputEvent};
use gpui_kit::controls::select::{Select, SelectEvent, SelectOption};
use gpui_kit::controls::settings_row::{SettingsList, SettingsRow, SettingsSection};
use gpui_kit::controls::toggle::Switch;
use gpui_kit::display::avatar::Avatar;
use gpui_kit::foundation::Disableable as _;
use gpui_kit::foundation::slot::{self, Slotted as _};
use gpui_kit::foundation::{FocusRing as _, Ident, Sizable, StyledExt as _};
use gpui_kit::overlay::{GlassExt as _, GlassPreset};
use gpui_kit::strings::{ActiveSearch as _, SearchMatcher};
use gpui_kit_assets::Icon;
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme, ControlSize, Radius, Space, Surface};

#[path = "sync_status.rs"]
mod sync_status;
use self::sync_status::SyncStatusView;

use crate::components::{
    Button, ButtonVariants as _, CloseOverlay, Label, elastic_overscroll::ElasticOverscroll,
};
use crate::keys::key;
use crate::themes::{SwitchAppearanceMode, SwitchTheme, ThemeCatalog};

use crate::{
    app::{SignIn, SignOut},
    auth::{AuthSession, AuthenticationState},
    notifications,
    selection::SelectionModifier,
    settings::{
        AppearancePreference, FocusCarouselOrientation, NotificationPreview, Settings,
        TimelineCreationKind, TimelineToolbarPosition,
    },
    stores::AppDatabaseStore,
};

const ACTION_DURATION_CHOICES: [i64; 7] = [5, 10, 15, 20, 30, 45, 60];
const GRANULARITY_CHOICES: [i64; 5] = [1, 5, 10, 15, 30];

const NOTIFICATION_LEAD_CHOICES: [(i64, &str); 6] = [
    (30, "30 sec"),
    (60, "1 min"),
    (5 * 60, "5 min"),
    (10 * 60, "10 min"),
    (15 * 60, "15 min"),
    (30 * 60, "30 min"),
];

const SETTINGS_LABEL_WIDTH: f32 = 240.0;
const SETTINGS_CONTENT_WIDTH: f32 = 880.0;

const KEY_CONTEXT: &str = "SettingsOverlay";

#[derive(Clone, Copy)]
pub(crate) enum SettingsDestination {
    Current,
    Account,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsCategory {
    Account,
    #[default]
    General,
    Appearance,
    Keymap,
    Timeline,
    Calendar,
}

impl SettingsCategory {
    const ALL: [Self; 6] = [
        Self::Account,
        Self::General,
        Self::Appearance,
        Self::Keymap,
        Self::Timeline,
        Self::Calendar,
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Keymap => "keymap",
            Self::Timeline => "timeline",
            Self::Calendar => "calendar",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Keymap => "Keymap",
            Self::Timeline => "Timeline",
            Self::Calendar => "Calendar",
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::Account => Icon::Key,
            Self::General => Icon::Tuning,
            Self::Appearance => Icon::Image,
            Self::Keymap => Icon::Keyboard,
            Self::Timeline => Icon::ClockCountdown,
            Self::Calendar => Icon::Calendar,
        }
    }
}

fn shortcuts_for_settings_search(
    mut commands: Vec<KeymapCommand>,
    query: &str,
    matcher: &dyn SearchMatcher,
) -> Vec<KeymapCommand> {
    let query = query.trim();
    if query.is_empty()
        || ["Keymap", "Keyboard shortcuts"]
            .into_iter()
            .any(|label| matcher.rank(query, label).is_some())
    {
        return commands;
    }
    commands.retain(|command| {
        [command.id(), command.label_text(), command.search_text()]
            .into_iter()
            .chain(command.context_label())
            .chain(command.keywords())
            .any(|text| matcher.rank(query, text).is_some())
    });
    commands
}

actions!(
    settings_overlay,
    [SettingsFocusNext, SettingsFocusPrevious, CloseSettings]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        key("tab", SettingsFocusNext, Some(KEY_CONTEXT)),
        key("shift-tab", SettingsFocusPrevious, Some(KEY_CONTEXT)),
        key("cmd-w", CloseSettings, Some(KEY_CONTEXT)),
        key("cmd-shift-w", CloseSettings, Some(KEY_CONTEXT)),
        key("escape", gpui::NoAction, Some(KEY_CONTEXT)),
    ]);
}

fn bind_escape<E: InteractiveElement>(element: E, focus: &FocusHandle) -> E {
    let focus = focus.clone();
    element
        .key_context(KEY_CONTEXT)
        .on_key_down(move |event, window, cx| {
            if event.keystroke.key != "escape" || event.keystroke.modifiers.modified() {
                return;
            }
            if !event.is_held {
                if focus.is_focused(window) {
                    window.dispatch_action(Box::new(CloseOverlay), cx);
                } else {
                    focus.focus(window, cx);
                }
            }
            cx.stop_propagation();
        })
}

fn modifier_options() -> Vec<SelectOption> {
    SelectionModifier::ALL
        .iter()
        .map(|m| SelectOption::new(m.label(), m.label()))
        .collect()
}

fn minute_options(choices: &[i64]) -> Vec<SelectOption> {
    choices
        .iter()
        .map(|m| SelectOption::new(m.to_string(), format!("{m} min")))
        .collect()
}

fn timeline_creation_options() -> Vec<SelectOption> {
    TimelineCreationKind::ALL
        .into_iter()
        .map(|kind| SelectOption::new(kind.id(), kind.label()))
        .collect()
}

fn timeline_toolbar_position_options() -> Vec<SelectOption> {
    TimelineToolbarPosition::ALL
        .into_iter()
        .map(|position| SelectOption::new(position.id(), position.label()))
        .collect()
}

fn focus_orientation_options() -> Vec<SelectOption> {
    FocusCarouselOrientation::ALL
        .into_iter()
        .map(|orientation| SelectOption::new(orientation.id(), orientation.label()))
        .collect()
}

fn notification_lead_options() -> Vec<SelectOption> {
    NOTIFICATION_LEAD_CHOICES
        .iter()
        .map(|(seconds, label)| SelectOption::new(seconds.to_string(), *label))
        .collect()
}

fn notification_preview_options() -> Vec<SelectOption> {
    NotificationPreview::ALL
        .into_iter()
        .map(|preview| SelectOption::new(preview.id(), preview.label()))
        .collect()
}

fn appearance_options() -> Vec<SelectOption> {
    AppearancePreference::ALL
        .into_iter()
        .map(|preference| SelectOption::new(preference.id(), preference.label()))
        .collect()
}

pub struct SettingsView {
    focus_handle: FocusHandle,
    end_focus: FocusHandle,
    sync_status: Entity<SyncStatusView>,
    search_input: Entity<SearchInput>,
    active_category: SettingsCategory,
    category_focus: [FocusHandle; SettingsCategory::ALL.len()],
    category_tab_stop: Option<SettingsCategory>,
    category_scroll: ScrollHandle,
    appearance_select: Entity<Select>,
    theme_select: Entity<Select>,
    notification_lead_select: Entity<Select>,
    notification_preview_select: Entity<Select>,
    action_duration_select: Entity<Select>,
    granularity_select: Entity<Select>,
    queue_creation_select: Entity<Select>,
    focus_orientation_select: Entity<Select>,
    timeline_toolbar_position_select: Entity<Select>,
    double_click_creation_select: Entity<Select>,
    force_drag_creation_select: Entity<Select>,
    toggle_modifier_select: Entity<Select>,
    range_modifier_select: Entity<Select>,

    keymap_editor: Entity<KeymapEditor>,
    shortcuts_search: Entity<SearchInput>,
    keymap_error: Option<String>,
    confirming_remove_offline_data: bool,
    overscroll: ElasticOverscroll,
}

impl SettingsView {
    pub(crate) fn show_destination(
        &mut self,
        destination: SettingsDestination,
        cx: &mut Context<Self>,
    ) {
        match destination {
            SettingsDestination::Current => {}
            SettingsDestination::Account => self.show_category(SettingsCategory::Account, cx),
        }
    }

    fn focus_next(&mut self, _: &SettingsFocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        if !self.focus_handle.contains_focused(window, cx) {
            self.focus_handle.focus(window, cx);
            window.focus_next(cx);
        }
    }

    fn focus_previous(
        &mut self,
        _: &SettingsFocusPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
        if !self.focus_handle.contains_focused(window, cx) {
            self.end_focus.focus(window, cx);
            window.focus_prev(cx);
        }
    }

    fn show_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.active_category = category;
        self.search_input
            .update(cx, |search, cx| search.set_value("", cx));
        cx.notify();
    }

    fn sync_selects(&self, settings: &Settings, cx: &mut App) {
        let options = theme_options(cx);
        self.theme_select
            .update(cx, |select, cx| select.set_options(options, cx));
        for (select, id) in [
            (&self.appearance_select, settings.appearance.id().into()),
            (&self.theme_select, cx.theme().id.clone()),
            (
                &self.notification_lead_select,
                settings.notifications.lead_seconds().to_string().into(),
            ),
            (
                &self.notification_preview_select,
                settings.notifications.preview.id().into(),
            ),
            (
                &self.action_duration_select,
                settings.default_action_minutes().to_string().into(),
            ),
            (
                &self.granularity_select,
                settings.granularity_minutes().to_string().into(),
            ),
            (
                &self.queue_creation_select,
                settings.queue_creation.id().into(),
            ),
            (
                &self.focus_orientation_select,
                settings.focus_carousel_orientation.id().into(),
            ),
            (
                &self.timeline_toolbar_position_select,
                settings.timeline_toolbar_position.id().into(),
            ),
            (
                &self.double_click_creation_select,
                settings.timeline_creation.double_click.id().into(),
            ),
            (
                &self.force_drag_creation_select,
                settings.timeline_creation.force_drag.id().into(),
            ),
            (
                &self.toggle_modifier_select,
                settings.selection.toggle.label().into(),
            ),
            (
                &self.range_modifier_select,
                settings.selection.range.label().into(),
            ),
        ] {
            select.update(cx, |select, cx| select.set_selected(Some(id), cx));
        }
    }

    fn render_categories(
        &mut self,
        active_category: SettingsCategory,
        searching: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme().clone();
        let muted = theme.colors.text_muted;
        let categories_id = Ident::new("settings.categories");
        let scroll_id = categories_id.child("scroll");

        self.category_tab_stop = SettingsCategory::ALL
            .into_iter()
            .zip(&self.category_focus)
            .find(|(_, focus)| focus.is_focused(window))
            .map(|(category, _)| category)
            .or(self.category_tab_stop)
            .or(Some(active_category));
        let metrics = theme.control.get(ControlSize::Lg);
        let categories = div()
            .column()
            .flex_none()
            .p(px(theme.space(Space::Xs)))
            .gap(px(theme.space(Space::Xs)))
            .children(
                SettingsCategory::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, category)| {
                        let ident = categories_id.child(category.id());
                        let current = active_category == category && !searching;
                        let color = if current { theme.colors.accent } else { muted };
                        let focus = self.category_focus[index]
                            .clone()
                            .tab_stop(self.category_tab_stop == Some(category));
                        let button = Button::new(ident.child("button").element_id())
                            .ghost()
                            .large()
                            .current(current)
                            .tab_stop(false)
                            .w_full()
                            .min_w_0()
                            .justify_start()
                            .px(px(theme.space(Space::Sm)))
                            .gap(px(theme.space(Space::Sm)))
                            .text_color(color)
                            .child(
                                gpui_kit_assets::icon(category.icon())
                                    .size(px(metrics.icon_size))
                                    .text_color(color),
                            )
                            .child(
                                div().flex_1().min_w_0().overflow_hidden().child(
                                    Label::new(category.label())
                                        .truncate()
                                        .text_size(px(metrics.font_size))
                                        .text_color(color),
                                ),
                            )
                            .tooltip(category.label());
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_none()
                            .rounded(px(theme.radii.control))
                            .focus_ring(&theme)
                            .reveal_on_focus(&self.category_scroll, px(theme.space(Space::Xs)))
                            .on_key_down(cx.listener(
                                move |view, event: &KeyDownEvent, window, cx| {
                                    let next = match event.keystroke.key.as_str() {
                                        "up" => index.checked_sub(1),
                                        "down" => (index + 1 < SettingsCategory::ALL.len())
                                            .then_some(index + 1),
                                        "home" => Some(0),
                                        "end" => Some(SettingsCategory::ALL.len() - 1),
                                        "enter" | "space" => {
                                            if !current {
                                                view.show_category(category, cx);
                                            }
                                            None
                                        }
                                        _ => return,
                                    };
                                    if let Some(next) = next {
                                        view.category_focus[next].focus(window, cx);
                                        cx.notify();
                                    }
                                    cx.stop_propagation();
                                },
                            ))
                            .child(button)
                            .semantic_in(
                                cx,
                                NodeSpec::new(ident.semantic_id(), Role::Link)
                                    .parent(categories_id.semantic_id())
                                    .selected(current)
                                    .disabled(false)
                                    .level(1)
                                    .text(category.label())
                                    .focus(&focus),
                            )
                            .when(!current, |row| {
                                row.on_click(cx.listener(move |view, _, window, cx| {
                                    cx.stop_propagation();
                                    view.category_focus[index].focus(window, cx);
                                    view.show_category(category, cx);
                                }))
                            })
                    }),
            )
            .semantic_in(
                cx,
                NodeSpec::new(scroll_id.child("content").semantic_id(), Role::Group)
                    .parent(scroll_id.semantic_id()),
            );
        div()
            .column()
            .flex_none()
            .h_full()
            .min_h_0()
            .min_w_0()
            .gap(px(theme.space(Space::Md)))
            .p(px(theme.space(Space::Sm)))
            .child(div().flex_none().min_w_0().child(self.search_input.clone()))
            .child(
                div()
                    .id(scroll_id.element_id())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.category_scroll)
                    .child(categories)
                    .semantic_in(cx, NodeSpec::new(scroll_id.semantic_id(), Role::Region)),
            )
            .semantic_in(
                cx,
                NodeSpec::new(categories_id.semantic_id(), Role::List).expanded(true),
            )
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let settings = Settings::global(cx);
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let auth_changes = AuthSession::global(cx).subscribe();
        cx.spawn(async move |view, cx| {
            while auth_changes.recv_async().await.is_ok() {
                if view.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        let search_input = cx.new(|cx| {
            SearchInput::new("settings.search", window, cx)
                .name("Search settings")
                .placeholder("Search settings")
                .large()
        });
        cx.subscribe_in(
            &search_input,
            window,
            |view, _search, event: &SearchInputEvent, window, cx| match event {
                SearchInputEvent::Change(_) => {
                    view.shortcuts_search
                        .update(cx, |search, cx| search.set_value("", cx));
                    cx.notify();
                }
                SearchInputEvent::Cancel => view.focus_handle.focus(window, cx),
                _ => {}
            },
        )
        .detach();

        let keymap_editor = cx.new(|cx| {
            KeymapEditor::new("settings.keymap.editor", window, cx)
                .large()
                .commands(settings.keymap.editor_commands())
        });
        cx.subscribe(
            &keymap_editor,
            |view, _editor, event: &KeymapEditorEvent, cx| {
                let result = match event {
                    KeymapEditorEvent::AddCaptured {
                        command_id,
                        keystroke,
                    } => {
                        Settings::update(cx, |settings| settings.keymap.add(command_id, keystroke))
                    }
                    KeymapEditorEvent::ReplaceCaptured {
                        command_id,
                        binding_id,
                        keystroke,
                    } => Settings::update(cx, |settings| {
                        settings.keymap.replace(command_id, binding_id, keystroke)
                    }),
                    KeymapEditorEvent::Remove {
                        command_id,
                        binding_id,
                    } => Settings::update(cx, |settings| {
                        settings.keymap.remove(command_id, binding_id)
                    }),
                    KeymapEditorEvent::Reset { command_id } => {
                        Settings::update(cx, |settings| settings.keymap.reset(command_id))
                    }
                    KeymapEditorEvent::RecordingCancelled { .. } => return,
                };
                view.keymap_error = result.err();
                if let Some(error) = &view.keymap_error {
                    tracing::warn!(%error, "keymap editor intent was refused");
                }
                cx.notify();
            },
        )
        .detach();

        let shortcuts_search = cx.new(|cx| {
            SearchInput::new("settings.keymap.search", window, cx)
                .name("Search keyboard shortcuts")
                .placeholder("Search shortcuts")
                .large()
        });
        cx.subscribe_in(
            &shortcuts_search,
            window,
            |view, _, event: &SearchInputEvent, window, cx| match event {
                SearchInputEvent::Change(query) => {
                    view.keymap_editor.update(cx, |editor, cx| {
                        editor.set_query(query.trim().to_owned(), cx);
                    });
                    cx.notify();
                }
                SearchInputEvent::Cancel => view.focus_handle.focus(window, cx),
                _ => {}
            },
        )
        .detach();

        let appearance_select = cx.new(|cx| {
            Select::new("settings.appearance.mode", window, cx)
                .large()
                .name("Appearance")
                .options(appearance_options())
                .selected(settings.appearance.id())
        });
        cx.subscribe_in(
            &appearance_select,
            window,
            move |_view, _select, event: &SelectEvent, window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(preference) = AppearancePreference::from_id(id)
                {
                    window.dispatch_action(Box::new(SwitchAppearanceMode(preference)), cx);
                }
            },
        )
        .detach();

        let theme_select = cx.new(|cx| {
            Select::new("settings.theme", window, cx)
                .large()
                .name("Theme")
                .options(theme_options(cx))
                .selected(cx.theme().id.clone())
        });

        cx.subscribe_in(
            &theme_select,
            window,
            move |_view, _select, event: &SelectEvent, window, cx| {
                if let SelectEvent::Selected(id) = event {
                    window.dispatch_action(Box::new(SwitchTheme(id.clone())), cx);
                }
            },
        )
        .detach();

        let notification_lead_select = cx.new(|cx| {
            Select::new("settings.notifications.lead", window, cx)
                .large()
                .name("Notify before")
                .options(notification_lead_options())
                .selected(settings.notifications.lead_seconds().to_string())
        });

        cx.subscribe_in(
            &notification_lead_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Ok(seconds) = id.parse::<i64>()
                {
                    Settings::update(cx, |settings| {
                        settings.notifications.set_lead_seconds(seconds)
                    });
                }
            },
        )
        .detach();

        let notification_preview_select = cx.new(|cx| {
            Select::new("settings.notifications.preview", window, cx)
                .large()
                .name("Notification preview")
                .options(notification_preview_options())
                .selected(settings.notifications.preview.id())
        });
        cx.subscribe_in(
            &notification_preview_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(preview) = NotificationPreview::from_id(id)
                {
                    Settings::update(cx, |settings| settings.notifications.preview = preview);
                }
            },
        )
        .detach();

        let action_duration_select = cx.new(|cx| {
            Select::new("settings.action-duration", window, cx)
                .large()
                .name("Default action duration")
                .options(minute_options(&ACTION_DURATION_CHOICES))
                .selected(settings.default_action_minutes().to_string())
        });

        cx.subscribe_in(
            &action_duration_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Ok(minutes) = id.parse::<i64>()
                {
                    Settings::update(cx, |settings| settings.set_default_action_minutes(minutes));
                }
            },
        )
        .detach();

        let granularity_select = cx.new(|cx| {
            Select::new("settings.granularity", window, cx)
                .large()
                .name("Time granularity")
                .options(minute_options(&GRANULARITY_CHOICES))
                .selected(settings.granularity_minutes().to_string())
        });

        cx.subscribe_in(
            &granularity_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Ok(minutes) = id.parse::<i64>()
                {
                    Settings::update(cx, |settings| settings.set_granularity_minutes(minutes));
                }
            },
        )
        .detach();

        let queue_creation_select = cx.new(|cx| {
            Select::new("settings.queue.creation", window, cx)
                .large()
                .name("Queue plus creates")
                .options(timeline_creation_options())
                .selected(settings.queue_creation.id())
        });
        cx.subscribe_in(
            &queue_creation_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(kind) = TimelineCreationKind::from_id(id)
                {
                    Settings::update(cx, |settings| settings.queue_creation = kind);
                }
            },
        )
        .detach();

        let focus_orientation_select = cx.new(|cx| {
            Select::new("settings.focus.orientation", window, cx)
                .large()
                .name("Focus carousel direction")
                .options(focus_orientation_options())
                .selected(settings.focus_carousel_orientation.id())
        });
        cx.subscribe_in(
            &focus_orientation_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(orientation) = FocusCarouselOrientation::from_id(id)
                {
                    Settings::update(cx, |settings| {
                        settings.focus_carousel_orientation = orientation
                    });
                }
            },
        )
        .detach();

        let timeline_toolbar_position_select = cx.new(|cx| {
            Select::new("settings.timeline.toolbar-position", window, cx)
                .large()
                .name("Toolbar position")
                .options(timeline_toolbar_position_options())
                .selected(settings.timeline_toolbar_position.id())
        });
        cx.subscribe_in(
            &timeline_toolbar_position_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(position) = TimelineToolbarPosition::from_id(id)
                {
                    Settings::update(cx, |settings| settings.timeline_toolbar_position = position);
                }
            },
        )
        .detach();
        let double_click_creation_select = cx.new(|cx| {
            Select::new("settings.timeline.double-click", window, cx)
                .large()
                .name("Double-click creates")
                .options(timeline_creation_options())
                .selected(settings.timeline_creation.double_click.id())
        });
        cx.subscribe_in(
            &double_click_creation_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(kind) = TimelineCreationKind::from_id(id)
                {
                    Settings::update(cx, |settings| {
                        settings.timeline_creation.double_click = kind
                    });
                }
            },
        )
        .detach();

        let force_drag_creation_select = cx.new(|cx| {
            Select::new("settings.timeline.force-drag", window, cx)
                .large()
                .name("Force or Control-drag creates")
                .options(timeline_creation_options())
                .selected(settings.timeline_creation.force_drag.id())
        });
        cx.subscribe_in(
            &force_drag_creation_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(kind) = TimelineCreationKind::from_id(id)
                {
                    Settings::update(cx, |settings| settings.timeline_creation.force_drag = kind);
                }
            },
        )
        .detach();

        let toggle_modifier_select = cx.new(|cx| {
            Select::new("settings.modifier.toggle", window, cx)
                .large()
                .name("Add an item to the selection")
                .options(modifier_options())
                .selected(settings.selection.toggle.label())
        });

        cx.subscribe_in(
            &toggle_modifier_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(modifier) = SelectionModifier::from_label(id)
                {
                    Settings::update(cx, |settings| settings.selection.toggle = modifier);
                }
            },
        )
        .detach();

        let range_modifier_select = cx.new(|cx| {
            Select::new("settings.modifier.range", window, cx)
                .large()
                .name("Select up to an item")
                .options(modifier_options())
                .selected(settings.selection.range.label())
        });

        cx.subscribe_in(
            &range_modifier_select,
            window,
            move |_view, _select, event: &SelectEvent, _window, cx| {
                if let SelectEvent::Selected(id) = event
                    && let Some(modifier) = SelectionModifier::from_label(id)
                {
                    Settings::update(cx, |settings| settings.selection.range = modifier);
                }
            },
        )
        .detach();

        let sync_status = cx.new(SyncStatusView::new);
        Self {
            focus_handle,
            end_focus: cx.focus_handle().tab_index(isize::MAX),
            sync_status,
            search_input,
            active_category: SettingsCategory::default(),
            category_focus: SettingsCategory::ALL.map(|_| cx.focus_handle()),
            category_tab_stop: None,
            category_scroll: ScrollHandle::default(),
            appearance_select,
            theme_select,
            notification_lead_select,
            notification_preview_select,
            action_duration_select,
            granularity_select,
            queue_creation_select,
            focus_orientation_select,
            timeline_toolbar_position_select,
            double_click_creation_select,
            force_drag_creation_select,
            toggle_modifier_select,
            range_modifier_select,

            keymap_editor,
            shortcuts_search,
            keymap_error: None,
            confirming_remove_offline_data: false,
            overscroll: ElasticOverscroll::default(),
        }
    }
}

fn theme_options(cx: &gpui::App) -> Vec<SelectOption> {
    cx.global::<ThemeCatalog>()
        .entries()
        .iter()
        .map(|entry| SelectOption::new(entry.id.clone(), entry.name.clone()))
        .collect()
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let overscroll_y = self.overscroll.advance(cx);
        if self.overscroll.needs_frame(cx) {
            window.request_animation_frame();
        }
        let viewport = window.viewport_size();
        let width = (viewport.width - px(32.)).min(px(960.)).max(px(0.));
        let height = (viewport.height - super::TOP_EDGE_INSET * 2.0)
            .min(px(760.))
            .max(px(0.));
        let compact = width < px(860.);
        let label_width = if compact { 160. } else { SETTINGS_LABEL_WIDTH };
        let text = theme.colors.text;
        let muted = theme.colors.text_muted;
        let hairline = theme.colors.hairline;
        let danger_border = theme.colors.danger.opacity(0.45);
        let danger_background = theme.colors.danger.opacity(0.08);
        let control_radius = theme.radii.control;
        let settings = Settings::global(cx);
        let persistence_issue = settings.persistence_issue().cloned();
        let calendar_appearance = settings.calendar_appearance;

        let reduce_motion = settings.reduce_motion;
        let notifications_enabled = settings.notifications.enabled;

        let auth = AuthSession::global(cx);
        let authentication = auth.state();
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        let pending_count = store.pending_count();
        let has_remote_workspace = store.has_remote_workspace();
        let show_account_sync = auth.is_signed_in() || has_remote_workspace;

        let confirming_remove_offline_data = self.confirming_remove_offline_data;

        let account_name = auth.display_name();
        let account_label = account_name
            .clone()
            .unwrap_or_else(|| "Subroutine account".into());
        let account_detail = match &authentication {
            AuthenticationState::Restoring => Some("Restoring session…".to_owned()),
            AuthenticationState::SigningIn => Some("Continue in your browser.".to_owned()),
            AuthenticationState::Error(message) | AuthenticationState::Offline(message) => {
                Some(message.clone())
            }
            AuthenticationState::SigningOut => Some("Removing the saved session…".to_owned()),
            AuthenticationState::SignedOut => Some(
                if auth.is_interactive_sign_in_configured() {
                    "Sign in to sync across your devices."
                } else {
                    "Offline-only mode is enabled. Unset SUBROUTINE_LITE_OFFLINE_ONLY and restart to enable sign-in. Your data stays on this device."
                }
                .to_owned(),
            ),
            AuthenticationState::SignedIn => {
                Some("Signed in to sync across your devices.".into())
            }
            AuthenticationState::Refreshing => {
                Some("Refreshing authentication; sync is paused.".into())
            }
        };
        let account_action = if auth.can_sign_out() {
            Button::new("settings.account.sign-out")
                .large()
                .label(if authentication == AuthenticationState::SigningIn {
                    "Cancel Sign In"
                } else {
                    "Sign Out"
                })
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(SignOut), cx);
                })
        } else {
            Button::new("settings.account.sign-in")
                .large()
                .primary()
                .label("Sign In")
                .disabled(!auth.can_sign_in())
                .when(auth.can_sign_in(), |button| {
                    button.on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(SignIn), cx);
                    })
                })
        };

        self.sync_selects(&settings, cx);
        let query = self.search_input.read(cx).value(cx);
        let searching = !query.trim().is_empty();
        let active_category = self.active_category;
        let shortcut_commands = shortcuts_for_settings_search(
            settings.keymap.editor_commands(),
            &query,
            cx.search().as_ref(),
        );
        let show_shortcuts = if searching {
            !shortcut_commands.is_empty()
        } else {
            active_category == SettingsCategory::Keymap
        };
        self.keymap_editor.update(cx, |editor, cx| {
            editor.set_commands(shortcut_commands, cx);
        });

        let account = SettingsSection::new("settings.account", "Profile");
        let account = if searching {
            account.row(
                SettingsRow::new("settings.account.session", account_label)
                    .search_terms(["account", "profile", "sign in", "sign out"])
                    .when_some(account_detail, |row, detail| row.description(detail))
                    .control(account_action),
            )
        } else {
            account.child(
                div()
                    .row()
                    .flex_wrap()
                    .w_full()
                    .items_center()
                    .gap_4()
                    .child(
                        div().flex_none().child(
                            Avatar::new(account_name.unwrap_or_default())
                                .id("settings.account.avatar")
                                .size(48.),
                        ),
                    )
                    .child(
                        div()
                            .column()
                            .flex_1()
                            .min_w(px(label_width))
                            .gap_1()
                            .child(Label::new(account_label).font_weight(FontWeight::SEMIBOLD))
                            .children(
                                account_detail
                                    .map(|detail| div().text_sm().text_color(muted).child(detail)),
                            ),
                    )
                    .child(account_action)
                    .semantic_in(
                        cx,
                        NodeSpec::new("settings.account.session", Role::Group).text("Account"),
                    ),
            )
        };
        let account_sync = show_account_sync.then(|| {
            SettingsSection::new("settings.account.synchronization", "Sync")
                .row(SyncStatusView::settings_row(self.sync_status.clone()))
        });

        let persistence_error = persistence_issue.map(|issue| {
            let affected_path = issue
                .path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "Configuration folder unavailable".to_owned());
            let open_folder = Settings::configuration_folder().is_some().then(|| {
                Button::new("settings.persistence.open-folder")
                    .large()
                    .outline()
                    .label(if cfg!(target_os = "macos") {
                        "Reveal Configuration Folder"
                    } else {
                        "Open Configuration Folder"
                    })
                    .on_click(|_, _window, _cx| {
                        if let Err(error) = Settings::open_configuration_folder() {
                            tracing::warn!(%error, "could not open desktop settings folder");
                        }
                    })
            });

            div()
                .column()
                .id("settings.persistence.error")
                .w_full()
                .gap_2()
                .p_3()
                .rounded(px(control_radius))
                .border_1()
                .border_color(danger_border)
                .bg(danger_background)
                .child(Label::new(issue.title()).font_weight(FontWeight::SEMIBOLD))
                .child(
                    Label::new(format!(
                        "{} Changes may be lost when Subroutine Lite restarts.",
                        issue.reason()
                    ))
                    .text_sm()
                    .text_color(muted),
                )
                .child(Label::new(affected_path).text_sm().text_color(muted))
                .child(
                    div()
                        .row()
                        .flex_wrap()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .children(open_folder)
                        .child(
                            Button::new("settings.persistence.retry")
                                .large()
                                .primary()
                                .label("Retry")
                                .on_click(|_, _window, cx| Settings::retry_persistence(cx)),
                        ),
                )
        });

        let appearance = SettingsSection::new("settings.appearance", "Appearance")
            .row(
                SettingsRow::new("settings.appearance.mode", "Appearance")
                    .select(self.appearance_select.clone()),
            )
            .row(
                SettingsRow::new("settings.appearance.theme", "Theme")
                    .select(self.theme_select.clone()),
            )
            .row(
                SettingsRow::new("settings.appearance.reduce-motion", "Reduce motion").switch(
                    Switch::new("settings-reduce-motion")
                        .named("Reduce motion")
                        .on(reduce_motion)
                        .large()
                        .on_change(move |enabled, _window, cx| {
                            Settings::update(cx, |settings| settings.reduce_motion = enabled);
                        }),
                ),
            );

        let calendar = SettingsSection::new("settings.calendar", "Calendar")
            .row(
                SettingsRow::new("settings.appearance.calendar-weekends", "Shade weekends").switch(
                    Switch::new("settings-calendar-weekends")
                        .named("Shade calendar weekends")
                        .on(calendar_appearance.weekend_shading)
                        .large()
                        .on_change(move |enabled, _window, cx| {
                            Settings::update(cx, |settings| {
                                settings.calendar_appearance.weekend_shading = enabled;
                            });
                        }),
                ),
            )
            .row(
                SettingsRow::new(
                    "settings.appearance.calendar-current-month",
                    "Shade current month",
                )
                .info("Shades the days of the month shown in the month selector.")
                .switch(
                    Switch::new("settings-calendar-current-month")
                        .named("Shade current month")
                        .on(calendar_appearance.current_month_shading)
                        .large()
                        .on_change(move |enabled, _window, cx| {
                            Settings::update(cx, |settings| {
                                settings.calendar_appearance.current_month_shading = enabled;
                            });
                        }),
                ),
            );

        let system_notification_settings_row =
            notifications::system_notification_settings_supported().then(|| {
                SettingsRow::new(
                    "settings.notifications.system-settings",
                    "System notification settings",
                )
                .control(
                    Button::new("settings.notifications.open-system-settings")
                        .large()
                        .label("Open Settings")
                        .on_click(|_, _window, cx| {
                            notifications::open_system_notification_settings(cx);
                        }),
                )
            });

        let reminders = SettingsSection::new("settings.notifications", "Reminders")
            .description("Delivered only while Subroutine Lite is open.")
            .row(
                SettingsRow::new(
                    "settings.notifications.upcoming-actions",
                    "Upcoming actions",
                )

                .switch(
                    Switch::new("settings.notifications.enabled")
                        .named("Upcoming action reminders")
                        .on(notifications_enabled)
                        .large()
                        .on_change(move |enabled, _window, cx| {
                            Settings::update(cx, |settings| {
                                settings.notifications.enabled = enabled;
                            });
                        }),
                ),
            )
            .row(
                SettingsRow::new("settings.notifications.lead", "Notify before").select(self.notification_lead_select.clone()),
            )
            .row(
                SettingsRow::new("settings.notifications.preview", "Notification preview")
                    .info("Whether notifications show the action’s title. Lock-screen visibility is set by your system.")
                    .search_terms(["private", "action title"])
                    .select(self.notification_preview_select.clone()),
            )
            .row(
                SettingsRow::new("settings.notifications.test", "Test notification")
                    .control(
                        Button::new("settings.notifications.send-test")
                            .large()
                            .label("Send Test")
                            .on_click(|_, _window, cx| {
                                notifications::show_test_notification(cx);
                            }),
                    ),
            )
            .when_some(system_notification_settings_row, |section, row| section.row(row));

        let planning = SettingsSection::new("settings.planning", "Planning")
            .row(
                SettingsRow::new(
                    "settings.planning.action-duration",
                    "Default action duration",
                )
                .info("Used for actions that don’t set their own duration.")
                .select(self.action_duration_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.granularity", "Time granularity")
                    .info("Scheduled times are rounded to this interval.")
                    .search_terms(["snap", "round", "interval"])
                    .select(self.granularity_select.clone()),
            );

        let force_drag_label = if cfg!(target_os = "macos") {
            "Force-drag creates"
        } else {
            "Control-drag creates"
        };
        let timeline_behavior = SettingsSection::new("settings.advanced.behavior", "Interaction")
            .row(
                SettingsRow::new("settings.timeline.toolbar-position", "Timeline toolbar")
                    .search_terms(["timeline", "bottom", "right", "horizontal", "vertical"])
                    .select(self.timeline_toolbar_position_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.focus", "Focus carousel direction")
                    .select(self.focus_orientation_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.queue-creation", "Queue + button creates")
                    .search_terms(["queue plus"])
                    .select(self.queue_creation_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.queue-batch-mode", "Queue batch mode")
                    .description("Submitting a queue draft opens the next draft.")
                    .search_terms(["queue", "batch", "draft", "submit"])
                    .switch(
                        Switch::new("settings-queue-batch-mode")
                            .named("Queue batch mode")
                            .on(settings.queue_batch_mode)
                            .large()
                            .on_change(move |enabled, _window, cx| {
                                Settings::update(cx, |settings| {
                                    settings.queue_batch_mode = enabled
                                });
                            }),
                    ),
            )
            .row(
                SettingsRow::new("settings.advanced.double-click", "Double-click creates")
                    .info("Double-clicking empty space on the timeline.")
                    .select(self.double_click_creation_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.force-drag", force_drag_label)
                    .info(if cfg!(target_os = "macos") {
                        "Force-click without dragging to create a signal."
                    } else {
                        "Control-click without dragging to create a signal."
                    })
                    .select(self.force_drag_creation_select.clone()),
            )
            .row(
                SettingsRow::new("settings.advanced.timeline-batch-mode", "Timeline batch mode")
                    .description(
                        "Submitting a timeline action or event draft opens the next draft at its end time.",
                    )
                    .search_terms(["timeline", "batch", "draft", "submit", "action", "event"])
                    .switch(
                        Switch::new("settings-timeline-batch-mode")
                            .named("Timeline batch mode")
                            .on(settings.timeline_batch_mode)
                            .large()
                            .on_change(move |enabled, _window, cx| {
                                Settings::update(cx, |settings| {
                                    settings.timeline_batch_mode = enabled
                                });
                            }),
                    ),
            );

        let pointer_selection =
            SettingsSection::new("settings.keymap.pointer-selection", "Pointer selection")
                .row(
                    SettingsRow::new(
                        "settings.advanced.selection.toggle",
                        "Add or remove an item",
                    )
                    .search_terms(["keymap", "mouse", "pointer", "selection"])
                    .select(self.toggle_modifier_select.clone()),
                )
                .row(
                    SettingsRow::new(
                        "settings.advanced.selection.range",
                        "Select through an item",
                    )
                    .search_terms(["keymap", "mouse", "pointer", "selection"])
                    .select(self.range_modifier_select.clone()),
                );

        let keyboard_shortcuts =
            SettingsSection::new("settings.keymap.keyboard-shortcuts", "Keyboard shortcuts")
                .description("Click a shortcut, then press the new keys.")
                .child(
                    div()
                        .column()
                        .gap_2()
                        .child(self.shortcuts_search.clone())
                        .children(self.keymap_error.as_ref().map(|error| {
                            let message = format!("Shortcut not changed: {error}");
                            div()
                                .id("settings.keymap.error")
                                .child(
                                    Label::new(message.clone())
                                        .text_sm()
                                        .text_color(theme.colors.danger),
                                )
                                .semantic_in(
                                    cx,
                                    NodeSpec::new("settings.keymap.error", Role::Status)
                                        .text(message),
                                )
                        }))
                        .child(self.keymap_editor.clone()),
                );

        let remove_offline_confirmation = div()
            .column()
            .id("settings.account.remove-offline-data-confirmation")
            .w_full()
            .gap_3()
            .p_4()
            .rounded(px(control_radius))
            .border_1()
            .border_color(danger_border)
            .bg(danger_background)
            .child(
                Label::new("Remove offline data and sign out?").font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                Label::new(match pending_count {
                    0 => "Synced data stays in your account.".to_owned(),
                    1 => "1 unsynced change will be lost. Synced data stays in your account."
                        .to_owned(),
                    count => format!(
                        "{count} unsynced changes will be lost. Synced data stays in your account."
                    ),
                })
                .text_sm()
                .text_color(muted),
            )
            .child(
                div()
                    .row()
                    .flex_wrap()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("settings.account.remove-offline-data.cancel")
                            .large()
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.confirming_remove_offline_data = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings.account.remove-offline-data.confirm")
                            .large()
                            .danger()
                            .label("Remove and Sign Out")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                let store = AppDatabaseStore::global(cx);
                                let removed =
                                    store.update(cx, |store, cx| store.remove_offline_data(cx));
                                if removed.is_ok() {
                                    this.confirming_remove_offline_data = false;
                                    let session = AuthSession::global(cx);
                                    let generation = session.begin_sign_out();
                                    store.update(cx, |store, cx| store.authentication_changed(cx));
                                    cx.background_spawn(async move {
                                        session.sign_out_blocking(generation)
                                    })
                                    .detach();
                                    cx.notify();
                                }
                            })),
                    ),
            );

        let account_data = SettingsSection::new("settings.advanced.data", "Data & privacy")
            .when(
                has_remote_workspace && !confirming_remove_offline_data,
                |section| {
                    section.row(
                        SettingsRow::new("settings.advanced.data.offline", "Offline account data")
                            .description("Remove account data stored on this device.")
                            .info("Your account and its synced data are not deleted.")
                            .control(
                                Button::new("settings.account.remove-offline-data")
                                    .large()
                                    .danger()
                                    .label("Remove…")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.confirming_remove_offline_data = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                },
            )
            .when(
                has_remote_workspace && confirming_remove_offline_data,
                |section| section.child(remove_offline_confirmation),
            );

        let mut sections = Vec::new();
        match (searching, active_category) {
            (true, _) => {
                sections.push(account);
                sections.extend(account_sync);
                sections.extend([
                    reminders,
                    appearance,
                    pointer_selection,
                    planning,
                    timeline_behavior,
                    calendar,
                ]);
                if has_remote_workspace {
                    sections.push(account_data);
                }
            }
            (false, SettingsCategory::Account) => {
                sections.push(account);
                sections.extend(account_sync);
                if has_remote_workspace {
                    sections.push(account_data);
                }
            }
            (false, SettingsCategory::General) => sections.push(reminders),
            (false, SettingsCategory::Appearance) => sections.push(appearance),
            (false, SettingsCategory::Keymap) => {
                sections.push(pointer_selection);
            }
            (false, SettingsCategory::Timeline) => {
                sections.extend([planning, timeline_behavior]);
            }
            (false, SettingsCategory::Calendar) => sections.push(calendar),
        }

        let sidebar = self
            .render_categories(active_category, searching, window, cx)
            .w(px(if compact { 192. } else { 216. }));
        let page_title = if searching {
            "Search Results"
        } else {
            active_category.label()
        };

        div()
            .column()
            .id("settings-dialog")
            .track_focus(&self.focus_handle)
            .tab_group()
            .map(|element| bind_escape(element, &self.focus_handle))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(|_: &CloseSettings, window, cx| {
                window.dispatch_action(Box::new(CloseOverlay), cx);
            })
            .w(width)
            .h(height)
            .min_h_0()
            .rounded(px(theme.radius(Radius::Dialog)))
            .border_1()
            .border_color(hairline)
            .overflow_hidden()
            .occlude()
            .on_any_mouse_down(|_, _, cx| crate::selection::SelectionManager::claim_press(cx))
            .text_color(text)
            .child(
                div()
                    .row()
                    .items_center()
                    .justify_between()
                    .flex_none()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(hairline)
                    .child(
                        Label::new("Settings")
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        IconButton::new("settings.close", Icon::Close, "Close settings (Esc)")
                            .ghost()
                            .semantic_parent("settings-dialog")
                            .on_click(|window, cx| {
                                window.dispatch_action(Box::new(CloseOverlay), cx);
                            }),
                    ),
            )
            .child(
                div()
                    .row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .flex_none()
                            .h_full()
                            .border_r_1()
                            .border_color(hairline)
                            .child(sidebar),
                    )
                    .child(
                        div()
                            .id("settings-content-scroll")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .on_scroll_wheel(cx.listener(|view, event, window, cx| {
                                if view.overscroll.handle_scroll(event, window, cx) {
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .relative()
                                    .top(overscroll_y)
                                    .column()
                                    .w_full()
                                    .min_w_0()
                                    .flex_shrink_0()
                                    .p_6()
                                    .max_w(px(SETTINGS_CONTENT_WIDTH))
                                    .gap_6()
                                    .child(
                                        Label::new(page_title)
                                            .text_xl()
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .children(persistence_error)
                                    .children(show_shortcuts.then_some(keyboard_shortcuts))
                                    .child(
                                        SettingsList::new("settings.list")
                                            .query(query)
                                            .sections(sections.into_iter().map(|section| {
                                                section.label_width(px(label_width))
                                            }))
                                            .when(show_shortcuts, |list| {
                                                list.slot(slot::EMPTY, |_, _| {
                                                    div().into_any_element()
                                                })
                                            }),
                                    ),
                            ),
                    ),
            )
            .child(div().size_0().track_focus(&self.end_focus))
            .semantic_in(
                cx,
                NodeSpec::new("settings-dialog", Role::Dialog)
                    .text("Settings")
                    .modal(true),
            )
            .bg_glass()
            .glass_surface(Surface::Overlay)
            .glass_radius(Radius::Dialog)
            .glass(|glass| glass.protect_text_contrast(false))
            .when(cfg!(not(target_os = "macos")), |frame| {
                frame.glass_preset(GlassPreset::Frosted)
            })
    }
}
