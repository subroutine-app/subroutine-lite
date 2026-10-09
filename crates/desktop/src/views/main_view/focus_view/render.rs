use gpui::{
    Context, DragMoveEvent, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_kit::{
    controls::button::IconButton,
    foundation::{Selectable as _, Sizable as _, StyledExt as _},
    layout::ScrollArea,
};
use gpui_kit_assets::Icon as KitIcon;

use crate::{
    components::{DragData, DraggedItems, DropZone, ext::ElementExt as _, menu::MenuBuilder},
    item_manager::ItemManager,
    selection::CompleteSelected,
    settings::{FocusCarouselOrientation, Settings},
    stores::AppDatabaseStore,
    views::{
        StartItemCreator, StartQueuedActionCreator, TOP_EDGE_INSET, drop_confirmation::confirm_drop,
    },
};

use super::super::tab::{GoToNow, MainViewTab, RefreshPipeline};
use super::{
    FocusMode, FocusView,
    notices::{MIN_CAROUSEL_ROOM, NOTICE_GAP},
    temporal::TemporalSnapshot,
};

fn focus_context_menu(has_items: bool, orientation: FocusCarouselOrientation) -> MenuBuilder {
    MenuBuilder::new()
        .item("New queued action", |window, cx| {
            window.dispatch_action(Box::new(StartQueuedActionCreator(None)), cx);
        })
        .item_with_keybinding("New item…", StartItemCreator, |window, cx| {
            window.dispatch_action(Box::new(StartItemCreator), cx);
        })
        .separator()
        .submenu("Carousel direction", |menu| {
            menu.check(
                "Horizontal",
                orientation == FocusCarouselOrientation::Horizontal,
                |_window, cx| {
                    Settings::update(cx, |settings| {
                        settings.focus_carousel_orientation = FocusCarouselOrientation::Horizontal
                    });
                },
            )
            .check(
                "Vertical",
                orientation == FocusCarouselOrientation::Vertical,
                |_window, cx| {
                    Settings::update(cx, |settings| {
                        settings.focus_carousel_orientation = FocusCarouselOrientation::Vertical
                    });
                },
            )
        })
        .separator()
        .when(has_items, |menu| {
            menu.item_with_keybinding("Complete current item", CompleteSelected, |window, cx| {
                window.dispatch_action(Box::new(CompleteSelected), cx);
            })
            .item_with_keybinding("Go to first item", GoToNow, |window, cx| {
                window.dispatch_action(Box::new(GoToNow), cx);
            })
        })
        .item_with_keybinding("Refresh queue", RefreshPipeline, |window, cx| {
            window.dispatch_action(Box::new(RefreshPipeline), cx);
        })
}

impl FocusView {
    pub(in crate::views::main_view) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        if self.drop_active {
            self.drop_active = false;
            cx.notify();
        }
    }

    fn drop_zone(&self, cx: &mut Context<Self>) -> DropZone<DragData<DraggedItems>> {
        DropZone::new("focus-drop")
            .size_full()
            .active(self.drop_active)
            .rounded_none()
            .rounded_bl_2xl()
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<DragData<DraggedItems>>, _window, cx| {
                    if this.mode != FocusMode::Action {
                        if this.drop_active {
                            this.drop_active = false;
                            cx.notify();
                        }
                        return;
                    }
                    let accepts = event
                        .drag(cx)
                        .data
                        .actions()
                        .any(|action| !this.items.iter().any(|item| item.id() == action.id));
                    let active = accepts
                        && event.bounds.contains(&event.event.position)
                        && event.event.position.y >= event.bounds.origin.y + TOP_EDGE_INSET;
                    if active != this.drop_active {
                        this.drop_active = active;
                        cx.notify();
                    }
                },
            ))
            .on_drop(
                cx.listener(|this, data: &DragData<DraggedItems>, window, cx| {
                    if this.mode != FocusMode::Action || !this.drop_active {
                        return;
                    }
                    let ids: Vec<_> = data
                        .data
                        .actions()
                        .map(|action| action.id)
                        .filter(|id| !this.items.iter().any(|item| item.id() == *id))
                        .collect();
                    let store = AppDatabaseStore::global(cx);
                    let updated = match store.read(cx).plan_queue_actions(&ids, cx) {
                        Ok(updated) => updated,
                        Err(error) => {
                            gpui_kit::overlay::toast::push(window, cx,
                                crate::components::timed_toast("item.save-failed", error)
                                    .tone(gpui_kit::display::badge::Tone::Warning));
                            this.clear_drop_target(cx);
                            return;
                        }
                    };
                    this.clear_drop_target(cx);
                    confirm_drop(
                        updated.len(),
                        "Queue",
                        "Add the dropped actions to the queue. This count includes other actions that will be rescheduled to make room.".into(),
                        window,
                        cx,
                        move |_, cx| {
                            store.update(cx, |store, cx| {
                                let _ = store.queue_actions(&ids, cx);
                            });
                        },
                    );
                }),
            )
    }
}

impl Render for FocusView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_workspace(
            AppDatabaseStore::global(cx).read(cx).workspace_generation(),
            cx,
        );
        self.reconcile_events(window, cx);
        if self.mode == FocusMode::Action && self.restore_focus {
            self.restore_focus = false;
            if self.focus_handle.contains_focused(window, cx) {
                let expected = self.active_id;
                let handle = expected
                    .and_then(|id| self.item_focus_handles.get(&id).cloned())
                    .unwrap_or_else(|| self.focus_handle.clone());
                cx.on_next_frame(window, move |view, window, cx| {
                    if view.mode == FocusMode::Action && view.active_id == expected {
                        handle.focus(window, cx);
                    }
                });
            }
        }

        let settings = Settings::global(cx);
        let threshold = settings.focus_timing_threshold();
        let horizon = settings.focus_horizon();
        let temporal =
            TemporalSnapshot::new(&self.events, &self.signals, self.now, threshold, horizon);
        self.card_states.retain(
            self.items.iter().map(subroutine_core::AnyItem::id).chain(
                self.event_moments
                    .iter()
                    .map(super::temporal::EventMoment::notice_id),
            ),
        );
        self.card_states.animate(window, cx);
        self.notice_states.animate(window, cx);
        let mode_switch = self.render_mode_switch(window, cx);
        let (event_notice, signal_notice, notice_height) =
            self.render_notices(temporal, window, cx);
        let has_notices = event_notice.is_some() || signal_notice.is_some();
        let notices = has_notices.then(|| {
            div()
                .w_full()
                .min_w_0()
                .flex_none()
                .px_4()
                .pb(NOTICE_GAP)
                .when_else(
                    self.notices_stacked(),
                    |notices| notices.column().items_center(),
                    |notices| notices.row().items_start(),
                )
                .justify_center()
                .gap(NOTICE_GAP)
                .when_some(event_notice, |this, notice| this.child(notice))
                .when_some(signal_notice, |this, notice| this.child(notice))
        });
        let notice_space = if has_notices {
            notice_height + NOTICE_GAP
        } else {
            gpui::px(0.)
        };
        let content = self.render_carousel(window, cx);
        let content_height = self
            .viewport_size
            .height
            .max(notice_space + MIN_CAROUSEL_ROOM);
        let scrolls = content_height > self.viewport_size.height;
        let content = div()
            .w_full()
            .when_else(
                scrolls,
                |content| content.h(content_height),
                |content| content.h_full(),
            )
            .column()
            .children(notices)
            .child(div().w_full().flex_1().min_h_0().child(content));
        let content = if scrolls {
            ScrollArea::new("focus-content-scroll")
                .vertical()
                .label("Focus content")
                .child(content)
                .into_any_element()
        } else {
            content.into_any_element()
        };
        let settings_open = self.settings_open;
        let toolbar = div()
            .id("focus-toolbar")
            .row()
            .flex_none()
            .gap_2()
            .px_4()
            .pt_2()
            .pb_4()
            .child(
                IconButton::new("focus-view-settings", KitIcon::Settings, "Focus settings")
                    .small()
                    .selected(settings_open)
                    .track_focus(&self.settings_button_focus)
                    .on_click({
                        let view = cx.entity().downgrade();
                        move |window, cx| {
                            view.update(cx, |view, cx| view.toggle_settings(window, cx))
                                .ok();
                        }
                    }),
            )
            .child(mode_switch);
        let view = cx.entity();
        let content = div()
            .w_full()
            .flex_1()
            .min_h_0()
            .on_prepaint(move |bounds, _, cx| {
                view.update(cx, |view, cx| {
                    if view.viewport_size != bounds.size {
                        view.viewport_size = bounds.size;
                        cx.notify();
                    }
                });
            })
            .child(content);
        let content_plane = div()
            .absolute()
            .top(TOP_EDGE_INSET)
            .right_0()
            .bottom_0()
            .left_0()
            .min_h_0()
            .column()
            .child(content)
            .child(toolbar);
        let settings_panel = settings_open.then(|| self.render_settings(window, cx));

        self.tab_root(self.drop_zone(cx), cx)
            .relative()
            .size_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    crate::components::menu::open_context_menu(
                        focus_context_menu(
                            view.mode == FocusMode::Action && !view.items.is_empty(),
                            Settings::global(cx).focus_carousel_orientation,
                        ),
                        event.position,
                        window,
                        cx,
                    );
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.is_held || ItemManager::global(cx).read(cx).is_editing() {
                    return;
                }
                let key = event.keystroke.key.as_str();
                let extend_selection = event.keystroke.modifiers.shift
                    && matches!(key, "left" | "right" | "up" | "down");
                match key {
                    "left" | "up" | "k" => {
                        cx.stop_propagation();
                        view.move_active(-1, extend_selection, window, cx);
                    }
                    "right" | "down" | "j" => {
                        cx.stop_propagation();
                        view.move_active(1, extend_selection, window, cx);
                    }
                    "space" | "enter" if view.mode == FocusMode::Action => {
                        let card_focused = view
                            .active_id
                            .and_then(|id| view.item_focus_handles.get(&id))
                            .is_some_and(|handle| handle.is_focused(window));
                        if view.focus_handle.is_focused(window) || card_focused {
                            cx.stop_propagation();
                            view.complete_active(window, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .child(content_plane)
            .when(settings_open, |this| {
                this.child(
                    div()
                        .id("focus-settings-dismiss")
                        .absolute()
                        .inset_0()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_settings(window, cx);
                        })),
                )
                .when_some(settings_panel, |this, panel| {
                    this.child(
                        div()
                            .absolute()
                            .bottom_4()
                            .left_4()
                            .shadow_lg()
                            .child(panel),
                    )
                })
            })
    }
}
