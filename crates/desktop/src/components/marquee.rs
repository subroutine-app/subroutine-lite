use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::components::ext::ElementExt;
use gpui::{
    App, Bounds, Context, CursorStyle, Entity, FocusHandle, InteractiveElement, IntoElement,
    MouseButton, MouseCancelEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    Pixels, Point, ScrollHandle, Styled, Window, canvas, div, point, prelude::FluentBuilder, px,
};
use gpui_kit_theme::ActiveTheme;
use uuid::Uuid;

use crate::{
    selection::{SelectionManager, SelectionScope},
    settings::Settings,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    origin: Point<Pixels>,
    current: Point<Pixels>,
    engaged: bool,
}

impl Band {
    const THRESHOLD: Pixels = px(4.);

    fn new(origin: Point<Pixels>) -> Self {
        Self {
            origin,
            current: origin,
            engaged: false,
        }
    }

    fn drag_to(&mut self, position: Point<Pixels>) -> bool {
        self.current = position;
        if !self.engaged {
            let dx = (position.x - self.origin.x).abs();
            let dy = (position.y - self.origin.y).abs();
            self.engaged = dx >= Self::THRESHOLD || dy >= Self::THRESHOLD;
        }
        self.engaged
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let left = self.origin.x.min(self.current.x);
        let right = self.origin.x.max(self.current.x);
        let top = self.origin.y.min(self.current.y);
        let bottom = self.origin.y.max(self.current.y);
        Bounds::from_corners(point(left, top), point(right, bottom))
    }
}

fn scroll_velocity(position: Point<Pixels>, viewport: Bounds<Pixels>) -> f32 {
    const EDGE_ZONE: f32 = 56.;
    const MAX_SPEED: f32 = 1080.;

    if viewport.is_empty() || position.x < viewport.left() || position.x > viewport.right() {
        return 0.;
    }
    let zone = EDGE_ZONE.min(f32::from(viewport.size.height) / 2.);
    let y = f32::from(position.y - viewport.top());
    let height = f32::from(viewport.size.height);
    if y < zone {
        (1. - y / zone).clamp(0., 1.).powi(2) * MAX_SPEED
    } else if y > height - zone {
        -(1. - (height - y) / zone).clamp(0., 1.).powi(2) * MAX_SPEED
    } else {
        0.
    }
}

struct ScrollTracking {
    handle: ScrollHandle,
    offset: Point<Pixels>,
    last_frame: Option<Instant>,
}

impl ScrollTracking {
    fn new(handle: ScrollHandle) -> Self {
        Self {
            offset: handle.offset(),
            handle,
            last_frame: None,
        }
    }

    fn advance(
        &mut self,
        band: &mut Band,
        offset: Point<Pixels>,
        viewport: Bounds<Pixels>,
        max_scroll: Pixels,
        now: Instant,
    ) -> Point<Pixels> {
        band.origin += offset - self.offset;
        let elapsed = self
            .last_frame
            .map_or(Duration::from_secs_f32(1. / 60.), |last| {
                now.saturating_duration_since(last)
                    .min(Duration::from_millis(50))
            });
        let speed = if band.engaged {
            scroll_velocity(band.current, viewport)
        } else {
            0.
        };
        let next = point(
            offset.x,
            (offset.y + px(speed * elapsed.as_secs_f32())).clamp(-max_scroll.max(px(0.)), px(0.)),
        );
        band.origin += next - offset;
        self.offset = next;
        self.last_frame = (next != offset).then_some(now);
        next
    }
}

#[derive(Clone, Copy)]
struct MarqueeCard {
    id: Uuid,
    bounds: Bounds<Pixels>,
}

struct ScrollRegion {
    handle: ScrollHandle,
    ids: HashSet<Uuid>,
}

pub struct MarqueeSelection {
    scope: SelectionScope,
    band: Option<Band>,
    base: Vec<Uuid>,
    cards: Vec<MarqueeCard>,
    viewport: Option<Bounds<Pixels>>,
    scroll: Option<ScrollTracking>,
    scroll_region: Option<ScrollRegion>,
    focus: Option<FocusHandle>,
}

impl MarqueeSelection {
    pub fn new(scope: SelectionScope) -> Self {
        Self {
            scope,
            band: None,
            base: Vec::new(),
            cards: Vec::new(),
            viewport: None,
            scroll: None,
            scroll_region: None,
            focus: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.band.is_some_and(|band| band.engaged)
    }

    fn has_pending_press(&self) -> bool {
        self.band.is_some()
    }

    pub fn begin(
        &mut self,
        position: Point<Pixels>,
        focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if SelectionManager::global(cx)
            .read(cx)
            .card_at(self.scope, position)
            .is_some()
        {
            return false;
        }
        if SelectionManager::global(cx).read(cx).press_is_claimed() {
            return false;
        }

        let additive = Settings::global(cx)
            .selection
            .is_modified(&window.modifiers());
        self.base = if additive {
            SelectionManager::claim_press(cx);
            let selection = SelectionManager::global(cx);
            let selection = selection.read(cx);
            if selection.has_selection_in(self.scope) {
                selection.ids().to_vec()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        self.cards.clear();
        self.scroll = None;
        self.scroll_region = None;
        self.focus = focus;
        self.band = Some(Band::new(position));
        true
    }

    pub fn begin_in_scroll_region(
        &mut self,
        position: Point<Pixels>,
        focus: Option<FocusHandle>,
        handle: &ScrollHandle,
        ids: impl IntoIterator<Item = Uuid>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !self.begin(position, focus, window, cx) {
            return false;
        }
        self.scroll_region = Some(ScrollRegion {
            handle: handle.clone(),
            ids: ids.into_iter().collect(),
        });
        true
    }

    pub fn has_scroll_region(&self) -> bool {
        self.scroll_region.is_some()
    }

    pub fn scroll_region(&mut self, window: &mut Window) -> bool {
        let Some(handle) = self
            .scroll_region
            .as_ref()
            .map(|region| region.handle.clone())
        else {
            return false;
        };
        self.scroll(&handle, px(0.), window)
    }

    pub fn drag_to(&mut self, position: Point<Pixels>, cx: &mut App) {
        let Some(band) = self.band.as_mut() else {
            return;
        };
        if !band.drag_to(position) {
            return;
        }

        self.apply_selection(cx);
    }

    fn apply_selection(&self, cx: &mut App) {
        let ids = self.selected_ids();

        let scope = self.scope;
        SelectionManager::global(cx).update(cx, |selection, cx| {
            if selection.scope() != (!ids.is_empty()).then_some(scope) || selection.ids() != ids {
                selection.select_many(scope, ids, cx);
            }
        });
    }

    pub fn end(&mut self) -> bool {
        self.base.clear();
        self.cards.clear();
        self.scroll = None;
        self.scroll_region = None;
        self.focus = None;
        self.band.take().is_some()
    }

    fn selected_ids(&self) -> Vec<Uuid> {
        let mut ids = self.base.clone();
        if let Some(band) = self.band {
            for card in &self.cards {
                if !card.bounds.is_empty() && card.bounds.intersects(&band.bounds()) {
                    ids.push(card.id);
                }
            }
        }
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(*id));
        ids
    }

    pub fn scrolled_by(&mut self, delta: Point<Pixels>) {
        if let Some(band) = self.band.as_mut() {
            band.origin += delta;
        }
        for card in &mut self.cards {
            card.bounds.origin += delta;
        }
    }

    pub fn scroll(
        &mut self,
        handle: &ScrollHandle,
        top_inset: Pixels,
        window: &mut Window,
    ) -> bool {
        if !window.is_window_active()
            || self
                .focus
                .as_ref()
                .is_some_and(|focus| !focus.is_focused(window))
        {
            self.end();
            return false;
        }
        let Some(band) = self.band.as_mut() else {
            return false;
        };
        let bounds = handle.bounds();
        let horizontal = self.scroll_region.as_ref().map_or_else(
            || self.viewport.unwrap_or(bounds),
            |region| region.handle.bounds(),
        );
        let viewport = Bounds::from_corners(
            point(
                horizontal.left(),
                (bounds.top() + top_inset).min(bounds.bottom()),
            ),
            point(horizontal.right(), bounds.bottom()),
        );
        let offset = handle.offset();
        let scroll = self
            .scroll
            .get_or_insert_with(|| ScrollTracking::new(handle.clone()));
        let previous = scroll.offset;
        let next = scroll.advance(
            band,
            offset,
            viewport,
            handle.max_offset().y,
            Instant::now(),
        );
        for card in &mut self.cards {
            card.bounds.origin += next - previous;
        }
        if next == offset {
            return false;
        }
        handle.set_offset(next);
        window.request_animation_frame();
        true
    }

    fn refresh_selection(&mut self, cx: &mut App) {
        if let Some(scroll) = self.scroll.as_mut() {
            let offset = scroll.handle.offset();
            let delta = offset - scroll.offset;
            scroll.offset = offset;
            self.scrolled_by(delta);
        }
        if !self.is_active() {
            return;
        }
        {
            let selection = SelectionManager::global(cx);
            let selection = selection.read(cx);
            self.refresh_cards(selection.card_bounds(self.scope));
        }
        self.apply_selection(cx);
    }

    fn refresh_cards(&mut self, cards: impl IntoIterator<Item = (Uuid, Bounds<Pixels>)>) {
        let current: Vec<_> = cards
            .into_iter()
            .filter(|(id, _)| {
                self.scroll_region
                    .as_ref()
                    .is_none_or(|region| region.ids.contains(id))
            })
            .map(|(id, bounds)| MarqueeCard { id, bounds })
            .collect();
        let replaced: HashSet<_> = current.iter().map(|card| card.id).collect();
        self.cards.retain(|card| !replaced.contains(&card.id));
        self.cards.extend(current);
    }

    fn set_viewport(&mut self, bounds: Bounds<Pixels>) {
        self.viewport = Some(bounds);
    }
}

pub trait MarqueeView: 'static + Sized {
    fn marquee(&self) -> &MarqueeSelection;
    fn marquee_mut(&mut self) -> &mut MarqueeSelection;

    fn marquee_dragged(&mut self, _position: Point<Pixels>, _cx: &mut Context<Self>) {}

    fn marquee_ended(&mut self, _cx: &mut Context<Self>) {}

    fn marquee_enabled(&self, _cx: &App) -> bool {
        true
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        None
    }
}

pub fn marquee<V, E>(element: E, view: &V, cx: &mut Context<V>) -> E
where
    V: MarqueeView,
    E: InteractiveElement + ParentElement,
{
    let entity = cx.entity();
    element
        .capture_key_down({
            let entity = entity.clone();
            move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    entity.update(cx, |view, cx| {
                        if view.marquee_mut().end() {
                            view.marquee_ended(cx);
                            cx.notify();
                        }
                    });
                }
            }
        })
        .on_mouse_down(MouseButton::Left, {
            let entity = entity.clone();
            move |event: &MouseDownEvent, window, cx| {
                let position = event.position;
                entity.update(cx, |view, cx| {
                    if view.marquee().has_pending_press() || !view.marquee_enabled(cx) {
                        return;
                    }
                    let focus = view.marquee_focus(cx);
                    if view
                        .marquee_mut()
                        .begin(position, focus.clone(), window, cx)
                    {
                        if let Some(home) = focus {
                            home.focus(window, cx);
                        }
                        cx.notify();
                    }
                });
            }
        })
        .child(marquee_layer(view, cx))
}

fn marquee_layer<V: MarqueeView>(view: &V, cx: &mut Context<V>) -> impl IntoElement {
    let entity = cx.entity();
    let pending = view.marquee().has_pending_press();

    div()
        .absolute()
        .inset_0()
        .on_prepaint({
            let entity = entity.clone();
            move |bounds, _, cx| {
                entity.update(cx, |view, _| view.marquee_mut().set_viewport(bounds));
            }
        })
        .when(pending, |this| this.child(marquee_hook(entity)))
}

fn marquee_hook<V: MarqueeView>(entity: Entity<V>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            entity.update(cx, |view, cx| {
                if view
                    .marquee_focus(cx)
                    .is_some_and(|focus| !focus.contains_focused(window, cx))
                {
                    if view.marquee_mut().end() {
                        view.marquee_ended(cx);
                        cx.notify();
                    }
                } else {
                    view.marquee_mut().refresh_selection(cx);
                }
            });
            if let Some(band) = entity.read(cx).marquee().band.filter(|band| band.engaged) {
                window.set_window_cursor_style(CursorStyle::Crosshair);
                let accent = cx.theme().colors.focus;
                window
                    .paint_quad(gpui::fill(band.bounds(), accent.alpha(0.1)).corner_radii(px(2.)));
                window.paint_quad(
                    gpui::outline(band.bounds(), accent, gpui::BorderStyle::default())
                        .corner_radii(px(2.))
                        .border_widths(px(1.)),
                );
            }

            let moved = entity.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
                if !phase.bubble() {
                    return;
                }
                match event.pressed_button {
                    Some(MouseButton::Left) => moved.update(cx, |view, cx| {
                        view.marquee_mut().drag_to(event.position, cx);
                        view.marquee_dragged(event.position, cx);
                        cx.notify();
                    }),
                    _ => moved.update(cx, |view, cx| {
                        if view.marquee_mut().end() {
                            view.marquee_ended(cx);
                            cx.notify();
                        }
                    }),
                }
            });

            let cancelled = entity.clone();
            window.on_mouse_event(move |_: &MouseCancelEvent, phase, _, cx| {
                if phase.capture() {
                    cancelled.update(cx, |view, cx| {
                        if view.marquee_mut().end() {
                            view.marquee_ended(cx);
                            cx.notify();
                        }
                    });
                }
            });
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
                if phase.capture() {
                    entity.update(cx, |view, cx| {
                        if view.marquee_mut().end() {
                            view.marquee_ended(cx);
                            cx.notify();
                        }
                    });
                }
            });
        },
    )
    .absolute()
    .size_full()
}
