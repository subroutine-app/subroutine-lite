use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::{Value, json};
use subroutine_core::{
    Action, AllData, AnyItem, OptimisticPatch, ResourceValue, SchedulePoint, ops,
};
use uuid::Uuid;

use crate::{
    args::*,
    create,
    resources::{self, Kind},
};

pub struct Plan {
    pub output: Value,
    pub patch: OptimisticPatch,
}

impl Plan {
    pub fn read(value: impl Serialize) -> Result<Self> {
        Ok(Self {
            output: serde_json::to_value(value)?,
            patch: OptimisticPatch::default(),
        })
    }

    pub fn write(resource: ResourceValue) -> Result<Self> {
        Ok(Self {
            output: Self::resource_value(&resource)?,
            patch: OptimisticPatch {
                writes: vec![resource],
                ..Default::default()
            },
        })
    }

    pub fn outcome<T: Serialize>(outcome: ops::Outcome<T>) -> Result<Self> {
        Ok(Self {
            output: serde_json::to_value(outcome.value)?,
            patch: resources::from_changes(outcome.changes),
        })
    }

    pub fn resource_value(resource: &ResourceValue) -> Result<Value> {
        let mut value = serde_json::to_value(resource)?;
        Ok(value["value"].take())
    }

    pub fn op_error(error: ops::OpError) -> anyhow::Error {
        anyhow::anyhow!("{error}")
    }
}

pub fn execute(
    command: Command,
    data: &AllData,
    now: DateTime<Local>,
    input: Option<Value>,
) -> Result<Plan> {
    let settings = ops::Settings::default();
    let snapshot = || {
        ops::Snapshot::new(
            now,
            settings,
            data.actions.clone(),
            data.events.clone(),
            data.routines.clone(),
            data.markers.clone(),
            data.signals.clone(),
        )
    };
    let plan = match command {
        Command::Item(command) => item(command, data, input, settings),
        Command::Add(command) => create::add(command, snapshot()),
        Command::Action(command) => action(command, data, snapshot()),
        Command::Routine(RoutineCommand::Run { id, start }) => {
            let routine = data
                .routines
                .iter()
                .find(|routine| routine.id == id)
                .with_context(|| format!("routine {id} not found"))?;
            Plan::outcome(
                ops::routines::instantiate(&snapshot(), routine, start).map_err(Plan::op_error)?,
            )
        }
        Command::Pipeline(command) => pipeline(command, snapshot()),
        Command::Data(DataCommand::Export) => Plan::read(data),
        Command::Data(DataCommand::Import { .. }) => {
            import(input.context("missing snapshot input")?)
        }
        Command::Parse(args) => Plan::read(create::parsed_value(args.kind, &args.text, now)?),
    }?;
    for resource in &plan.patch.writes {
        resources::validate(resource, settings).with_context(|| {
            format!(
                "invalid {} {}",
                resources::kind(resource).name(),
                resource.key().id()
            )
        })?;
    }
    Ok(plan)
}

fn item(
    command: ItemCommand,
    data: &AllData,
    input: Option<Value>,
    settings: ops::Settings,
) -> Result<Plan> {
    match command {
        ItemCommand::List(args) => list(args, data),
        ItemCommand::Get { kind, id } => {
            Plan::read(Plan::resource_value(&resources::get(data, kind, id)?)?)
        }
        ItemCommand::Put { kind, .. } => {
            let mut resource = resources::decode(kind, input.context("missing resource input")?)?;
            resources::validate(&resource, settings)?;
            match &mut resource {
                ResourceValue::Action(action) => {
                    let previous = data
                        .actions
                        .iter()
                        .find(|previous| previous.id == action.id);
                    return Plan::outcome(match previous {
                        Some(previous) => ops::actions::update(Some(previous), action.clone()),
                        None => ops::actions::create(action.clone()),
                    });
                }
                ResourceValue::Event(event) => event.preserve_missing_availability(
                    data.events.iter().find(|previous| previous.id == event.id),
                ),
                ResourceValue::EventTemplate(template) => template.preserve_missing_availability(
                    data.event_templates
                        .iter()
                        .find(|previous| previous.id == template.id),
                ),
                _ => {}
            }
            Plan::write(resource)
        }
        ItemCommand::Delete { kind, id } => {
            resources::get(data, kind, id)?;
            let key = kind.key(id);
            Ok(Plan {
                output: json!({ "deleted": key }),
                patch: OptimisticPatch {
                    deletes: vec![key],
                    ..Default::default()
                },
            })
        }
    }
}

fn list(args: ListArgs, data: &AllData) -> Result<Plan> {
    ensure!(
        args.state.is_none() || args.kind.is_none_or(|kind| kind == Kind::Action),
        "--state only applies to actions"
    );
    let kind = args.kind.or_else(|| args.state.map(|_| Kind::Action));
    let search = args.search.map(|search| search.to_lowercase());
    let mut items: Vec<_> = resources::list(data, kind)
        .into_iter()
        .filter(|item| {
            let matches_state = match (args.state, item) {
                (Some(state), ResourceValue::Action(action)) => match state {
                    ActionState::Open => !action.is_completed(),
                    ActionState::Queued => action.queued && !action.is_completed(),
                    ActionState::Backlog => !action.queued && !action.is_completed(),
                    ActionState::Completed => action.is_completed(),
                },
                (None, _) => true,
                _ => false,
            };
            matches_state
                && search
                    .as_ref()
                    .is_none_or(|search| resources::title(item).to_lowercase().contains(search))
        })
        .collect();
    items.sort_by_key(|item| {
        let start = resources::start(item);
        (
            start.is_none(),
            start.map(DateTime::<chrono::Utc>::from),
            item.key().id(),
        )
    });
    if let Some(limit) = args.limit {
        items.truncate(limit);
    }
    if args.ids {
        Plan::read(items.iter().map(|item| item.key().id()).collect::<Vec<_>>())
    } else {
        Plan::read(items)
    }
}

fn action(command: ActionCommand, data: &AllData, snapshot: ops::Snapshot) -> Result<Plan> {
    let find = |id: Uuid| -> Result<Action> {
        data.actions
            .iter()
            .find(|action| action.id == id)
            .cloned()
            .with_context(|| format!("action {id} not found"))
    };
    match command {
        ActionCommand::Complete { id } => {
            Plan::outcome(ops::actions::complete(find(id)?, snapshot.now_utc()))
        }
        ActionCommand::Reopen { id } => Plan::outcome(ops::actions::uncomplete(find(id)?)),
        ActionCommand::Queue { id } => {
            Plan::outcome(ops::actions::queue(&snapshot, id).map_err(Plan::op_error)?)
        }
        ActionCommand::Backlog { id } => Plan::outcome(ops::actions::backlog(find(id)?)),
        ActionCommand::Pin { id, off } => {
            let action = find(id)?;
            ensure!(
                off || (action.queued
                    && !action.is_completed()
                    && matches!(action.start, Some(SchedulePoint::DateTime(_)))),
                "only an open, queued action with a time can be pinned"
            );
            Plan::outcome(ops::actions::set_pinned(action, !off).map_err(Plan::op_error)?)
        }
    }
}

fn pipeline(command: PipelineCommand, mut snapshot: ops::Snapshot) -> Result<Plan> {
    match command {
        PipelineCommand::Show { view } => {
            let context = snapshot.context();
            let items = match view {
                View::Queue => context.queue_items(),
                View::Backlog => context
                    .waiting()
                    .into_iter()
                    .cloned()
                    .map(AnyItem::Action)
                    .collect(),
                View::Timeline => context.timeline_items(),
            };
            Plan::read(items.into_iter().map(resource).collect::<Vec<_>>())
        }
        PipelineCommand::Refresh => {
            Plan::outcome(ops::pipeline::refresh(&snapshot).map_err(Plan::op_error)?)
        }
        PipelineCommand::AutoQueue => {
            Plan::outcome(ops::pipeline::auto_queue(&snapshot).map_err(Plan::op_error)?)
        }
        PipelineCommand::Expedite { within } => {
            snapshot.settings.expedite_horizon = within;
            Plan::outcome(ops::pipeline::expedite(&snapshot).map_err(Plan::op_error)?)
        }
        PipelineCommand::NextSlot { duration } => {
            let start = snapshot
                .context()
                .next_slot(duration)
                .map_err(anyhow::Error::msg)?;
            let end = subroutine_core::checked_duration_end(start.into(), duration)
                .map_err(anyhow::Error::msg)?;
            Plan::read(json!({ "start": start, "end": end }))
        }
    }
}

fn import(value: Value) -> Result<Plan> {
    let fields = value.as_object().context("expected a snapshot object")?;
    let schema = serde_json::to_value(AllData::default())?;
    for key in fields.keys() {
        ensure!(schema.get(key).is_some(), "unknown snapshot field {key}");
    }
    serde_json::from_value::<AllData>(value.clone()).context("invalid snapshot")?;
    let mut writes = Vec::new();
    for &kind in <Kind as clap::ValueEnum>::value_variants() {
        let collection = format!("{}s", kind.name().replace('-', "_"));
        if let Some(items) = value.get(&collection).and_then(Value::as_array) {
            for item in items {
                writes.push(resources::decode(kind, item.clone())?);
            }
        }
    }
    let mut keys = HashSet::new();
    ensure!(
        writes.iter().all(|resource| keys.insert(resource.key())),
        "snapshot contains duplicate resource IDs"
    );
    Ok(Plan {
        output: json!({ "imported": writes.len() }),
        patch: OptimisticPatch {
            writes,
            ..Default::default()
        },
    })
}

fn resource(item: AnyItem) -> ResourceValue {
    match item {
        AnyItem::Action(value) => ResourceValue::Action(value),
        AnyItem::Event(value) => ResourceValue::Event(value),
        AnyItem::Marker(value) => ResourceValue::Marker(value),
        AnyItem::Signal(value) => ResourceValue::Signal(value),
        AnyItem::Routine(value) => ResourceValue::Routine(value),
        AnyItem::ActionTemplate(value) => ResourceValue::ActionTemplate(value),
        AnyItem::EventTemplate(value) => ResourceValue::EventTemplate(value),
    }
}
