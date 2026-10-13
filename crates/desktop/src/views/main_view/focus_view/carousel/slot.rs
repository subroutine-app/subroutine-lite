use chrono::Utc;
use gpui::{
    AnyElement, Context, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::{foundation::StyledExt as _, overlay::Tooltip};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_core::{Action, AnyItem, Event};

use crate::{
    AppIcon,
    components::{
        ItemCard,
        transition::{self, WindowTransitionExt as _},
    },
    icons::Icon,
    item_manager::{DraftView, ItemManager},
    selection::SelectionManager,
    settings::FocusCarouselOrientation,
    stores::{AppDatabaseStore, WorkspaceContent},
};

use super::{FocusMode, FocusView, carousel_object, geometry::CarouselGeometry};

pub(super) enum CarouselSlot {
    Draft(AnyItem),
    Create(FocusMode),
    Status(&'static str, AppIcon, &'static str),
}

impl CarouselSlot {
    pub(super) fn resolve(
        draft: Option<AnyItem>,
        content: WorkspaceContent,
        mode: FocusMode,
        blocked: bool,
        has_active: bool,
    ) -> Option<Self> {
        if let Some(draft) = draft {
            return Some(Self::Draft(draft));
        }
        if has_active {
            return None;
        }
        Some(match content {
            WorkspaceContent::Loading => Self::Status("focus-loading", AppIcon::Clock, "Loading…"),
            WorkspaceContent::Unavailable => Self::Status(
                "focus-unavailable",
                AppIcon::ClockAlert,
                "Focus unavailable",
            ),
            WorkspaceContent::Ready if mode == FocusMode::Action && blocked => Self::Status(
                "focus-action-blocked",
                AppIcon::CalendarClock,
                "Queue paused for an event",
            ),
            WorkspaceContent::Ready => Self::Create(mode),
        })
    }
}

impl FocusView {
    fn begin_focus_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if AppDatabaseStore::global(cx).read(cx).workspace_content() != WorkspaceContent::Ready
            || self.has_draft(cx)
            || match self.mode {
                FocusMode::Action => self.queue_blocker.is_some() || !self.items.is_empty(),
                FocusMode::Event => !self.event_moments.is_empty(),
            }
        {
            return;
        }
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
            return;
        }
        let item = match self.mode {
            FocusMode::Action => AnyItem::Action(Action::new("").with_queued(true)),
            FocusMode::Event => {
                AnyItem::Event(Event::new("", Utc::now(), chrono::Duration::hours(1)))
            }
        };
        SelectionManager::clear_global(cx);
        self.reset_wheel();
        self.carousel_overscroll.reset();
        self.restore_focus = false;
        self.focus_handle.focus(window, cx);
        manager.update(cx, |manager, cx| {
            manager.begin_view_draft(&item, DraftView::Focus, None, window, cx);
        });
        self.inline_card_id = manager
            .read(cx)
            .view_draft(DraftView::Focus)
            .map(AnyItem::id);
        cx.notify();
    }

    pub(super) fn render_carousel_slot(
        &mut self,
        slot: CarouselSlot,
        geometry: CarouselGeometry,
        footer_height: f32,
        orientation: FocusCarouselOrientation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (object_id, card): (ElementId, AnyElement) = match slot {
            CarouselSlot::Draft(item) => {
                let id = item.id();
                let element_id: ElementId = match self.mode {
                    FocusMode::Action => ("focus-carousel-card", id.as_u64_pair().1).into(),
                    FocusMode::Event => format!("focus-event-card.{id}").into(),
                };
                // This card replaces an already visible slot, not an entering carousel item.
                window
                    .keyed_transition(
                        (element_id.clone(), "draft-entrance"),
                        cx,
                        transition::QUICK,
                        || 1.0,
                    )
                    .snap(1.0, cx);
                let card = ItemCard::new_with_id(
                    element_id,
                    &item,
                    super::super::super::format_item_meta(&item),
                    window,
                    cx,
                )
                .details(self.card_states.get(id))
                .actionable(false)
                .editable(true)
                .draggable(false, None)
                .when_else(
                    geometry.width < 360.0,
                    |card| card.large_title(true),
                    |card| card.display_title(),
                )
                .size_full();
                (
                    ("focus-carousel-object", id.as_u64_pair().1).into(),
                    card.render(window, cx).into_any_element(),
                )
            }
            CarouselSlot::Create(mode) => {
                let (id, label) = match mode {
                    FocusMode::Action => ("focus-add-action", "Add an action"),
                    FocusMode::Event => ("focus-schedule-event", "Schedule an event"),
                };
                let card = ItemCard::create_slot(id, label, cx)
                    .size_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.begin_focus_draft(window, cx);
                    }));
                ("focus-carousel-slot".into(), card.into_any_element())
            }
            CarouselSlot::Status(id, icon, label) => {
                let card = div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id(id)
                            .size_10()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(cx.theme().colors.text_muted)
                            .aria_label(label)
                            .tooltip(move |_, cx| Tooltip::new(id, label).view(cx))
                            .semantic_in(cx, NodeSpec::new(id, Role::Status).text(label))
                            .child(Icon::new(icon).size_5()),
                    );
                ("focus-carousel-slot".into(), card.into_any_element())
            }
        };
        let card = div()
            .column()
            .size_full()
            .child(div().w_full().flex_1().min_h_0().child(card))
            .child(div().w_full().h(px(footer_height)).flex_none())
            .into_any_element();
        carousel_object(object_id, geometry, orientation, 1.0, card).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_changes_cannot_replace_an_open_draft() {
        let draft = AnyItem::Action(Action::new("").with_queued(true));
        for content in [
            WorkspaceContent::Ready,
            WorkspaceContent::Loading,
            WorkspaceContent::Unavailable,
        ] {
            let slot =
                CarouselSlot::resolve(Some(draft.clone()), content, FocusMode::Action, true, true);
            assert!(matches!(slot, Some(CarouselSlot::Draft(item)) if item.id() == draft.id()));
        }
    }

    #[test]
    fn creation_requires_ready_empty_unblocked_content() {
        for content in [WorkspaceContent::Loading, WorkspaceContent::Unavailable] {
            assert!(matches!(
                CarouselSlot::resolve(None, content, FocusMode::Action, false, false),
                Some(CarouselSlot::Status(..))
            ));
        }
        assert!(matches!(
            CarouselSlot::resolve(
                None,
                WorkspaceContent::Ready,
                FocusMode::Action,
                true,
                false
            ),
            Some(CarouselSlot::Status(..))
        ));
        assert!(matches!(
            CarouselSlot::resolve(
                None,
                WorkspaceContent::Ready,
                FocusMode::Action,
                false,
                false
            ),
            Some(CarouselSlot::Create(FocusMode::Action))
        ));
        assert!(matches!(
            CarouselSlot::resolve(None, WorkspaceContent::Ready, FocusMode::Event, true, false),
            Some(CarouselSlot::Create(FocusMode::Event))
        ));
        assert!(
            CarouselSlot::resolve(
                None,
                WorkspaceContent::Ready,
                FocusMode::Action,
                false,
                true
            )
            .is_none()
        );
    }
}
