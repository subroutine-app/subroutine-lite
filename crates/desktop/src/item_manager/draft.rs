use chrono::{DateTime, Local, TimeZone as _, Utc};
use chronoutil::RelativeDuration;
use gpui::{Context, Window, actions};
use subroutine_core::{Action, AnyItem, Event, ItemType, SchedulePoint};
use uuid::Uuid;

use super::{EditKind, ItemManager, ItemSubject};
use crate::{settings::Settings, stores::AppDatabaseStore};

pub(crate) const DRAFT_KEY_CONTEXT: &str = "ItemDraft";
actions!(item_draft, [DraftAsAction, DraftAsEvent, OpenDraftTypeMenu]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftType {
    Action,
    Event,
}

impl DraftType {
    fn of(item: &AnyItem) -> Option<Self> {
        match item {
            AnyItem::Action(_) => Some(Self::Action),
            AnyItem::Event(_) => Some(Self::Event),
            _ => None,
        }
    }

    pub(crate) fn item_type(self) -> ItemType {
        match self {
            Self::Action => ItemType::Action,
            Self::Event => ItemType::Event,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Action => "Action",
            Self::Event => "Event",
        }
    }
}

fn convert_draft(
    item: &AnyItem,
    target: DraftType,
    fallback_start: DateTime<Utc>,
) -> Option<AnyItem> {
    match (item, target) {
        (AnyItem::Action(action), DraftType::Event) => {
            let start = match action.start {
                Some(SchedulePoint::DateTime(start)) => start,
                Some(SchedulePoint::Date(date)) => {
                    let morning = date.and_hms_opt(9, 0, 0)?;
                    Local
                        .from_local_datetime(&morning)
                        .earliest()
                        .map(|time| time.with_timezone(&Utc))
                        .unwrap_or_else(|| morning.and_utc())
                }
                None => fallback_start,
            };
            let mut event = Event::new(
                action.title.clone(),
                start,
                action.duration.unwrap_or(RelativeDuration::hours(1)),
            )
            .with_content(action.content.clone())
            .with_recurrence(action.recurrence);
            event.id = action.id;
            event.lineage_id = action.id;
            Some(AnyItem::Event(event))
        }
        (AnyItem::Event(event), DraftType::Action) => {
            let mut action = Action::new(event.title.clone())
                .with_queued(true)
                .with_start(Some(SchedulePoint::DateTime(event.start)))
                .with_duration(Some(event.duration))
                .with_content(event.content.clone())
                .with_recurrence(event.recurrence);
            action.id = event.id;
            action.recurrence_id = event.id;
            Some(AnyItem::Action(action))
        }
        _ => None,
    }
}

impl ItemManager {
    pub(crate) fn draft_item(&self, id: Uuid) -> Option<&AnyItem> {
        let editing = self
            .editing_item
            .as_ref()
            .filter(|editing| editing.is_draft())?;
        match &editing.original {
            ItemSubject::Live(item) if item.id() == id => Some(item),
            _ => None,
        }
    }

    pub(crate) fn draft_type(&self, id: Uuid) -> Option<DraftType> {
        if self.editing_item.as_ref()?.kind == EditKind::FixedTypeDraft {
            return None;
        }
        self.draft_item(id).and_then(DraftType::of)
    }

    pub(crate) fn set_draft_type(
        &mut self,
        id: Uuid,
        target: DraftType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.draft_type(id) else {
            return;
        };
        let editing = self.editing_item.as_mut().expect("draft is being edited");
        if current != target {
            let replacement = editing.alternate_draft.take().or_else(|| {
                let ItemSubject::Live(item) = &editing.original else {
                    return None;
                };
                let start = item.start().map(DateTime::<Utc>::from).unwrap_or_else(|| {
                    let settings = Settings::global(cx);
                    let store = AppDatabaseStore::global(cx);
                    store.read(cx).pipeline(&settings).quantize_ceil(Utc::now())
                });
                convert_draft(item, target, start)
            });
            let Some(replacement) = replacement else {
                return;
            };
            if let ItemSubject::Live(previous) =
                std::mem::replace(&mut editing.original, ItemSubject::Live(replacement))
            {
                editing.alternate_draft = Some(previous);
            }
            editing.parsed = editing.parse(&editing.value(cx));
            editing.highlight_revision = editing.highlight_revision.wrapping_add(1);
            cx.notify();
        }
        cx.focus_view(&editing.input, window);
    }
}
