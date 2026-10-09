use uuid::Uuid;

use crate::{
    Action, ActionTemplate, Event, EventTemplate, Marker, MarkerTemplate, Routine, Signal,
    SignalTemplate,
};
use chrono::NaiveDate;

use super::{Delete, OpError, OpResult, Outcome, Write, change::put};

pub trait Identify {
    fn id(&self) -> Uuid;
    fn ensure_id(&mut self);
}

macro_rules! identify_with_lineage {
    ($($ty:ty => $lineage:ident;)*) => {
        $(impl Identify for $ty {
            fn id(&self) -> Uuid {
                self.id
            }

            fn ensure_id(&mut self) {
                if self.id.is_nil() {
                    self.id = Uuid::now_v7();
                }
                if self.$lineage.is_nil() {
                    self.$lineage = self.id;
                }
            }
        })*
    };
}

macro_rules! identify_by_id {
    ($($ty:ty;)*) => {
        $(impl Identify for $ty {
            fn id(&self) -> Uuid {
                self.id
            }

            fn ensure_id(&mut self) {
                if self.id.is_nil() {
                    self.id = Uuid::now_v7();
                }
            }
        })*
    };
}

identify_with_lineage! {
    Action => recurrence_id;
    Event => lineage_id;
    EventTemplate => lineage_id;
    Marker => lineage_id;
    MarkerTemplate => lineage_id;
    Signal => lineage_id;
    SignalTemplate => lineage_id;
    Routine => recurrence_id;
}

identify_by_id! {
    ActionTemplate;
}

pub trait Templated {
    type Template: Clone + Into<Write>;

    fn template(&self) -> Self::Template;
}

macro_rules! templated {
    ($($ty:ty => $template:ty;)*) => {
        $(impl Templated for $ty {
            type Template = $template;

            fn template(&self) -> $template {
                self.as_template()
            }
        })*
    };
}

templated! {
    Action => ActionTemplate;
    Event => EventTemplate;
    Marker => MarkerTemplate;
    Signal => SignalTemplate;
}

pub fn create<T>(mut item: T) -> Outcome<T>
where
    T: Identify + Clone + Into<Write>,
{
    item.ensure_id();
    put(item)
}

pub fn validate_update<T: Identify>(kind: &'static str, path_id: Uuid, item: &T) -> OpResult<()> {
    let body_id = item.id();
    if path_id.is_nil() {
        return Err(OpError::rejected(format!("{kind} path id must not be nil")));
    }
    if body_id.is_nil() {
        return Err(OpError::rejected(format!("{kind} body id must not be nil")));
    }
    if path_id != body_id {
        return Err(OpError::rejected(format!(
            "{kind} path id {path_id} does not match body id {body_id}"
        )));
    }
    Ok(())
}

pub fn save_as_template<T: Templated>(item: &T) -> Outcome<T::Template> {
    put(item.template())
}

pub fn convert_event_to_marker(
    event: &Event,
    date: NaiveDate,
    local_end_date: Option<NaiveDate>,
) -> OpResult<Outcome<Marker>> {
    convert_event_to_marker_with_id(event, date, local_end_date, Uuid::now_v7())
}

pub fn convert_event_to_marker_with_id(
    event: &Event,
    date: NaiveDate,
    local_end_date: Option<NaiveDate>,
    marker_id: Uuid,
) -> OpResult<Outcome<Marker>> {
    let marker = match local_end_date {
        Some(end_date) => event.to_marker_between_with_id(date, end_date, marker_id),
        None => event.to_marker_on_with_id(date, marker_id),
    }
    .map_err(OpError::rejected)?;
    let mut changes = super::Changes::default();
    changes.put(marker.clone()).delete(Delete::Event(event.id));
    Ok(Outcome::new(marker, changes))
}
