use std::{cell::Cell, collections::HashSet, rc::Rc};

use gpui::{App, Context, Window};
use gpui_kit::{
    display::badge::Tone,
    overlay::{DialogEvent, toast},
};
use subroutine_core::AnyItem;

use crate::{
    components::{DraggedItems, timed_toast},
    stores::{AppDatabaseStore, DataChanged},
};

use super::{RootView, library_drop::LibraryDropItem};

const BULK_DROP_THRESHOLD: usize = 5;

pub(super) fn resolve_dragged(dragged: &DraggedItems, cx: &App) -> DraggedItems {
    let store = AppDatabaseStore::global(cx);
    let store = store.read(cx);
    let mut resolved = dragged.clone();
    let mut seen = HashSet::new();
    resolved.items = dragged
        .items
        .iter()
        .filter(|item| store.is_ready() && seen.insert(item.id()))
        .filter_map(|item| {
            let current = store.get_item(item.id());
            let template_exists = match item {
                AnyItem::Action(action) => action
                    .template_id
                    .is_some_and(|id| store.get_action_template(id).is_some()),
                AnyItem::Event(event) => event
                    .template_id
                    .is_some_and(|id| store.get_event_template(id).is_some()),
                _ => false,
            };
            LibraryDropItem::resolve(
                item,
                dragged.saved_item_ids.as_deref(),
                dragged.materialized_item_ids.contains(&item.id()),
                current,
                template_exists,
            )
            .map(|resolved| resolved.item)
        })
        .collect();
    resolved
}

pub(super) fn scheduled_item_count(items: &[AnyItem]) -> usize {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert(item.id()))
        .map(|item| match item {
            AnyItem::Routine(routine) => routine.steps.len(),
            AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => 0,
            AnyItem::Action(_) | AnyItem::Event(_) | AnyItem::Marker(_) | AnyItem::Signal(_) => 1,
        })
        .sum()
}

pub(super) fn confirm_drop<T: 'static>(
    count: usize,
    verb: &'static str,
    detail: String,
    window: &mut Window,
    cx: &mut Context<T>,
    apply: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let store = AppDatabaseStore::global(cx);
    if count == 0 || !store.read(cx).is_ready() {
        return;
    }
    if count < BULK_DROP_THRESHOLD {
        apply(window, cx);
        return;
    }

    let generation = store.read(cx).workspace_generation();
    let changed = Rc::new(Cell::new(false));
    let changed_on_event = changed.clone();
    let subscription = cx.subscribe(&store, move |_, _, _: &DataChanged, _| {
        changed_on_event.set(true);
    });
    let label = format!("{verb} {count} items");
    window.defer(cx, move |window, cx| {
        let Some(root) = window.root::<RootView>().flatten() else {
            return;
        };
        root.update(cx, |root, cx| {
            root.open_drop_confirmation(label, detail, window, cx, move |answer, window, cx| {
                let current = store.read(cx);
                let unchanged = !changed.get()
                    && current.is_ready()
                    && current.workspace_generation() == generation;
                drop(subscription);
                if answer == DialogEvent::Confirmed && !unchanged {
                    toast::push(
                        window,
                        cx,
                        timed_toast(
                            "bulk-drop.changed",
                            "The data changed while confirming. Nothing was applied; drag the items again.",
                        )
                        .tone(Tone::Warning),
                    );
                }
                apply_confirmed(answer, unchanged, || apply(window, cx));
            });
        });
    });
}

fn apply_confirmed(answer: DialogEvent, unchanged: bool, apply: impl FnOnce()) {
    if answer == DialogEvent::Confirmed && unchanged {
        apply();
    }
}
