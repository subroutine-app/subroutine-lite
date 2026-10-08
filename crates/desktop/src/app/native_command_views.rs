use gpui::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_kit::{
    controls::{
        button::Button,
        keymap_editor::{KeymapEditor, KeymapEditorEvent},
        search::{SearchInput, SearchInputEvent},
    },
    foundation::{Sizable as _, StyledExt as _, text},
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme as _, Space, TextTone, TypeScale};

use crate::{
    app::WEBSITE_URL, components::elastic_overscroll::ElasticOverscroll, settings::Settings,
};

pub(super) struct AboutView {
    focus_handle: FocusHandle,
}

impl AboutView {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self { focus_handle }
    }
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.theme().clone();

        div()
            .id("about-window")
            .track_focus(&self.focus_handle)
            .column()
            .size_full()
            .items_center()
            .justify_center()
            .bg(theme.colors.canvas)
            .text_color(theme.colors.text)
            .p(px(theme.space(Space::Xl)))
            .child(
                div()
                    .column()
                    .w_full()
                    .max_w(px(440.))
                    .gap_token(&theme, Space::Md)
                    .child(crate::branding::logotype("about.brand", 280., cx))
                    .child(
                        text(
                            &theme,
                            TypeScale::Subtitle,
                            format!("Subroutine Lite · Version {}", env!("CARGO_PKG_VERSION")),
                        )
                        .text_tone(&theme, TextTone::Muted),
                    )
                    .child(text(
                        &theme,
                        TypeScale::Body,
                        "Plan actions, events, routines, markers, and signals in one timeline.",
                    ))
                    .child(
                        div()
                            .row()
                            .justify_end()
                            .gap_token(&theme, Space::Sm)
                            .child(
                                Button::new("about.website")
                                    .secondary()
                                    .label("Subroutine Lite Repository")
                                    .on_click(|_, cx| cx.open_url(WEBSITE_URL)),
                            )
                            .child(
                                Button::new("about.close")
                                    .label("Close")
                                    .on_click(|window, _| window.remove_window()),
                            ),
                    ),
            )
    }
}

pub(super) struct ShortcutsView {
    focus_handle: FocusHandle,
    keymap_editor: Entity<KeymapEditor>,
    shortcuts_search: Entity<SearchInput>,
    keymap_error: Option<String>,
    overscroll: ElasticOverscroll,
}

impl ShortcutsView {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let keymap_editor = cx.new(|cx| {
            KeymapEditor::new("shortcuts.editor", window, cx)
                .large()
                .commands(Settings::global(cx).keymap.editor_commands())
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
                    tracing::warn!(%error, "keyboard shortcut intent was refused");
                }
                cx.notify();
            },
        )
        .detach();

        let shortcuts_search = cx.new(|cx| {
            SearchInput::new("shortcuts.search", window, cx)
                .name("Search keyboard shortcuts")
                .placeholder("Search shortcuts")
                .large()
        });
        cx.subscribe(
            &shortcuts_search,
            |view, _, event: &SearchInputEvent, cx| {
                if let SearchInputEvent::Change(query) = event {
                    view.keymap_editor.update(cx, |editor, cx| {
                        editor.set_query(query.trim().to_owned(), cx);
                    });
                }
            },
        )
        .detach();

        Self {
            focus_handle,
            keymap_editor,
            shortcuts_search,
            keymap_error: None,
            overscroll: ElasticOverscroll::default(),
        }
    }
}

impl Render for ShortcutsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.theme().clone();
        let overscroll_y = self.overscroll.advance(cx);
        if self.overscroll.needs_frame(cx) {
            window.request_animation_frame();
        }
        self.keymap_editor.update(cx, |editor, cx| {
            editor.set_commands(Settings::global(cx).keymap.editor_commands(), cx);
        });

        div()
            .id("shortcuts-window")
            .track_focus(&self.focus_handle)
            .column()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.colors.canvas)
            .text_color(theme.colors.text)
            .child(
                div()
                    .column()
                    .flex_none()
                    .gap_token(&theme, Space::Xs)
                    .px(px(theme.space(Space::Xl)))
                    .pt(px(theme.space(Space::Xl)))
                    .child(text(&theme, TypeScale::Title, "Keyboard Shortcuts"))
                    .child(
                        text(
                            &theme,
                            TypeScale::Body,
                            "Click a shortcut, then press the new keys.",
                        )
                        .text_tone(&theme, TextTone::Muted),
                    )
                    .child(
                        div()
                            .w_full()
                            .pt(px(theme.space(Space::Sm)))
                            .child(self.shortcuts_search.clone()),
                    )
                    .children(self.keymap_error.as_ref().map(|error| {
                        let message = format!("Shortcut not changed: {error}");
                        div()
                            .id("shortcuts.error")
                            .child(
                                text(&theme, TypeScale::Caption, message.clone())
                                    .text_color(theme.colors.danger),
                            )
                            .semantic_in(
                                cx,
                                NodeSpec::new("shortcuts.error", Role::Status).text(message),
                            )
                    })),
            )
            .child(
                div()
                    .id("shortcuts-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(theme.space(Space::Xl)))
                    .on_scroll_wheel(cx.listener(|view, event, window, cx| {
                        if view.overscroll.handle_scroll(event, window, cx) {
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .relative()
                            .top(overscroll_y)
                            .child(self.keymap_editor.clone()),
                    ),
            )
    }
}
