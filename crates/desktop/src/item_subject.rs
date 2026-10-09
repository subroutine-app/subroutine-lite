use subroutine_core::{
    ActionTemplate, AnyItem, CoreItem, EventTemplate, ItemType, Marker, Recurrence, ResourceValue,
    Routine, SchedulePoint, checked_duration_end, checked_duration_sum,
};
use uuid::Uuid;

pub(crate) fn validate_item_timing(item: &AnyItem) -> Result<(), &'static str> {
    match item {
        AnyItem::Routine(routine) => validate_routine_timing(routine),
        AnyItem::Marker(marker) => validate_marker_timing(marker),
        _ => validate_duration(item),
    }
}

pub(crate) fn validate_saved_timing(item: &SavedItem) -> Result<(), &'static str> {
    match item {
        SavedItem::Action(template) => validate_duration(template),
        SavedItem::Event(template) => validate_duration(template),
    }
}

pub(crate) fn validate_resource_timing(resource: &ResourceValue) -> Result<(), &'static str> {
    match resource {
        ResourceValue::Action(item) => validate_duration(item),
        ResourceValue::Event(item) => validate_duration(item),
        ResourceValue::Routine(item) => validate_routine_timing(item),
        ResourceValue::Marker(item) => validate_marker_timing(item),
        ResourceValue::Signal(item) => validate_duration(item),
        ResourceValue::ActionTemplate(item) => validate_duration(item),
        ResourceValue::EventTemplate(item) => validate_duration(item),
        ResourceValue::MarkerTemplate(item) => validate_duration(item),
        ResourceValue::SignalTemplate(item) => validate_duration(item),
    }
}

fn validate_duration(item: &impl CoreItem) -> Result<(), &'static str> {
    if let Some(duration) = item.duration() {
        checked_duration_end(item.start().unwrap_or_else(SchedulePoint::now), duration)?;
    }
    Ok(())
}

fn validate_marker_timing(marker: &Marker) -> Result<(), &'static str> {
    if marker.end_date.is_some_and(|end| end < marker.date) {
        return Err("The end date must not be before the start date");
    }
    Ok(())
}

fn validate_routine_timing(routine: &Routine) -> Result<(), &'static str> {
    let default = subroutine_core::ops::Settings::default().default_step_duration;
    let durations = || {
        routine
            .steps
            .iter()
            .map(|step| step.duration.unwrap_or(default))
    };
    let start = routine.target.unwrap_or_else(SchedulePoint::now);
    checked_duration_end(start, checked_duration_sum(durations())?)?;
    durations().try_fold(start, checked_duration_end)?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) enum ItemSubject {
    Live(AnyItem),
    Saved(SavedItem),
}

#[derive(Clone, Debug)]
pub(crate) enum SavedItem {
    Action(ActionTemplate),
    Event(EventTemplate),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemSubjectKey {
    Live(Uuid),
    Saved(Uuid),
}

impl ItemSubject {
    pub(crate) fn key(&self) -> ItemSubjectKey {
        match self {
            Self::Live(item) => ItemSubjectKey::Live(item.id()),
            Self::Saved(item) => ItemSubjectKey::Saved(item.id()),
        }
    }

    pub(crate) fn id(&self) -> Uuid {
        match self {
            Self::Live(item) => item.id(),
            Self::Saved(item) => item.id(),
        }
    }

    pub(crate) fn item_type(&self) -> ItemType {
        match self {
            Self::Live(item) => item.item_type(),
            Self::Saved(item) => item.item_type(),
        }
    }

    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Live(item) => item.title(),
            Self::Saved(item) => item.title(),
        }
    }

    pub(crate) fn content(&self) -> Option<String> {
        match self {
            Self::Live(item) => item.content(),
            Self::Saved(item) => item.content(),
        }
    }

    pub(crate) fn is_saved(&self) -> bool {
        match self {
            Self::Live(item) => item.is_template(),
            Self::Saved(_) => true,
        }
    }

    pub(crate) fn as_live(&self) -> Option<&AnyItem> {
        match self {
            Self::Live(item) => Some(item),
            Self::Saved(_) => None,
        }
    }
}

impl From<AnyItem> for ItemSubject {
    fn from(item: AnyItem) -> Self {
        Self::Live(item)
    }
}

impl From<SavedItem> for ItemSubject {
    fn from(item: SavedItem) -> Self {
        Self::Saved(item)
    }
}

impl SavedItem {
    pub(crate) fn id(&self) -> Uuid {
        match self {
            Self::Action(template) => template.id,
            Self::Event(template) => template.id,
        }
    }

    pub(crate) fn item_type(&self) -> ItemType {
        match self {
            Self::Action(_) => ItemType::ActionTemplate,
            Self::Event(_) => ItemType::EventTemplate,
        }
    }

    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Action(template) => &template.title,
            Self::Event(template) => &template.title,
        }
    }

    pub(crate) fn content(&self) -> Option<String> {
        match self {
            Self::Action(template) => template.content.clone(),
            Self::Event(template) => template.content.clone(),
        }
    }

    pub(crate) fn recurrence(&self) -> Option<Recurrence> {
        match self {
            Self::Action(template) => template.recurrence,
            Self::Event(template) => template.recurrence,
        }
    }
}

impl From<ActionTemplate> for SavedItem {
    fn from(template: ActionTemplate) -> Self {
        Self::Action(template)
    }
}

impl From<EventTemplate> for SavedItem {
    fn from(template: EventTemplate) -> Self {
        Self::Event(template)
    }
}

impl From<SavedItem> for AnyItem {
    fn from(item: SavedItem) -> Self {
        match item {
            SavedItem::Action(template) => AnyItem::ActionTemplate(template),
            SavedItem::Event(template) => AnyItem::EventTemplate(template),
        }
    }
}

impl TryFrom<AnyItem> for SavedItem {
    type Error = AnyItem;

    fn try_from(item: AnyItem) -> Result<Self, Self::Error> {
        match item {
            AnyItem::ActionTemplate(template) => Ok(SavedItem::Action(template)),
            AnyItem::EventTemplate(template) => Ok(SavedItem::Event(template)),
            item => Err(item),
        }
    }
}
