use super::{
    CurrentOverlay, RootView,
    navigation::{ViewDirection, WorkspaceRoute},
};
use crate::{
    components::{OverlayPosition, overlay, timed_toast},
    dates::InclusiveDateRange,
    keys::ConfigurableCommand,
    selection,
    views::{
        CreatorMode, ItemCreator, SettingsDestination, SettingsView,
        command_palette::{AppCommandPalette, AppCommandPaletteEvent},
    },
};
use chrono::NaiveDate;
use gpui::{
    AnyElement, App, AppContext, Context, FocusHandle, Focusable, IntoElement, ParentElement,
    Window, div, px,
};
use gpui_kit::{
    display::badge::Tone,
    overlay::{Dialog, DialogEvent, toast},
};

impl RootView {
    pub(super) fn render_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (current, _) = self.current_overlay.as_ref()?;
        Some(match current {
            CurrentOverlay::CommandPalette(palette) => overlay(
                "command-palette-overlay",
                div().child(palette.clone()),
                OverlayPosition::Top(px(96.).into()),
                window,
                cx,
            )
            .into_any_element(),
            CurrentOverlay::ItemCreator(creator) => creator.clone().into_any_element(),
            CurrentOverlay::BulkDrop(dialog) => dialog.clone().into_any_element(),
            CurrentOverlay::ItemEditor => overlay(
                "item-editor-overlay",
                self.render_item_editor(window, cx),
                OverlayPosition::Center,
                window,
                cx,
            )
            .into_any_element(),
            CurrentOverlay::Settings(settings) => overlay(
                "settings-overlay",
                div().child(settings.clone()),
                OverlayPosition::Center,
                window,
                cx,
            )
            .into_any_element(),
        })
    }

    pub(crate) fn open_drop_confirmation(
        &mut self,
        label: String,
        detail: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        reply: impl FnOnce(DialogEvent, &mut Window, &mut App) + 'static,
    ) {
        if self.current_overlay.is_some() {
            return;
        }
        self.close_navigation_drawer(window, cx);
        self.finish_item_drag(cx);
        let previous_focus = window.focused(cx);
        let dialog = cx.new(|cx| {
            Dialog::new("bulk-drop", window, cx)
                .title(format!("{label}?"))
                .description(detail)
                .confirm_label(label)
                .cancel_label("Cancel")
                .destructive(true)
        });
        let mut reply = Some(reply);
        cx.subscribe_in(
            &dialog,
            window,
            move |view, _, event, window, cx| match event {
                DialogEvent::Confirmed | DialogEvent::Cancelled | DialogEvent::Dismissed => {
                    if let Some(reply) = reply.take() {
                        reply(*event, window, cx);
                    }
                }
                DialogEvent::Closed => view.close_overlay(window, cx),
                DialogEvent::Opened => {}
            },
        )
        .detach();
        self.current_overlay = Some((CurrentOverlay::BulkDrop(dialog.clone()), previous_focus));
        dialog.update(cx, |dialog, cx| dialog.open(window, cx));
        cx.notify();
    }

    pub(crate) fn show_settings(
        &mut self,
        destination: SettingsDestination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_navigation_drawer(window, cx);
        let settings = match self.current_overlay.as_ref() {
            Some((CurrentOverlay::Settings(settings), _)) => settings.clone(),
            Some(_) => return,
            None => {
                let previous_focus = window.focused(cx);
                let settings = cx.new(|cx| SettingsView::new(window, cx));
                self.current_overlay =
                    Some((CurrentOverlay::Settings(settings.clone()), previous_focus));
                settings
            }
        };
        settings.update(cx, |settings, cx| {
            settings.show_destination(destination, cx)
        });
        cx.on_next_frame(window, move |_, window, cx| {
            settings.read(cx).focus_handle(cx).focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.current_overlay.as_ref(),
            Some((CurrentOverlay::ItemEditor, _))
        ) && self.inspector.read(cx).is_dirty(cx)
        {
            self.inspector
                .read(cx)
                .editor_focus_handle(cx)
                .focus(window, cx);
            toast::push(
                window,
                cx,
                timed_toast(
                    "item-editor.unsaved",
                    "Save or revert your changes before closing the editor.",
                )
                .tone(Tone::Warning),
            );
            return;
        }

        let Some((_, previous_focus)) = self.current_overlay.take() else {
            return;
        };
        if let Some(focus_handle) = previous_focus.as_ref() {
            window.focus(focus_handle, cx);
        }
        cx.notify();
    }

    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_command_palette_page(None, window, cx);
    }

    pub(super) fn open_command_palette_page(
        &mut self,
        command: Option<ConfigurableCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_overlay.is_some() {
            return;
        }

        self.close_navigation_drawer(window, cx);
        let previous_focus = window.focused(cx);
        let palette = cx.new(|cx| AppCommandPalette::new(command, window, cx));
        cx.subscribe_in(
            &palette,
            window,
            |view, _, event: &AppCommandPaletteEvent, window, cx| match event {
                AppCommandPaletteEvent::Invoked(command) => {
                    let command = *command;
                    view.close_overlay(window, cx);
                    if command == ConfigurableCommand::CompleteSelected {
                        selection::complete_selected(window, cx);
                    } else if command == ConfigurableCommand::NextTab {
                        view.switch_view(ViewDirection::Next, window, cx);
                    } else if command == ConfigurableCommand::PreviousTab {
                        view.switch_view(ViewDirection::Previous, window, cx);
                    } else if command.targets_main_view() {
                        view.dismiss_library_drawer(cx);
                        view.pending_full_source_route = None;
                        view.route = WorkspaceRoute::Main;
                        view.pending_main_command = Some(command);
                        cx.notify();
                    } else {
                        window.dispatch_action(command.action(), cx);
                    }
                }
                AppCommandPaletteEvent::ThemeSelected(id) => {
                    view.close_overlay(window, cx);
                    window.dispatch_action(Box::new(crate::themes::SwitchTheme(id.clone())), cx);
                }
                AppCommandPaletteEvent::Dismissed => view.close_overlay(window, cx),
            },
        )
        .detach();

        self.current_overlay = Some((CurrentOverlay::CommandPalette(palette), previous_focus));
        cx.notify();
    }

    fn can_open_creator(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.current_overlay.as_ref() {
            None => true,
            Some((CurrentOverlay::ItemEditor, _)) if !self.inspector.read(cx).is_dirty(cx) => true,
            Some((CurrentOverlay::ItemEditor, _)) => {
                self.inspector
                    .read(cx)
                    .editor_focus_handle(cx)
                    .focus(window, cx);
                toast::push(
                    window,
                    cx,
                    timed_toast(
                        "item-editor.unsaved",
                        "Save or revert your changes before creating another item.",
                    )
                    .tone(Tone::Warning),
                );
                false
            }
            Some(_) => false,
        }
    }

    fn previous_focus_for_overlay(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        self.current_overlay
            .as_ref()
            .and_then(|(_, focus)| focus.clone())
            .or_else(|| window.focused(cx))
    }

    pub(super) fn open_creator(
        &mut self,
        mode: CreatorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_open_creator(window, cx) {
            return;
        }
        self.close_navigation_drawer(window, cx);
        let previous_focus = self.previous_focus_for_overlay(window, cx);
        let creator = cx.new(|cx| ItemCreator::new(mode, window, cx));
        self.current_overlay = Some((CurrentOverlay::ItemCreator(creator), previous_focus));
        cx.notify();
    }

    pub(super) fn open_creator_on_date(
        &mut self,
        mode: CreatorMode,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_open_creator(window, cx) {
            return;
        }
        self.close_navigation_drawer(window, cx);
        let previous_focus = self.previous_focus_for_overlay(window, cx);
        let creator = cx.new(|cx| ItemCreator::new_on_date(mode, date, window, cx));
        self.current_overlay = Some((CurrentOverlay::ItemCreator(creator), previous_focus));
        cx.notify();
    }

    pub(super) fn open_queued_creator(
        &mut self,
        date: Option<NaiveDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_open_creator(window, cx) {
            return;
        }
        self.close_navigation_drawer(window, cx);
        let previous_focus = self.previous_focus_for_overlay(window, cx);
        let creator = cx.new(|cx| ItemCreator::new_queued_action(date, window, cx));
        self.current_overlay = Some((CurrentOverlay::ItemCreator(creator), previous_focus));
        cx.notify();
    }

    pub(super) fn open_marker_creator_on_range(
        &mut self,
        range: InclusiveDateRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_open_creator(window, cx) {
            return;
        }
        self.close_navigation_drawer(window, cx);
        let previous_focus = self.previous_focus_for_overlay(window, cx);
        let creator = cx.new(|cx| ItemCreator::new_marker_on_range(range, window, cx));
        self.current_overlay = Some((CurrentOverlay::ItemCreator(creator), previous_focus));
        cx.notify();
    }
}
