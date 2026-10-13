use chrono::NaiveDate;
use gpui::{
    AppContext, Context, InteractiveElement, IntoElement, MouseButton, MouseCancelEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, StatefulInteractiveElement,
    Styled, Window, canvas, div, prelude::FluentBuilder, px,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, Marker};
use uuid::Uuid;

use crate::{
    dates::InclusiveDateRange,
    haptics::HapticsExt as _,
    item_manager::ItemManager,
    selection::SelectionManager,
    stores::{AppDatabaseStore, WorkspaceContent},
};

use super::CalendarView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MarkerResizeEdge {
    Start,
    End,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MarkerResizeDrag {
    pub item_id: Uuid,
    pub edge: MarkerResizeEdge,
}

pub(super) struct MarkerResize {
    pub item_id: Uuid,
    workspace_generation: u64,
    edge: MarkerResizeEdge,
    original: InclusiveDateRange,
    original_end_date: Option<NaiveDate>,
    preview: InclusiveDateRange,
}

impl MarkerResize {
    fn new(marker: &Marker, edge: MarkerResizeEdge, workspace_generation: u64) -> Option<Self> {
        let end = marker.end_date.unwrap_or(marker.date);
        if end < marker.date {
            return None;
        }
        let original = InclusiveDateRange::new(marker.date, end);
        Some(Self {
            item_id: marker.id,
            workspace_generation,
            edge,
            original,
            original_end_date: marker.end_date,
            preview: original,
        })
    }

    fn update(&mut self, date: NaiveDate) -> bool {
        let preview = match self.edge {
            MarkerResizeEdge::Start => {
                InclusiveDateRange::new(date.min(self.original.end()), self.original.end())
            }
            MarkerResizeEdge::End => {
                InclusiveDateRange::new(self.original.start(), date.max(self.original.start()))
            }
        };
        if preview == self.preview {
            return false;
        }
        self.preview = preview;
        true
    }

    fn is_current(&self, generation: u64, marker: &Marker) -> bool {
        self.workspace_generation == generation
            && self.item_id == marker.id
            && self.original.start() == marker.date
            && self.original_end_date == marker.end_date
    }

    fn preview_marker(&self, mut marker: Marker) -> Marker {
        if marker.id == self.item_id && self.preview != self.original {
            marker.date = self.preview.start();
            marker.end_date = (self.preview.end() != marker.date).then_some(self.preview.end());
        }
        marker
    }

    fn committed_marker(self, generation: u64, marker: Marker) -> Option<Marker> {
        if !self.is_current(generation, &marker) || self.preview == self.original {
            return None;
        }
        Some(self.preview_marker(marker))
    }
}

struct MarkerResizeGhost;

impl Render for MarkerResizeGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl CalendarView {
    pub(super) fn render_marker_resize_handle(
        id: Uuid,
        edge: MarkerResizeEdge,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id(format!("calendar-marker-resize-{id}-{edge:?}"))
            .absolute()
            .top_0()
            .map(|handle| match edge {
                MarkerResizeEdge::Start => handle.left_0(),
                MarkerResizeEdge::End => handle.right_0(),
            })
            .w(px(8.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .role(gpui::Role::Button)
            .aria_label(match edge {
                MarkerResizeEdge::Start => "Resize marker start date",
                MarkerResizeEdge::End => "Resize marker end date",
            })
            .block_mouse_except_scroll()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_pressure(|_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_drag(
                MarkerResizeDrag { item_id: id, edge },
                move |drag, _, window, cx| {
                    entity.update(cx, |view, cx| {
                        view.begin_marker_resize(drag.item_id, drag.edge, window, cx);
                    });
                    cx.new(|_| MarkerResizeGhost)
                },
            )
            .opacity(0.)
            .hover(|style| style.opacity(0.5))
            .child(
                div()
                    .w(px(2.))
                    .h(px(14.))
                    .rounded_full()
                    .bg(cx.theme().colors.text),
            )
    }

    pub(super) fn marker_resize_hook(&self, cx: &Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moved = entity.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if !phase.capture() {
                        return;
                    }
                    moved.update(cx, |view, cx| {
                        if view.marker_resize.is_none() {
                            return;
                        }
                        if event.pressed_button == Some(MouseButton::Left) {
                            view.update_marker_resize_at(event.position, cx);
                        } else {
                            view.cancel_marker_resize(cx);
                            cx.stop_active_drag(window);
                        }
                        cx.stop_propagation();
                    });
                });

                let released = entity.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase.capture() && event.button == MouseButton::Left {
                        released.update(cx, |view, cx| {
                            if view.marker_resize.is_some() {
                                view.commit_marker_resize_at(event.position, cx);
                                cx.stop_propagation();
                            }
                        });
                    }
                });

                window.on_mouse_event(move |_: &MouseCancelEvent, phase, _, cx| {
                    if phase.capture() {
                        entity.update(cx, |view, cx| {
                            view.cancel_marker_resize(cx);
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    fn begin_marker_resize(
        &mut self,
        id: Uuid,
        edge: MarkerResizeEdge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marker_resize.is_some()
            || self.editing_marker_id == Some(id)
            || ItemManager::global(cx).read(cx).is_being_edited(id)
        {
            return;
        }
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        if store.workspace_content() != WorkspaceContent::Ready {
            return;
        }
        let Some(AnyItem::Marker(marker)) = store.get_item(id) else {
            return;
        };
        let Some(resize) = MarkerResize::new(&marker, edge, store.workspace_generation()) else {
            return;
        };

        self.cancel_scroll();
        self.deselect_dates(cx);
        self.clear_drop_target(cx);
        self.marquee.end();
        self.hovered_date = None;
        self.force_click_opened = false;
        self.suppress_date_click = false;
        self.month_selector_open = false;
        if self.inspector_date.is_some() {
            self.close_day_inspector(window, cx);
        }
        self.focus_handle.focus(window, cx);

        self.marker_resize = Some(resize);
        self.update_marker_resize_at(window.mouse_position(), cx);
        cx.notify();
    }

    pub(super) fn update_marker_resize_at(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.validate_marker_resize(cx);
        if self.marker_resize.is_none() {
            return;
        }
        let Some(date) = self.date_at_position(position) else {
            self.set_edge_scroll_speed(None, cx);
            return;
        };
        if let Some(resize) = self.marker_resize.as_mut()
            && resize.update(date)
        {
            cx.play_alignment_haptic();
            cx.notify();
        }
        self.set_edge_scroll_speed(self.edge_scroll_speed_at(position), cx);
    }

    pub(super) fn cancel_marker_resize(&mut self, cx: &mut Context<Self>) -> bool {
        if self.marker_resize.take().is_none() {
            return false;
        }
        self.set_edge_scroll_speed(None, cx);
        cx.notify();
        true
    }

    pub(super) fn resizing_marker(&self, marker: Marker) -> Marker {
        match &self.marker_resize {
            Some(resize) => resize.preview_marker(marker),
            None => marker,
        }
    }

    pub(super) fn validate_marker_resize(&mut self, cx: &mut Context<Self>) {
        let Some(resize) = self.marker_resize.as_ref() else {
            return;
        };
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        let current = match store.get_item(resize.item_id) {
            Some(AnyItem::Marker(marker)) => {
                resize.is_current(store.workspace_generation(), &marker)
            }
            _ => false,
        };
        if !current
            || ItemManager::global(cx)
                .read(cx)
                .is_being_edited(resize.item_id)
        {
            self.cancel_marker_resize(cx);
        }
    }

    fn commit_marker_resize_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.date_at_position(position).is_none() {
            self.cancel_marker_resize(cx);
            return;
        }
        self.update_marker_resize_at(position, cx);
        let Some(resize) = self.marker_resize.take() else {
            return;
        };
        self.set_edge_scroll_speed(None, cx);
        cx.notify();

        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let Some(AnyItem::Marker(marker)) = store.get_item(resize.item_id) else {
                return;
            };
            if let Some(marker) = resize.committed_marker(store.workspace_generation(), marker) {
                let _ = store.update_items(vec![AnyItem::Marker(marker)], cx);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    #[test]
    fn endpoints_snap_inclusively_and_clamp_without_moving_the_other_end() {
        let marker = Marker::new("Range", date(1, 28)).with_end_date(date(2, 3));
        let mut start = MarkerResize::new(&marker, MarkerResizeEdge::Start, 1).unwrap();
        start.update(date(1, 30));
        assert_eq!(
            start.preview,
            InclusiveDateRange::new(date(1, 30), date(2, 3))
        );
        assert_eq!(start.preview.day_count(), 5);
        start.update(date(2, 8));
        assert_eq!(start.preview, InclusiveDateRange::single(date(2, 3)));
        start.update(date(1, 20));
        assert_eq!(
            start.preview,
            InclusiveDateRange::new(date(1, 20), date(2, 3))
        );

        let mut end = MarkerResize::new(&marker, MarkerResizeEdge::End, 1).unwrap();
        end.update(date(2, 1));
        assert_eq!(
            end.preview,
            InclusiveDateRange::new(date(1, 28), date(2, 1))
        );
        assert_eq!(end.preview.day_count(), 5);
        end.update(date(1, 20));
        assert_eq!(end.preview, InclusiveDateRange::single(date(1, 28)));
        end.update(date(2, 8));
        assert_eq!(
            end.preview,
            InclusiveDateRange::new(date(1, 28), date(2, 8))
        );
    }

    #[test]
    fn single_day_no_ops_preserve_both_end_encodings_and_do_not_commit() {
        for end_date in [None, Some(date(2, 1))] {
            let mut marker = Marker::new("Day", date(2, 1));
            marker.end_date = end_date;
            for edge in [MarkerResizeEdge::Start, MarkerResizeEdge::End] {
                let mut resize = MarkerResize::new(&marker, edge, 1).unwrap();
                assert!(!resize.update(marker.date));
                let (extension, crossing) = match edge {
                    MarkerResizeEdge::Start => (date(1, 31), date(2, 2)),
                    MarkerResizeEdge::End => (date(2, 2), date(1, 31)),
                };
                assert!(resize.update(extension));
                assert_eq!(resize.preview.day_count(), 2);
                assert!(resize.update(crossing));
                assert_eq!(resize.preview, InclusiveDateRange::single(marker.date));
                assert_eq!(resize.preview_marker(marker.clone()).end_date, end_date);
                assert!(resize.committed_marker(1, marker.clone()).is_none());
            }
        }
    }

    #[test]
    fn commit_merges_only_dates_and_rejects_stale_state() {
        let marker = Marker::new("Original", date(1, 28)).with_end_date(date(2, 3));
        let resized = || {
            let mut resize = MarkerResize::new(&marker, MarkerResizeEdge::End, 7).unwrap();
            resize.update(date(1, 28));
            resize
        };
        let mut fresh = marker.clone();
        fresh.title = "Edited title".into();
        fresh.content = Some("Edited content".into());
        let committed = resized().committed_marker(7, fresh.clone()).unwrap();
        assert_eq!(committed.date, marker.date);
        assert_eq!(committed.end_date, None);
        assert_eq!(committed.title, fresh.title);
        assert_eq!(committed.content, fresh.content);
        assert!(resized().committed_marker(8, fresh.clone()).is_none());

        fresh.date = date(1, 29);
        assert!(resized().committed_marker(7, fresh).is_none());
        let mut changed_end = marker.clone();
        changed_end.end_date = Some(date(2, 4));
        assert!(resized().committed_marker(7, changed_end).is_none());

        let other = Marker::new("Other", marker.date).with_end_date(date(2, 3));
        let preview = resized().preview_marker(other.clone());
        assert_eq!(preview.date, other.date);
        assert_eq!(preview.end_date, other.end_date);
        assert!(resized().committed_marker(7, other).is_none());

        let mut single = Marker::new("Day", date(2, 1));
        let mut resize = MarkerResize::new(&single, MarkerResizeEdge::End, 7).unwrap();
        resize.update(date(2, 2));
        single.end_date = Some(single.date);
        assert!(resize.committed_marker(7, single).is_none());
    }
}
