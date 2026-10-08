
use std::collections::HashSet;

use subroutine_core::{AnyItem, Routine, RoutineStep, ops::actions::backlog};
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct LibraryDropItem {
    pub(super) item: AnyItem,
    pub(super) from_saved_items: bool,
}

impl LibraryDropItem {
    pub(super) fn resolve(
        dragged: &AnyItem,
        saved_ids: Option<&[Uuid]>,
        materialized: bool,
        current: Option<AnyItem>,
        template_exists: bool,
    ) -> Option<Self> {
        if let Some(item) = current {
            return Some(Self {
                item,
                from_saved_items: false,
            });
        }

        if !materialized {
            return None;
        }

        let template_id = match dragged {
            AnyItem::Action(action) => action.template_id,
            AnyItem::Event(event) => event.template_id,
            _ => None,
        }?;
        (template_exists && saved_ids?.contains(&template_id)).then(|| Self {
            item: dragged.clone(),
            from_saved_items: true,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibraryDropTarget {
    Unqueued,
    SavedItems,
    Routines,
}

#[derive(Debug)]
pub(super) struct LibraryDropPlan {
    pub(super) items: Vec<AnyItem>,
    pub(super) accepted: usize,
}

impl LibraryDropTarget {
    pub(super) fn from_id(id: &str) -> Option<Self> {
        match id {
            "unqueued" => Some(Self::Unqueued),
            "saved-items" => Some(Self::SavedItems),
            "routines" => Some(Self::Routines),
            _ => None,
        }
    }

    pub(super) fn accepts(self, item: &AnyItem, from_saved_items: bool) -> bool {
        match (self, item) {
            (Self::Unqueued, AnyItem::Action(action)) => {
                !action.is_completed()
                    && (action.queued
                        || action.pinned
                        || action.start.is_some()
                        || from_saved_items)
            }
            (Self::SavedItems, AnyItem::Action(action)) => {
                !from_saved_items && action.template_id.is_none()
            }
            (Self::SavedItems, AnyItem::Event(event)) => {
                !from_saved_items && event.template_id.is_none()
            }
            (Self::Routines, AnyItem::Action(_)) => true,
            _ => false,
        }
    }

    pub(super) fn plan(self, items: &[LibraryDropItem]) -> Option<LibraryDropPlan> {
        let mut seen = HashSet::new();
        let supported: Vec<&AnyItem> = items
            .iter()
            .filter(|candidate| {
                self.accepts(&candidate.item, candidate.from_saved_items)
                    && seen.insert(candidate.item.id())
            })
            .map(|candidate| &candidate.item)
            .collect();
        let accepted = supported.len();
        if accepted == 0 {
            return None;
        }

        let items = match self {
            Self::Unqueued => supported
                .into_iter()
                .filter_map(|item| match item {
                    AnyItem::Action(action) => Some(AnyItem::Action(backlog(action.clone()).value)),
                    _ => None,
                })
                .collect(),
            Self::SavedItems => supported
                .into_iter()
                .filter_map(|item| match item {
                    AnyItem::Action(action) => Some(AnyItem::ActionTemplate(action.as_template())),
                    AnyItem::Event(event) => Some(AnyItem::EventTemplate(event.as_template())),
                    _ => None,
                })
                .collect(),
            Self::Routines => {
                let mut ordered: Vec<_> = supported
                    .into_iter()
                    .filter_map(|item| match item {
                        AnyItem::Action(action) => Some(action),
                        _ => None,
                    })
                    .collect();
                ordered.sort_by_key(|action| action.start.map(|start| start.timestamp()));
                let first = ordered.first()?;
                let steps = ordered
                    .iter()
                    .map(|action| {
                        let step = RoutineStep::new(action.title.clone());
                        match action.duration {
                            Some(duration) => step.with_duration(duration),
                            None => step,
                        }
                    })
                    .collect();
                vec![AnyItem::Routine(
                    Routine::new(first.title.clone()).with_steps(steps),
                )]
            }
        };
        Some(LibraryDropPlan { items, accepted })
    }

    pub(super) fn drop_hint(self) -> &'static str {
        match self {
            Self::Unqueued => "Drop incomplete actions to move them to Unqueued",
            Self::SavedItems => "Drop actions or events to save them for reuse",
            Self::Routines => "Drop actions to create one routine",
        }
    }

    pub(super) fn success_message(self, accepted: usize, ignored: usize) -> String {
        let plural = if accepted == 1 { "" } else { "s" };
        let message = match self {
            Self::Unqueued => format!("Moved {accepted} action{plural} to Unqueued"),
            Self::SavedItems => format!("Saved {accepted} item{plural} for reuse"),
            Self::Routines => format!("Created routine from {accepted} action{plural}"),
        };
        if ignored == 0 {
            message
        } else {
            let plural = if ignored == 1 { "" } else { "s" };
            format!("{message} ({ignored} item{plural} ignored)")
        }
    }
}
