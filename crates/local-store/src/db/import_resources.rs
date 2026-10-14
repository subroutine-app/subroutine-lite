use std::collections::HashMap;

use subroutine_core::{AllData, CoreItem, ResourceValue, SchedulePoint, occurrence_id};
use uuid::Uuid;

pub(super) fn import_resources(data: AllData, namespace: Uuid) -> Vec<ResourceValue> {
    let ids = ImportIds::new(&data, namespace);
    let AllData {
        dataset_id: _,
        seq: _,
        actions,
        events,
        routines,
        markers,
        signals,
        action_templates,
        event_templates,
        marker_templates,
        signal_templates,
    } = data;

    action_templates
        .into_iter()
        .map(ResourceValue::ActionTemplate)
        .chain(
            event_templates
                .into_iter()
                .map(ResourceValue::EventTemplate),
        )
        .chain(
            marker_templates
                .into_iter()
                .map(ResourceValue::MarkerTemplate),
        )
        .chain(
            signal_templates
                .into_iter()
                .map(ResourceValue::SignalTemplate),
        )
        .chain(events.into_iter().map(ResourceValue::Event))
        .chain(markers.into_iter().map(ResourceValue::Marker))
        .chain(signals.into_iter().map(ResourceValue::Signal))
        .chain(actions.into_iter().map(ResourceValue::Action))
        .chain(routines.into_iter().map(ResourceValue::Routine))
        .map(|mut resource| {
            match &mut resource {
                ResourceValue::Action(value) => {
                    value.id = ids.get(value.id);
                    value.recurrence_id = ids.get(value.recurrence_id);
                    value.routine_id = value.routine_id.map(|id| ids.get(id));
                    value.template_id = value.template_id.map(|id| ids.get(id));
                }
                ResourceValue::Event(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                    value.template_id = value.template_id.map(|id| ids.get(id));
                }
                ResourceValue::Marker(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                    value.template_id = value.template_id.map(|id| ids.get(id));
                }
                ResourceValue::Signal(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                    value.template_id = value.template_id.map(|id| ids.get(id));
                }
                ResourceValue::Routine(value) => {
                    value.recurrence_id = ids.get(value.lineage_id());
                    value.id = ids.get(value.id);
                }
                ResourceValue::ActionTemplate(value) => {
                    value.id = ids.get(value.id);
                }
                ResourceValue::EventTemplate(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                }
                ResourceValue::MarkerTemplate(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                }
                ResourceValue::SignalTemplate(value) => {
                    value.id = ids.get(value.id);
                    value.lineage_id = ids.get(value.lineage_id);
                }
            }
            resource
        })
        .collect()
}

struct ImportIds {
    namespace: Uuid,
    occurrences: HashMap<Uuid, Occurrence>,
}

struct Occurrence {
    lineage_id: Uuid,
    kind: &'static str,
    point: SchedulePoint,
}

impl ImportIds {
    fn new(data: &AllData, namespace: Uuid) -> Self {
        let occurrences = data
            .events
            .iter()
            .map(|value| (value.id, value.lineage_id, "event", value.start.into()))
            .chain(
                data.markers
                    .iter()
                    .map(|value| (value.id, value.lineage_id, "marker", value.date.into())),
            )
            .chain(
                data.signals
                    .iter()
                    .map(|value| (value.id, value.lineage_id, "signal", value.datetime.into())),
            )
            .filter(|(id, lineage_id, kind, point)| {
                id != lineage_id && *id == occurrence_id(*lineage_id, kind, *point)
            })
            .map(|(id, lineage_id, kind, point)| {
                (
                    id,
                    Occurrence {
                        lineage_id,
                        kind,
                        point,
                    },
                )
            })
            .collect();
        Self {
            namespace,
            occurrences,
        }
    }

    fn get(&self, id: Uuid) -> Uuid {
        match self.occurrences.get(&id) {
            Some(occurrence) => occurrence_id(
                self.get(occurrence.lineage_id),
                occurrence.kind,
                occurrence.point,
            ),
            None => Uuid::new_v5(&self.namespace, id.as_bytes()),
        }
    }
}
