use gpui::{
    AnyElement, App, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use gpui_kit::overlay::Tooltip;
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, Event};

use crate::{AppIcon, components::menu::MenuBuilder, icons::Icon, stores::AppDatabaseStore};

pub(super) fn event_availability_indicator(
    item: &AnyItem,
    card_id: &ElementId,
    cx: &App,
) -> Option<AnyElement> {
    let AnyItem::Event(event) = item else {
        return None;
    };
    let explanation = event_availability_explanation(event)?;
    Some(
        div()
            .id((card_id.clone(), "event-availability"))
            .flex()
            .items_center()
            .text_color(cx.theme().colors.text_muted)
            .child(Icon::new(AppIcon::ListChecks).size_3p5())
            .tooltip(move |_, cx| Tooltip::new("event-allows-actions", explanation).view(cx))
            .semantic_in(
                cx,
                NodeSpec::new(format!("event-availability.{card_id:?}"), Role::Status)
                    .text("Allows actions")
                    .description(explanation),
            )
            .into_any_element(),
    )
}

fn event_availability_explanation(event: &Event) -> Option<&'static str> {
    if event.blocks_time() {
        None
    } else if event.busy_override == Some(false) {
        Some("You allowed actions during this event.")
    } else {
        Some("This event allows actions to be scheduled during it.")
    }
}

fn event_availability_target<'a>(event: &Event, stored: &'a [Event]) -> Option<&'a Event> {
    let current = stored.iter().find(|current| current.id == event.id);
    if let Some(current) = current
        && (current.source_provider.is_some()
            || current.recurrence.is_none()
            || current.id == current.lineage_id)
    {
        return Some(current);
    }
    let source = current.unwrap_or(event);
    if source.source_provider.is_some() || source.recurrence.is_none() {
        return None;
    }
    stored.iter().find(|root| {
        root.id == source.lineage_id
            && root.id == root.lineage_id
            && root.source_provider.is_none()
            && root.recurrence.is_some()
    })
}

pub(super) fn projected_event_availability_target<'a>(
    item: &AnyItem,
    stored: &'a [Event],
) -> Option<&'a Event> {
    let AnyItem::Event(event) = item else {
        return None;
    };
    if event.source_provider.is_some() || event.recurrence.is_none() {
        return None;
    }
    event_availability_target(event, stored).filter(|target| {
        target.id == event.lineage_id
            && target.id == target.lineage_id
            && target.source_provider.is_none()
            && target.recurrence.is_some()
    })
}

pub(super) fn event_availability_menu(menu: MenuBuilder, event: &Event, cx: &App) -> MenuBuilder {
    let store = AppDatabaseStore::global(cx);
    let Some(current) = event_availability_target(event, store.read(cx).events()) else {
        return menu;
    };
    event_availability_menu_for_target(menu, current, store.clone())
}

pub(super) fn event_availability_menu_for_target(
    menu: MenuBuilder,
    current: &Event,
    store: Entity<AppDatabaseStore>,
) -> MenuBuilder {
    let id = current.id;
    let series = current.source_provider.is_none() && current.recurrence.is_some();
    let busy = current.blocks_time();
    let label = match (series, busy) {
        (true, true) => "Allow actions during this series",
        (true, false) => "Reserve time for this series",
        (false, true) => "Allow actions during this event",
        (false, false) => "Reserve this time",
    };
    let toggle_store = store.clone();
    menu.item(label, move |_, cx| {
        toggle_store.update(cx, |store, cx| {
            store.set_event_busy_override(id, Some(!busy), cx);
        });
    })
    .when(current.busy_override.is_some(), |menu| {
        menu.item(
            if series {
                "Use default availability for this series"
            } else {
                "Use default availability"
            },
            move |_, cx| {
                store.update(cx, |store, cx| {
                    store.set_event_busy_override(id, None, cx);
                });
            },
        )
    })
}
