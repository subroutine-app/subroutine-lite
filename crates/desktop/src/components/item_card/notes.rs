use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement, IntoElement, MouseButton,
    MouseCancelEvent, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, ScrollHandle,
    SharedString, Size, StatefulInteractiveElement, Styled, Window, canvas, div, point,
    prelude::FluentBuilder as _, px,
};
use gpui_kit::{
    content::{Markdown, MarkdownEvent, MarkdownTabNavigation},
    foundation::{Ident, StyledExt as _, ThemeOverlay},
    layout::{FadeEdges, ScrollFade},
};
use gpui_kit_theme::{ActiveTheme as _, ControlMetrics, Spacing, Theme, TypeStyle};
use subroutine_core::AnyItem;

use crate::{
    components::{
        elastic_overscroll::ElasticOverscroll,
        transition::{self, WindowTransitionExt as _},
    },
    selection::SelectionManager,
    settings::Settings,
};

use super::ItemCardDetails;

pub(super) struct NotesFocus {
    handle: FocusHandle,
    expanded: bool,
}

impl NotesFocus {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            handle: cx.focus_handle().tab_stop(false),
            expanded: false,
        }
    }

    pub(super) fn sync(
        &mut self,
        expanded: bool,
        row: &FocusHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> FocusHandle {
        if self.expanded && !expanded && self.handle.contains_focused(window, cx) {
            row.focus(window, cx);
        }
        self.expanded = expanded;
        self.handle.clone()
    }
}

pub(super) fn item_content(item: &AnyItem) -> Option<SharedString> {
    item.content()
        .filter(|content| !content.trim().is_empty())
        .map(SharedString::from)
}

fn notes_viewport_height(
    available_height: Pixels,
    natural_height: Option<Pixels>,
    expansion: f32,
    line_height: Pixels,
) -> Pixels {
    if line_height <= px(0.0) || available_height < line_height {
        return px(0.0);
    }
    let full = natural_height
        .unwrap_or(available_height)
        .min(available_height)
        .max(px(0.0));
    let preview = full.min(line_height * 4.0);
    preview + (full - preview) * expansion.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NotesPaint {
    edges: FadeEdges,
    document_opacity: f32,
}
fn notes_paint(
    natural_height: Option<Pixels>,
    viewport_height: Pixels,
    scroll_offset: Pixels,
) -> NotesPaint {
    let Some(natural_height) = natural_height else {
        return NotesPaint {
            edges: FadeEdges::default(),
            document_opacity: 0.0,
        };
    };
    NotesPaint {
        edges: scroll_fade_edges(
            scroll_offset,
            (natural_height - viewport_height).max(px(0.)),
        ),
        document_opacity: 1.0,
    }
}

const EDGE_TOLERANCE: Pixels = px(1.0);
const DOUBLE_CLICK_DRAG_THRESHOLD: f64 = 2.0;

fn measurements_match(first: Pixels, second: Pixels) -> bool {
    (first - second).abs() <= EDGE_TOLERANCE
}

fn rounded_scroll_range(content_height: Pixels, viewport_height: Pixels) -> Pixels {
    (((content_height - viewport_height) * 100.0).round() / 100.0).max(px(0.0))
}

fn scroll_fade_edges(offset: Pixels, max_offset: Pixels) -> FadeEdges {
    if max_offset <= EDGE_TOLERANCE {
        return FadeEdges::default();
    }
    let max_offset = f32::from(max_offset);
    let offset = f32::from(offset).clamp(-max_offset, 0.0);
    FadeEdges {
        top: offset < -f32::from(EDGE_TOLERANCE),
        bottom: max_offset + offset > f32::from(EDGE_TOLERANCE),
        ..FadeEdges::default()
    }
}

fn fade_band(theme_band: Pixels, line_height: Pixels, viewport_height: Pixels) -> Pixels {
    theme_band
        .min(line_height)
        .min(viewport_height / 2.0)
        .max(px(0.0))
}

fn compact_markdown_spacing(mut spacing: Spacing) -> Spacing {
    spacing.md = spacing.xs;
    spacing.xs = spacing.xxs;
    spacing
}

#[derive(Clone, Debug, PartialEq)]
struct MarkdownLayoutMetrics {
    font_families: [SharedString; 4],
    type_styles: [TypeStyle; 7],
    readout_scale: f32,
    spacing: Spacing,
    copy_button: ControlMetrics,
    hairline: f32,
    rail_width: f32,
}

impl MarkdownLayoutMetrics {
    fn new(theme: &Theme) -> Self {
        let typography = &theme.typography;
        Self {
            font_families: [
                typography.sans.clone(),
                typography.sans_fallback.clone(),
                typography.mono.clone(),
                typography.mono_fallback.clone(),
            ],
            type_styles: [
                typography.caption,
                typography.label,
                typography.body,
                typography.strong,
                typography.subtitle,
                typography.title,
                typography.code,
            ],
            readout_scale: typography.readout_scale,
            spacing: compact_markdown_spacing(theme.spacing),
            copy_button: theme.control.xs,
            hairline: theme.borders.hairline,
            rail_width: theme.effects.rail_width,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportMeasurement {
    viewport_height: Pixels,
    content_height: Pixels,
    max_offset: Pixels,
}

impl ViewportMeasurement {
    fn range_for(self, viewport_height: Pixels, content_height: Pixels) -> Option<Pixels> {
        (measurements_match(self.viewport_height, viewport_height)
            && measurements_match(self.content_height, content_height))
        .then_some(self.max_offset)
    }
}

#[derive(Default)]
struct DocumentMeasurement {
    source: SharedString,
    requested_width: Pixels,
    metrics: Option<MarkdownLayoutMetrics>,
    size: Option<Size<Pixels>>,
    viewport: Option<ViewportMeasurement>,
}

impl DocumentMeasurement {
    fn for_source(
        &mut self,
        source: &SharedString,
        width: Pixels,
        theme: &Theme,
    ) -> Option<Pixels> {
        let metrics = MarkdownLayoutMetrics::new(theme);
        if self.source != *source || self.metrics.as_ref() != Some(&metrics) {
            self.source = source.clone();
            self.metrics = Some(metrics);
            self.size = None;
            self.viewport = None;
        } else if self.requested_width != width
            && self
                .size
                .is_some_and(|size| !measurements_match(size.width, width))
        {
            self.size = None;
            self.viewport = None;
        }
        self.requested_width = width;
        self.size.map(|size| size.height)
    }

    fn record(&mut self, size: Size<Pixels>) -> bool {
        let size = Some(size.map(|length| length.max(px(0.0))));
        if self.size == size {
            return false;
        }
        self.size = size;
        self.viewport = None;
        true
    }

    fn paint(&self, viewport_height: Pixels, scroll_offset: Pixels) -> NotesPaint {
        let natural_height = self.size.map(|size| size.height);
        let mut paint = notes_paint(natural_height, viewport_height, scroll_offset);
        if let Some(max_offset) = natural_height.and_then(|height| {
            self.viewport
                .and_then(|viewport| viewport.range_for(viewport_height, height))
        }) {
            paint.edges = scroll_fade_edges(scroll_offset, max_offset);
        }
        paint
    }

    fn reconcile_viewport(
        &mut self,
        viewport_height: Pixels,
        scroll_offset: Pixels,
        expanded_max_offset: Option<Pixels>,
        painted_edges: FadeEdges,
    ) -> bool {
        let Some(size) = self.size else {
            return false;
        };
        let max_offset = expanded_max_offset
            .unwrap_or_else(|| rounded_scroll_range(size.height, viewport_height))
            .max(px(0.0));
        self.viewport = Some(ViewportMeasurement {
            viewport_height,
            content_height: size.height,
            max_offset,
        });
        scroll_fade_edges(scroll_offset, max_offset) != painted_edges
    }
}

fn notes_ident(element_id: &ElementId) -> Ident {
    Ident::new(format!("item-card-notes:{element_id:?}"))
}

pub(super) fn render_item_card_notes(
    element_id: ElementId,
    source: SharedString,
    available_size: Size<Pixels>,
    details: Option<ItemCardDetails>,
    tab_stop: bool,
    inspector_action: Option<Box<dyn gpui::Action>>,
    header_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let theme = cx.theme().clone();
    let line_height = px(theme.typography.body.line_height);
    let owner = window.current_view();

    let measurement =
        window.use_keyed_state((element_id.clone(), "notes-measurement"), cx, |_, _| {
            DocumentMeasurement::default()
        });
    let scroll_handle = window
        .use_keyed_state((element_id.clone(), "notes-scroll"), cx, |_, _| {
            ScrollHandle::new()
        })
        .read(cx)
        .clone();
    let overscroll =
        window.use_keyed_state((element_id.clone(), "notes-overscroll"), cx, |_, _| {
            ElasticOverscroll::default()
        });
    let expanded = details.as_ref().is_some_and(ItemCardDetails::expanded);
    let progress = details.as_ref().map_or(0.0, ItemCardDetails::progress);
    let scroll_return = window.keyed_transition(
        (element_id.clone(), "notes-scroll-return"),
        cx,
        transition::QUICK,
        || px(0.0),
    );
    if expanded {
        scroll_return.snap(scroll_handle.offset().y, cx);
    } else {
        scroll_return.set(px(0.0), cx);
    }
    let scroll_y = scroll_return.animate(window, cx);
    if !expanded {
        scroll_handle.set_offset(point(px(0.0), scroll_y));
    }
    let natural_height = measurement.update(cx, |state, _| {
        state.for_source(&source, available_size.width, &theme)
    });
    if let (Some(details), Some(natural_height)) = (&details, natural_height) {
        let limit = (window.viewport_size().height * 0.6)
            .min(px(480.0))
            .max(line_height * 4.0);
        if details.measure(header_height + natural_height.min(limit)) {
            window.on_next_frame(move |_, cx| cx.notify(owner));
        }
    }
    let viewport_height =
        notes_viewport_height(available_size.height, natural_height, progress, line_height);

    let overscroll_y = if expanded {
        let offset = overscroll.update(cx, |state, cx| state.advance(cx));
        if overscroll.read(cx).needs_frame(cx) {
            window.request_animation_frame();
        }
        offset
    } else {
        overscroll.update(cx, |state, _| state.reset());
        px(0.0)
    };
    let paint = measurement.read(cx).paint(viewport_height, scroll_y);
    let ident = notes_ident(&element_id);
    let markdown = Markdown::new(ident.child("markdown"), source)
        .tab_navigation(if expanded && tab_stop {
            MarkdownTabNavigation::Controls
        } else {
            MarkdownTabNavigation::None
        })
        .on_event(|event, _, cx| match event {
            MarkdownEvent::LinkClicked { href } => {
                cx.stop_propagation();
                cx.open_url(href.as_ref());
            }
            MarkdownEvent::CodeCopied { .. } | MarkdownEvent::CodeCopyRefused { .. } => {
                cx.stop_propagation();
            }
            _ => {}
        });
    let document_measurement = measurement.clone();
    let document = div()
        .w_full()
        .flex_none()
        .opacity(paint.document_opacity)
        .relative()
        .top(overscroll_y + if expanded { px(0.0) } else { scroll_y })
        .child(ThemeOverlay::new(
            |theme| {
                theme.clone().modify(|theme| {
                    theme.spacing = compact_markdown_spacing(theme.spacing);
                })
            },
            markdown,
        ))
        .on_children_prepainted(move |bounds, window, cx| {
            let Some(document) = bounds.first() else {
                return;
            };
            let changed = document_measurement.update(cx, |state, _| state.record(document.size));
            if changed {
                window.on_next_frame(move |_, cx| cx.notify(owner));
                window.request_animation_frame();
            }
        });

    let viewport = div()
        .id((element_id.clone(), "notes-viewport"))
        .w_full()
        .h(viewport_height)
        .overflow_hidden()
        .child(document)
        .when(expanded, |viewport| {
            let overscroll = overscroll.clone();
            viewport
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .on_scroll_wheel(move |event, window, cx| {
                    let changed =
                        overscroll.update(cx, |state, cx| state.handle_scroll(event, window, cx));
                    if changed {
                        cx.notify(owner);
                    }
                })
        });
    let scroll_for_fade = scroll_handle.clone();
    let inspectable = inspector_action.is_some();
    let double_click = window
        .use_keyed_state((element_id.clone(), "notes-double-click"), cx, |_, _| {
            Rc::new(Cell::new(None::<Point<Pixels>>))
        })
        .read(cx)
        .clone();

    Some(
        div()
            .on_children_prepainted(move |bounds, window, cx| {
                let Some(viewport) = bounds.first() else {
                    return;
                };
                let changed = measurement.update(cx, |state, _| {
                    state.reconcile_viewport(
                        viewport.size.height,
                        if expanded {
                            scroll_for_fade.offset().y
                        } else {
                            scroll_y
                        },
                        expanded.then(|| scroll_for_fade.max_offset().y),
                        paint.edges,
                    )
                });
                if changed {
                    window.on_next_frame(move |_, cx| cx.notify(owner));
                    window.request_animation_frame();
                }
            })
            .id((element_id, "notes"))
            .column()
            .relative()
            .w_full()
            .h(viewport_height)
            .flex_none()
            .on_mouse_down(MouseButton::Left, {
                let candidate = Rc::clone(&double_click);
                move |event, _, cx| {
                    SelectionManager::claim_press(cx);
                    cx.stop_propagation();
                    candidate.set(
                        (inspectable
                            && event.click_count == 2
                            && !Settings::global(cx).selection.is_modified(&event.modifiers))
                        .then_some(event.position),
                    );
                }
            })
            .capture_any_mouse_up({
                let candidate = Rc::clone(&double_click);
                move |event, window, cx| {
                    if event.button != MouseButton::Left {
                        return;
                    }
                    let Some(start) = candidate.get() else {
                        return;
                    };
                    if !inspectable
                        || event.click_count != 2
                        || Settings::global(cx).selection.is_modified(&event.modifiers)
                        || (event.position - start).magnitude() > DOUBLE_CLICK_DRAG_THRESHOLD
                    {
                        candidate.set(None);
                        return;
                    }
                    // Only a stationary release overrides Markdown's word selection.
                    if !window.document_selection().is_empty() {
                        window.clear_document_selection();
                        window.release_pointer();
                        window.refresh();
                    }
                }
            })
            .on_click({
                let candidate = Rc::clone(&double_click);
                move |event, window, cx| {
                    cx.stop_propagation();
                    if candidate.take().is_some()
                        && event.click_count() == 2
                        && !Settings::global(cx)
                            .selection
                            .is_modified(&window.modifiers())
                        && let Some(action) = &inspector_action
                    {
                        window.dispatch_action(action.boxed_clone(), cx);
                    }
                }
            })
            .child(
                ScrollFade::new(ident.child("fade"))
                    .fit_height()
                    .edges(paint.edges)
                    .band(f32::from(fade_band(
                        px(theme.effects.edge_fade_band),
                        line_height,
                        viewport_height,
                    )))
                    .child(viewport),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let candidate = Rc::clone(&double_click);
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, _| {
                            if phase.capture() {
                                candidate.set(None);
                            }
                        });
                        // Observe excursions outside the notes without taking text's capture.
                        let candidate = Rc::clone(&double_click);
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, _| {
                            if phase.capture()
                                && candidate.get().is_some_and(|start| {
                                    event.pressed_button != Some(MouseButton::Left)
                                        || (event.position - start).magnitude()
                                            > DOUBLE_CLICK_DRAG_THRESHOLD
                                })
                            {
                                candidate.set(None);
                            }
                        });
                        window.on_mouse_event(move |_: &MouseCancelEvent, phase, _, _| {
                            if phase.capture() {
                                double_click.set(None);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_card::ItemCardStates;
    use gpui::{AppContext as _, Context, Render, TestAppContext};

    struct NotesFocusTestView {
        row: FocusHandle,
        child: FocusHandle,
        outside: FocusHandle,
        details: ItemCardDetails,
    }

    impl Render for NotesFocusTestView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let notes = window
                .use_keyed_state((ElementId::from("card"), "notes-focus"), cx, |_, cx| {
                    NotesFocus::new(cx)
                })
                .update(cx, |state, cx| {
                    state.sync(self.details.expanded(), &self.row, window, cx)
                });
            div()
                .size_full()
                .child(
                    div().id("row").track_focus(&self.row).child(
                        div()
                            .id("notes")
                            .track_focus(&notes)
                            .child(div().id("child").track_focus(&self.child)),
                    ),
                )
                .child(div().id("outside").track_focus(&self.outside))
        }
    }

    #[gpui::test]
    fn notes_focus_restores_only_on_focused_collapse(cx: &mut TestAppContext) {
        let states = ItemCardStates::default();
        let details = states.get(uuid::Uuid::nil());
        let handle = cx.add_window(move |_, cx| NotesFocusTestView {
            row: cx.focus_handle().tab_stop(true),
            child: cx.focus_handle().tab_stop(false),
            outside: cx.focus_handle().tab_stop(true),
            details,
        });
        let [row, child, outside] = handle
            .update(cx, |view, _, _| {
                [view.row.clone(), view.child.clone(), view.outside.clone()]
            })
            .unwrap();

        for (focused, expected) in [(&child, &row), (&outside, &outside)] {
            handle
                .update(cx, |view, window, cx| {
                    view.details.toggle(&view.row, None, window, cx);
                    cx.notify();
                })
                .unwrap();
            cx.update_window(handle.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                window.focus_from_pointer(focused, cx);
                assert!(states.collapse_all());
                window.refresh();
                window.draw(cx).clear(cx);
                assert_eq!(window.focused(cx).as_ref(), Some(expected));

                window.focus_from_pointer(&child, cx);
                for _ in 0..2 {
                    window.refresh();
                    window.draw(cx).clear(cx);
                    assert!(
                        child.is_focused(window),
                        "collapsed notes retain pointer focus"
                    );
                }
            })
            .unwrap();
        }
    }
}
