use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    InteractiveElement, IntoElement, ParentElement, Render, Styled, Subscription, Window, actions,
    div, prelude::FluentBuilder as _, px,
};
use gpui_kit::{
    controls::button::Button,
    foundation::{FocusRing as _, Sizable as _, Slotted as _, StyledExt as _, slot, text},
    overlay::{
        Command, CommandPalette, CommandPaletteEvent, FocusTrap, OverlaySurface, Tooltipped as _,
        surface,
    },
};
use gpui_kit_assets::{Icon, icon};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme as _, Space, ThemeRegistry, TypeScale};

use crate::{
    keys::{ConfigurableCommand, key},
    selection,
    settings::{GlobalSettings, Settings},
    stores::{AppDatabaseStore, DataChanged},
    themes::ThemeCatalog,
};

mod context;
mod items;
mod picker;
use context::{command_state, command_states, command_visible};
use picker::{PickerCommand, PickerSelection};

const KEY_CONTEXT: &str = "ApplicationCommandPalette";
actions!(command_palette, [Back, NextField, PreviousField]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        key("escape", Back, Some(KEY_CONTEXT)),
        key("tab", NextField, Some(KEY_CONTEXT)),
        key("shift-tab", PreviousField, Some(KEY_CONTEXT)),
    ]);
}

pub(crate) enum AppCommandPaletteEvent {
    Invoked(ConfigurableCommand),
    ThemeSelected(gpui::SharedString),
    Dismissed,
}

struct PickerPage {
    command: PickerCommand,
    palette: Entity<CommandPalette>,
    commands: Vec<Command>,
    changed: bool,
    _subscription: Subscription,
}

pub struct AppCommandPalette {
    root: Entity<CommandPalette>,
    root_commands: Vec<Command>,
    root_changed: bool,
    refresh_focus: FocusHandle,
    warning_focus: FocusHandle,
    picker: Option<PickerPage>,
    focus_handle: FocusHandle,
    back_focus: FocusHandle,
    trap: FocusTrap,
    pending_focus: bool,
    error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AppCommandPaletteEvent> for AppCommandPalette {}

fn root_commands(cx: &App) -> Vec<Command> {
    let settings = Settings::global(cx);
    let states = command_states(cx);
    settings
        .keymap
        .palette_commands(settings.recent_commands(), |command| {
            states
                .iter()
                .find(|(candidate, _)| *candidate == command)
                .map(|(_, state)| *state)
                .unwrap_or_default()
        })
        .into_iter()
        .filter(|row| {
            ConfigurableCommand::from_id(row.id())
                .is_some_and(|command| command_visible(command, cx))
        })
        .collect()
}

fn invocation_is_current(displayed: &[Command], current: &[Command], id: &str) -> bool {
    displayed == current
        && displayed
            .iter()
            .any(|command| command.id().as_ref() == id && command.is_available())
}

fn remember_command(command: ConfigurableCommand, cx: &mut App) {
    Settings::update(cx, |settings| settings.remember_command(command));
}

impl AppCommandPalette {
    pub(crate) fn new(
        initial_command: Option<ConfigurableCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let commands = root_commands(cx);
        let root = cx.new(|cx| {
            CommandPalette::new("root-view.command-palette", window, cx).commands(commands.clone())
        });
        let root_subscription = cx.subscribe_in(
            &root,
            window,
            |view, _, event: &CommandPaletteEvent, window, cx| match event {
                CommandPaletteEvent::Invoked(id) if view.picker.is_none() => {
                    if !invocation_is_current(&view.root_commands, &root_commands(cx), id) {
                        view.refresh_state(cx);
                        return;
                    }
                    if let Some(command) = ConfigurableCommand::from_id(id) {
                        if let Some(picker) = PickerCommand::from_command(command) {
                            view.open_picker(picker, window, cx);
                        } else if command_state(command, cx).unavailable.is_none() {
                            remember_command(command, cx);
                            cx.emit(AppCommandPaletteEvent::Invoked(command));
                        } else {
                            view.refresh_root(cx);
                        }
                    }
                }
                CommandPaletteEvent::Dismissed if view.picker.is_none() => view.back(cx),

                _ => {}
            },
        );
        let store = AppDatabaseStore::global(cx);
        let observation = cx.observe(&store, |view, _, cx| view.refresh_state(cx));
        let data_subscription = cx.subscribe(&store, |view, _, _: &DataChanged, cx| {
            view.refresh_state(cx);
        });
        let selected = selection::SelectionManager::global(cx);
        let selection_subscription = cx.observe(&selected, |view, _, cx| view.refresh_state(cx));
        let settings_subscription =
            cx.observe_global::<GlobalSettings>(|view, cx| view.refresh_state(cx));
        let catalog_subscription =
            cx.observe_global::<ThemeCatalog>(|view, cx| view.refresh_state(cx));
        let theme_subscription =
            cx.observe_global::<ThemeRegistry>(|view, cx| view.refresh_state(cx));
        let mut view = Self {
            root,
            root_commands: commands,
            root_changed: false,
            refresh_focus: cx.focus_handle(),
            warning_focus: cx.focus_handle(),
            picker: None,
            focus_handle: cx.focus_handle(),
            back_focus: cx.focus_handle(),
            trap: FocusTrap::new(),
            pending_focus: true,
            error: None,
            _subscriptions: vec![
                root_subscription,
                observation,
                data_subscription,
                selection_subscription,
                settings_subscription,
                catalog_subscription,
                theme_subscription,
            ],
        };
        if let Some(command) = initial_command.and_then(PickerCommand::from_command) {
            view.open_picker(command, window, cx);
        }
        view
    }

    fn active_palette(&self) -> &Entity<CommandPalette> {
        self.picker
            .as_ref()
            .map_or(&self.root, |page| &page.palette)
    }

    fn open_picker(&mut self, command: PickerCommand, window: &mut Window, cx: &mut Context<Self>) {
        let commands = command.commands(cx);
        let has_items = !commands.is_empty();
        let palette = cx.new(|cx| {
            let palette = CommandPalette::new(command.palette_id(), window, cx)
                .commands(commands.clone())
                .slot(slot::EMPTY, move |_, cx| {
                    command.empty_state(has_items, cx).into_any_element()
                });
            palette.query_input().update(cx, |input, cx| {
                input.set_placeholder(command.placeholder(), cx);
                input.set_name(command.placeholder(), cx);
            });
            palette
        });
        let subscription =
            cx.subscribe(&palette, |view, source, event: &CommandPaletteEvent, cx| {
                let Some(page) = &view.picker else { return };
                if page.palette != source {
                    return;
                }
                match event {
                    CommandPaletteEvent::Invoked(id) => {
                        let command = page.command;
                        if !invocation_is_current(&page.commands, &command.commands(cx), id) {
                            view.refresh_picker(cx);
                            return;
                        }
                        match command.invoke(id, cx) {
                            Ok(selection) => {
                                remember_command(command.configurable_command(), cx);
                                cx.emit(match selection {
                                    PickerSelection::Applied => AppCommandPaletteEvent::Dismissed,
                                    PickerSelection::Theme(id) => {
                                        AppCommandPaletteEvent::ThemeSelected(id)
                                    }
                                });
                            }
                            Err(reason) => {
                                view.error = Some(reason);
                                view.refresh_picker(cx);
                            }
                        }
                    }
                    CommandPaletteEvent::Dismissed => view.back(cx),
                    CommandPaletteEvent::QueryChanged(_) => {
                        if view.error.take().is_some() {
                            cx.notify();
                        }
                    }
                }
            });
        self.picker = Some(PickerPage {
            command,
            palette,
            commands,
            changed: false,
            _subscription: subscription,
        });
        self.error = None;
        self.pending_focus = true;
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            self.error = None;
            self.refresh_root(cx);
            self.pending_focus = true;
            cx.notify();
        } else {
            cx.emit(AppCommandPaletteEvent::Dismissed);
        }
    }

    fn refresh_state(&mut self, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.refresh_picker(cx);
        } else {
            let changed = self.root_commands != root_commands(cx);
            if self.root_changed != changed {
                self.root_changed = changed;
                cx.notify();
            }
        }
    }

    fn refresh_root(&mut self, cx: &mut Context<Self>) {
        self.root_changed = false;
        let commands = root_commands(cx);
        if self.root_commands != commands {
            self.root_commands = commands.clone();
            self.root
                .update(cx, |palette, cx| palette.set_commands(commands, cx));
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = &mut self.picker {
            let commands = page.command.commands(cx);
            let command = page.command;
            let has_items = !commands.is_empty();
            page.commands = commands.clone();
            page.changed = false;
            page.palette.update(cx, |palette, cx| {
                palette.set_commands(commands, cx);
                palette.slots_mut().set(slot::EMPTY, move |_, cx| {
                    command.empty_state(has_items, cx).into_any_element()
                });
            });
        } else {
            self.refresh_root(cx);
        }
        self.error = None;
        self.pending_focus = true;
        cx.notify();
    }

    fn refresh_control(&self, hint: &'static str, cx: &Context<Self>) -> gpui::AnyElement {
        let view = cx.entity().downgrade();
        div()
            .id("command-palette.refresh-help")
            .tip("command-palette.refresh-commands", hint)
            .child(
                Button::new("command-palette.refresh-commands")
                    .ghost()
                    .small()
                    .label("Refresh")
                    .accessible_description(hint)
                    .track_focus(&self.refresh_focus)
                    .on_click(move |_, cx| {
                        let _ = view.update(cx, |view, cx| view.refresh(cx));
                    }),
            )
            .into_any_element()
    }

    fn refresh_picker(&mut self, cx: &mut Context<Self>) {
        let Some(page) = &mut self.picker else {
            return;
        };
        let commands = page.command.commands(cx);
        page.changed = page.commands != commands;
        cx.notify();
    }
}

impl Render for AppCommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let palette = self.active_palette().clone();
        let query = palette.read(cx).query_input().read(cx).focus_handle(cx);
        let warning = self
            .picker
            .as_ref()
            .and_then(|page| page.command.warning(cx));
        let changed = self
            .picker
            .as_ref()
            .map_or(self.root_changed, |page| page.changed);
        self.trap.begin_frame();
        self.trap.register(query.clone());
        if self.picker.is_some() {
            self.trap.register(self.back_focus.clone());
        }
        if changed {
            self.trap.register(self.refresh_focus.clone());
        }
        if warning.is_some() {
            self.trap.register(self.warning_focus.clone());
        }
        if self.pending_focus {
            self.pending_focus = false;
            query.focus(window, cx);
        }
        let back = cx.entity().downgrade();

        div()
            .column()
            .gap_token(&theme, Space::Xs)
            .track_focus(&self.focus_handle)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|view, _: &Back, _, cx| view.back(cx)))
            .on_action(cx.listener(|view, _: &NextField, window, cx| {
                view.trap.focus_next(window, cx);
            }))
            .on_action(cx.listener(|view, _: &PreviousField, window, cx| {
                view.trap.focus_prev(window, cx);
            }))
            .on_action(|_: &super::ChooseTheme, _, _| {})
            .on_action(|_: &super::EditSelectedItem, _, _| {})
            .on_action(|_: &super::SyncNow, _, _| {})
            .on_action(|_: &super::OpenConfigurationFolder, _, _| {})
            .on_action(|_: &crate::app::ShowAccountSettings, _, _| {})
            .on_action(|_: &selection::CompleteSelected, _, _| {})
            .on_action(|_: &selection::ToggleQueuedSelected, _, _| {})
            .on_action(|_: &selection::TogglePinnedSelected, _, _| {})
            .on_action(|_: &selection::DeleteSelected, _, _| {})
            .on_action(|_: &selection::CopySelected, _, _| {})
            .on_action(|_: &selection::CutSelected, _, _| {})
            .on_action(|_: &selection::PasteItems, _, _| {})
            .on_action(|_: &selection::DuplicateSelected, _, _| {})
            .on_action(|_: &selection::SelectAllItems, _, _| {})
            .on_action(|_: &super::Undo, _, _| {})
            .on_action(|_: &super::Redo, _, _| {})
            .on_action(|_: &super::GoToHome, _, _| {})
            .on_action(|_: &super::GoToTimeline, _, _| {})
            .on_action(|_: &super::GoToCalendar, _, _| {})
            .on_action(|_: &super::GoToQueue, _, _| {})
            .on_action(|_: &super::GoToFocus, _, _| {})
            .on_action(|_: &super::GoToUnqueued, _, _| {})
            .on_action(|_: &super::GoToRoutines, _, _| {})
            .on_action(|_: &super::GoToSavedItems, _, _| {})
            .on_action(|_: &super::ToggleLeftSidebar, _, _| {})
            .on_action(|_: &super::ToggleRightSidebar, _, _| {})
            .on_action(|_: &crate::app::ToggleSearch, _, _| {})
            .on_action(|_: &crate::app::ShowSettings, _, _| {})
            .when(self.picker.is_none() && changed, |shell| {
                shell.child(div().row().justify_end().child(self.refresh_control(
                    "Refresh to review the updated commands before choosing one.",
                    cx,
                )))
            })
            .when_some(self.picker.as_ref(), |shell, page| {
                shell.child(
                    surface("command-palette.navigation", &theme, OverlaySurface::MODAL)
                        .w(px(480.))
                        .p_token(&theme, Space::Sm)
                        .gap_token(&theme, Space::Xs)
                        .child(
                            div()
                                .row()
                                .items_center()
                                .gap_token(&theme, Space::Sm)
                                .child(
                                    Button::new("command-palette.back")
                                        .ghost()
                                        .small()
                                        .label("Back")
                                        .accessible_name("Back to commands")
                                        .accessible_description("Escape returns to commands")
                                        .track_focus(&self.back_focus)
                                        .on_click(move |_, cx| {
                                            let _ = back.update(cx, |view, cx| view.back(cx));
                                        }),
                                )
                                .child(
                                    text(&theme, TypeScale::Subtitle, page.command.title())
                                        .flex_1()
                                        .semantic_in(
                                            cx,
                                            NodeSpec::new("command-palette.title", Role::Heading)
                                                .text(page.command.title())
                                                .level(2),
                                        ),
                                )
                                .when(page.changed, |header| {
                                    header.child(
                                        self.refresh_control(page.command.refresh_hint(), cx),
                                    )
                                })
                                .when_some(warning, |header, warning| {
                                    header.child(
                                        div()
                                            .id("command-palette.sync")
                                            .track_focus(&self.warning_focus)
                                            .focus_ring(&theme)
                                            .rounded_full()
                                            .child(
                                                icon(Icon::Info)
                                                    .size(px(theme.control.xs.icon_size))
                                                    .text_color(theme.colors.text_muted),
                                            )
                                            .help_tip("command-palette.sync", warning)
                                            .semantic_in(
                                                cx,
                                                NodeSpec::new("command-palette.sync", Role::Status)
                                                    .text(warning)
                                                    .focus(&self.warning_focus),
                                            ),
                                    )
                                }),
                        )
                        .when_some(self.error, |header, error| {
                            header.child(
                                text(&theme, TypeScale::Caption, error)
                                    .text_color(theme.colors.danger)
                                    .semantic_in(
                                        cx,
                                        NodeSpec::new("command-palette.error", Role::Status)
                                            .text(error),
                                    ),
                            )
                        }),
                )
            })
            .child(palette)
    }
}
