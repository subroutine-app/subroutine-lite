use uuid::Uuid;

use crate::{
    Action, ActionTemplate, ChangeEvent, Event, EventTemplate, Marker, MarkerTemplate, Routine,
    Signal, SignalTemplate,
};

#[derive(Debug, Clone)]
pub enum Write {
    Action(Action),
    ActionTemplate(ActionTemplate),
    Event(Event),
    EventTemplate(EventTemplate),
    Marker(Marker),
    MarkerTemplate(MarkerTemplate),
    Routine(Routine),
    Signal(Signal),
    SignalTemplate(SignalTemplate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delete {
    Action(Uuid),
    ActionTemplate(Uuid),
    Event(Uuid),
    EventTemplate(Uuid),
    Marker(Uuid),
    MarkerTemplate(Uuid),
    Routine(Uuid),
    Signal(Uuid),
    SignalTemplate(Uuid),
}

macro_rules! items {
    ($($variant:ident($ty:ty) => $event:ident, $label:literal;)*) => {
        $(
            impl From<$ty> for Write {
                fn from(value: $ty) -> Self {
                    Write::$variant(value)
                }
            }
        )*

        impl Write {
            fn change_event(&self) -> ChangeEvent {
                match self {
                    $(Write::$variant(_) => ChangeEvent::$event,)*
                }
            }
        }

        impl Delete {
            pub fn id(&self) -> Uuid {
                match self {
                    $(Delete::$variant(id) => *id,)*
                }
            }

            pub fn label(&self) -> &'static str {
                match self {
                    $(Delete::$variant(_) => $label,)*
                }
            }

            pub fn change_event(&self) -> ChangeEvent {
                match self {
                    $(Delete::$variant(_) => ChangeEvent::$event,)*
                }
            }
        }
    };
}

items! {
    Action(Action) => ActionsChanged, "action";
    ActionTemplate(ActionTemplate) => ActionTemplatesChanged, "action template";
    Event(Event) => EventsChanged, "event";
    EventTemplate(EventTemplate) => EventTemplatesChanged, "event template";
    Marker(Marker) => MarkersChanged, "marker";
    MarkerTemplate(MarkerTemplate) => MarkerTemplatesChanged, "marker template";
    Routine(Routine) => RoutinesChanged, "routine";
    Signal(Signal) => SignalsChanged, "signal";
    SignalTemplate(SignalTemplate) => SignalTemplatesChanged, "signal template";
}

#[derive(Debug, Default)]
pub struct Changes {
    writes: Vec<Write>,
    deletes: Vec<Delete>,
    rescheduled: bool,
}

impl Changes {
    pub fn put(&mut self, write: impl Into<Write>) -> &mut Self {
        self.writes.push(write.into());
        self
    }

    pub fn put_all<T: Into<Write>>(&mut self, items: impl IntoIterator<Item = T>) -> &mut Self {
        self.writes.extend(items.into_iter().map(Into::into));
        self
    }

    pub fn delete(&mut self, target: Delete) -> &mut Self {
        self.deletes.push(target);
        self
    }

    pub fn rescheduled(mut self) -> Self {
        self.rescheduled = true;
        self
    }

    pub fn writes(&self) -> &[Write] {
        &self.writes
    }

    pub fn deletes(&self) -> &[Delete] {
        &self.deletes
    }

    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.deletes.is_empty()
    }

    pub fn change_events(&self) -> Vec<ChangeEvent> {
        let mut events: Vec<ChangeEvent> = Vec::new();
        for write in &self.writes {
            let event = match (write, self.rescheduled) {
                (Write::Action(_), true) => ChangeEvent::PipelineChanged,
                _ => write.change_event(),
            };
            if !events.contains(&event) {
                events.push(event);
            }
        }
        for delete in &self.deletes {
            let event = delete.change_event();
            if !events.contains(&event) {
                events.push(event);
            }
        }
        events
    }
}

#[derive(Debug)]
pub struct Outcome<T> {
    pub value: T,
    pub changes: Changes,
}

impl<T> Outcome<T> {
    pub fn new(value: T, changes: Changes) -> Self {
        Self { value, changes }
    }
}

pub fn put<T: Clone + Into<Write>>(item: T) -> Outcome<T> {
    let mut changes = Changes::default();
    changes.put(item.clone());
    Outcome::new(item, changes)
}
