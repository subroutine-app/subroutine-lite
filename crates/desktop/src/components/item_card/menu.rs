use gpui::{App, Entity};
use subroutine_core::{Action, AnyItem, ItemType};
use uuid::Uuid;

use crate::{
    components::menu::MenuBuilder,
    item_manager::ItemManager,
    selection::{
        CompleteSelected, DeleteSelected, DuplicateSelected, SelectionManager,
        TogglePinnedSelected, ToggleQueuedSelected, bulk, selection_context_menu,
    },
    stores::AppDatabaseStore,
    views::{
        OpenItemInspector, OpenSavedItemInspector, ScheduledItemDestination, ViewScheduledItem,
        saved_items_context_menu,
    },
};

use super::availability::{
    event_availability_menu, event_availability_menu_for_target,
    projected_event_availability_target,
};

pub(super) const RESTORE_ACTION_LABEL: &str = "Restore";

pub(super) fn restore_action_label(action: &Action) -> Option<&'static str> {
    action.is_completed().then_some(RESTORE_ACTION_LABEL)
}

fn open_in_item_inspector(
    menu: MenuBuilder,
    label: &'static str,
    item_id: Uuid,
    selection_scope: Option<crate::selection::SelectionScope>,
) -> MenuBuilder {
    menu.item(label, move |window, cx| {
        window.dispatch_action(Box::new(OpenItemInspector(item_id, selection_scope)), cx);
    })
}

fn inspector_action_label(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Action => "Edit action",
        ItemType::Event => "Edit event",
        ItemType::Routine => "Edit routine",
        ItemType::Marker => "Edit marker",
        ItemType::Signal => "Edit signal",
        ItemType::ActionTemplate => "Edit saved action",
        ItemType::EventTemplate => "Edit saved event",
    }
}

fn action_context_menu(
    menu: MenuBuilder,
    action: &Action,
    selection_scope: Option<crate::selection::SelectionScope>,
    cx: &App,
) -> MenuBuilder {
    let action_id = action.id;
    let completion_action = action.clone();
    let templates = AppDatabaseStore::global(cx).read(cx).action_templates();
    let saved_template_id = templates
        .iter()
        .find(|template| action.template_id == Some(template.id))
        .map(|template| template.id);
    let has_template = saved_template_id.is_some();
    let pinned = action.pinned;
    let completed = action.is_completed();

    open_in_item_inspector(
        menu,
        inspector_action_label(ItemType::Action),
        action_id,
        selection_scope,
    )
    .separator()
    .when(!completed, |menu| {
        menu.item_with_keybinding("Complete", CompleteSelected, move |window, cx| {
            ItemManager::global(cx).update(cx, |manager, cx| {
                if !manager.is_completing(action_id) {
                    manager.begin_complete_action(completion_action.clone(), None, window, cx);
                }
            });
        })
    })
    .when_some(restore_action_label(action), |menu, label| {
        menu.item(label, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.uncomplete_action(action_id, cx);
            });
        })
    })
    .when(!completed && !action.queued, |menu| {
        menu.item_with_keybinding("Queue", ToggleQueuedSelected, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.auto_queue_action(action_id, cx);
            });
        })
    })
    .when(!completed && action.queued, |menu| {
        menu.item_with_keybinding("Unqueue", ToggleQueuedSelected, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.backlog_action(action_id, cx);
            });
        })
    })
    .when(!completed && action.start.is_some(), |menu| {
        let action = action.clone();
        menu.item_with_keybinding(
            if pinned {
                "Unpin from this time"
            } else {
                "Pin to this time"
            },
            TogglePinnedSelected,
            move |_, cx| {
                bulk::set_pinned(&[AnyItem::Action(action.clone())], !pinned, cx);
            },
        )
    })
    .when(!has_template, |menu| {
        menu.item("Save for reuse", move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.save_action(action_id, cx);
            });
        })
    })
    .when_some(saved_template_id, |menu, template_id| {
        menu.item("Show saved item", move |window, cx| {
            window.dispatch_action(Box::new(OpenSavedItemInspector(template_id)), cx);
        })
    })
    .separator()
    .when(action.duration.is_some(), |menu| {
        menu.item("Remove duration", move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.clear_action_duration(action_id, cx);
            });
        })
    })
    .item_with_keybinding("Duplicate action", DuplicateSelected, {
        let action = action.clone();
        move |_, cx| {
            bulk::duplicate(&[AnyItem::Action(action.clone())], cx);
        }
    })
    .item_with_keybinding("Delete action", DeleteSelected, {
        let action = action.clone();
        move |window, cx| bulk::delete(&[AnyItem::Action(action.clone())], window, cx)
    })
}

fn simple_item_menu(
    menu: MenuBuilder,
    delete_label: &'static str,
    item: AnyItem,
    selection_scope: Option<crate::selection::SelectionScope>,
) -> MenuBuilder {
    let item_id = item.id();
    let edit_label = inspector_action_label(item.item_type());
    open_in_item_inspector(menu, edit_label, item_id, selection_scope)
        .separator()
        .item_with_keybinding(delete_label, DeleteSelected, move |window, cx| {
            bulk::delete(std::slice::from_ref(&item), window, cx)
        })
}

fn scheduled_navigation_menu(menu: MenuBuilder, item: &AnyItem) -> MenuBuilder {
    let Some(start) = item.start() else {
        return menu;
    };
    let item_id = item.id();

    menu.label("View")
        .item("View in timeline", move |window, cx| {
            window.dispatch_action(
                Box::new(ViewScheduledItem(
                    ScheduledItemDestination::Timeline,
                    item_id,
                    start,
                )),
                cx,
            );
        })
        .item("View in queue", move |window, cx| {
            window.dispatch_action(
                Box::new(ViewScheduledItem(
                    ScheduledItemDestination::Queue,
                    item_id,
                    start,
                )),
                cx,
            );
        })
        .item("View in calendar", move |window, cx| {
            window.dispatch_action(
                Box::new(ViewScheduledItem(
                    ScheduledItemDestination::Calendar,
                    item_id,
                    start,
                )),
                cx,
            );
        })
        .separator()
}

fn item_or_selection_context_menu(
    item: &AnyItem,
    selection_scope: Option<crate::selection::SelectionScope>,
    schedule_navigation: bool,
    cx: &App,
) -> MenuBuilder {
    let selected = selection_scope.and_then(|scope| {
        SelectionManager::global(cx)
            .read(cx)
            .context_menu_targets_selection(scope, item.id())
            .then(|| SelectionManager::selected_items(cx))
    });
    match selected {
        Some(items) if items.len() > 1 && items.iter().all(AnyItem::is_template) => {
            saved_items_context_menu(items)
        }
        Some(items) if items.len() > 1 => selection_context_menu(items),
        _ => item_context_menu(item, selection_scope, schedule_navigation, cx),
    }
}

pub(super) fn card_context_menu(
    item: &AnyItem,
    selection_scope: Option<crate::selection::SelectionScope>,
    schedule_navigation: bool,
    availability_store: Option<&Entity<AppDatabaseStore>>,
    cx: &App,
) -> MenuBuilder {
    if let Some(store) = availability_store {
        let menu = MenuBuilder::new();
        return match projected_event_availability_target(item, store.read(cx).events()) {
            Some(target) => event_availability_menu_for_target(menu, target, store.clone()),
            None => menu,
        };
    }
    item_or_selection_context_menu(item, selection_scope, schedule_navigation, cx)
}

pub(crate) fn item_context_menu(
    item: &AnyItem,
    selection_scope: Option<crate::selection::SelectionScope>,
    schedule_navigation: bool,
    cx: &App,
) -> MenuBuilder {
    let menu = MenuBuilder::new().when(schedule_navigation && item.start().is_some(), |menu| {
        scheduled_navigation_menu(menu, item)
    });

    match item {
        AnyItem::Action(action) => action_context_menu(menu, action, selection_scope, cx),
        AnyItem::Event(event) => {
            let event_id = event.id;
            let convert = event.clone();
            let delete = event.clone();
            let saved_template_id = AppDatabaseStore::global(cx)
                .read(cx)
                .event_templates()
                .iter()
                .find(|template| event.template_id == Some(template.id))
                .map(|template| template.id);
            let menu = open_in_item_inspector(
                menu,
                inspector_action_label(ItemType::Event),
                event_id,
                selection_scope,
            )
            .when_some(saved_template_id, |menu, template_id| {
                menu.item("Show saved item", move |window, cx| {
                    window.dispatch_action(Box::new(OpenSavedItemInspector(template_id)), cx);
                })
            });
            event_availability_menu(menu, event, cx)
                .item("Convert to date marker", move |_, cx| {
                    AppDatabaseStore::global(cx).update(cx, |store, cx| {
                        store.convert_event_to_marker(convert.clone(), cx);
                    });
                })
                .separator()
                .item_with_keybinding("Delete event", DeleteSelected, move |window, cx| {
                    bulk::delete(&[AnyItem::Event(delete.clone())], window, cx)
                })
        }
        AnyItem::Routine(routine) => simple_item_menu(
            menu,
            "Delete routine",
            AnyItem::Routine(routine.clone()),
            selection_scope,
        ),
        AnyItem::Marker(marker) => simple_item_menu(
            menu,
            "Delete marker",
            AnyItem::Marker(marker.clone()),
            selection_scope,
        ),
        AnyItem::Signal(signal) => simple_item_menu(
            menu,
            "Delete signal",
            AnyItem::Signal(signal.clone()),
            selection_scope,
        ),
        AnyItem::ActionTemplate(template) => {
            saved_items_context_menu(vec![AnyItem::ActionTemplate(template.clone())])
        }
        AnyItem::EventTemplate(template) => {
            saved_items_context_menu(vec![AnyItem::EventTemplate(template.clone())])
        }
    }
}
