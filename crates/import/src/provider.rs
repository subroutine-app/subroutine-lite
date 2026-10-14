use anyhow::Result;
use subroutine_core::{Action, Event, Marker};

pub enum SyncItem {
    Action(Imported<Action>),
    Event(Imported<Event>),
    Marker(Imported<Marker>),
}

// External identity is stable; versions are opaque change markers, not ordered values.
pub struct Imported<T> {
    pub external_id: String,
    pub external_version: Option<String>,
    pub item: T,
}

pub struct SyncOutcome {
    pub provider_id: &'static str,
    pub items: Vec<SyncItem>,
    // Policy exclusions remove local items but preserve identity mappings;
    // unfetched or unavailable items are not exclusions.
    excluded_external_ids: Vec<String>,
}

impl SyncOutcome {
    pub fn new(provider_id: &'static str) -> Self {
        Self {
            provider_id,
            items: Vec::new(),
            excluded_external_ids: Vec::new(),
        }
    }

    pub fn push_action(
        &mut self,
        external_id: String,
        external_version: Option<String>,
        action: Action,
    ) {
        self.items.push(SyncItem::Action(Imported {
            external_id,
            external_version,
            item: action,
        }));
    }

    pub fn push_event(
        &mut self,
        external_id: String,
        external_version: Option<String>,
        event: Event,
    ) {
        self.items.push(SyncItem::Event(Imported {
            external_id,
            external_version,
            item: event,
        }));
    }

    pub fn push_marker(
        &mut self,
        external_id: String,
        external_version: Option<String>,
        marker: Marker,
    ) {
        self.items.push(SyncItem::Marker(Imported {
            external_id,
            external_version,
            item: marker,
        }));
    }

    pub fn exclude(&mut self, external_id: impl Into<String>) {
        self.excluded_external_ids.push(external_id.into());
    }

    pub fn excluded_external_ids(&self) -> impl Iterator<Item = &str> {
        self.excluded_external_ids.iter().map(String::as_str)
    }

    pub fn actions(&self) -> impl Iterator<Item = &Imported<Action>> {
        self.items.iter().filter_map(|item| match item {
            SyncItem::Action(a) => Some(a),
            _ => None,
        })
    }

    pub fn events(&self) -> impl Iterator<Item = &Imported<Event>> {
        self.items.iter().filter_map(|item| match item {
            SyncItem::Event(e) => Some(e),
            _ => None,
        })
    }

    pub fn markers(&self) -> impl Iterator<Item = &Imported<Marker>> {
        self.items.iter().filter_map(|item| match item {
            SyncItem::Marker(m) => Some(m),
            _ => None,
        })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarSource {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

// Providers may block and be !Send: construct, use, and drop them within one
// synchronous background job, returning only owned results. Do not hold them
// across await points or invoke their private runtimes from an async runtime.
pub trait Provider {
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn ensure_authorized(&mut self) -> Result<()>;

    fn sync(&mut self) -> Result<SyncOutcome>;

    fn list_calendars(&mut self) -> Result<Vec<CalendarSource>> {
        Ok(Vec::new())
    }

    fn push_action(&mut self, _action: &Action) -> Result<()> {
        Ok(())
    }

    fn push_event(&mut self, _event: &Event) -> Result<()> {
        Ok(())
    }

    fn push_marker(&mut self, _marker: &Marker) -> Result<()> {
        Ok(())
    }
}
