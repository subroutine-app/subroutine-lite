use chrono::Local;

use subroutine_core::{
    Action, ActionTemplate, AllData, DataDelta, Event, EventTemplate, Marker, MarkerTemplate,
    Signal, SignalTemplate, Tombstones,
};

use super::{TenantScope, record, routines};
use crate::ops::{Changes, Delete, Settings, Snapshot, Write};

pub(crate) async fn all_data(scope: &TenantScope) -> anyhow::Result<AllData> {
    let mut snapshot = scope.begin_mutation().await?;
    let user_id = snapshot.user_id();
    let (dataset_id, seq) = snapshot.identity().await?;
    let actions = record::fetch_all_in::<Action>(snapshot.connection(), user_id).await?;
    let events = record::fetch_all_in::<Event>(snapshot.connection(), user_id).await?;
    let routines = routines::fetch_all_in(snapshot.connection(), user_id).await?;
    let markers = record::fetch_all_in::<Marker>(snapshot.connection(), user_id).await?;
    let signals = record::fetch_all_in::<Signal>(snapshot.connection(), user_id).await?;
    let action_templates =
        record::fetch_all_in::<ActionTemplate>(snapshot.connection(), user_id).await?;
    let event_templates =
        record::fetch_all_in::<EventTemplate>(snapshot.connection(), user_id).await?;
    let marker_templates =
        record::fetch_all_in::<MarkerTemplate>(snapshot.connection(), user_id).await?;
    let signal_templates =
        record::fetch_all_in::<SignalTemplate>(snapshot.connection(), user_id).await?;
    snapshot.rollback().await?;

    Ok(AllData {
        dataset_id,
        seq,
        actions,
        events,
        routines,
        markers,
        signals,
        action_templates,
        event_templates,
        marker_templates,
        signal_templates,
    })
}

pub(crate) async fn data_delta_in(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
    dataset_id: uuid::Uuid,
    since: i64,
    current_seq: i64,
) -> anyhow::Result<DataDelta> {
    let actions = record::fetch_delta_in::<Action>(conn, user_id, since, current_seq).await?;
    let events = record::fetch_delta_in::<Event>(conn, user_id, since, current_seq).await?;
    let routines = routines::fetch_delta_in(conn, user_id, since, current_seq).await?;
    let routine_order = routines::fetch_order_in(conn, user_id).await?;
    let markers = record::fetch_delta_in::<Marker>(conn, user_id, since, current_seq).await?;
    let signals = record::fetch_delta_in::<Signal>(conn, user_id, since, current_seq).await?;
    let action_templates =
        record::fetch_delta_in::<ActionTemplate>(conn, user_id, since, current_seq).await?;
    let event_templates =
        record::fetch_delta_in::<EventTemplate>(conn, user_id, since, current_seq).await?;
    let marker_templates =
        record::fetch_delta_in::<MarkerTemplate>(conn, user_id, since, current_seq).await?;
    let signal_templates =
        record::fetch_delta_in::<SignalTemplate>(conn, user_id, since, current_seq).await?;

    Ok(DataDelta {
        dataset_id,
        seq: current_seq,
        actions: actions.changed,
        events: events.changed,
        routines: routines.changed,
        routine_order,
        markers: markers.changed,
        signals: signals.changed,
        action_templates: action_templates.changed,
        event_templates: event_templates.changed,
        marker_templates: marker_templates.changed,
        signal_templates: signal_templates.changed,
        tombstones: Tombstones {
            actions: actions.deleted,
            events: events.deleted,
            routines: routines.deleted,
            markers: markers.deleted,
            signals: signals.deleted,
            action_templates: action_templates.deleted,
            event_templates: event_templates.deleted,
            marker_templates: marker_templates.deleted,
            signal_templates: signal_templates.deleted,
        },
    })
}

pub(crate) async fn snapshot_in(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
    settings: Settings,
) -> anyhow::Result<Snapshot> {
    let actions = record::fetch_all_in::<Action>(conn, user_id).await?;
    let events = record::fetch_all_in::<Event>(conn, user_id).await?;
    let markers = record::fetch_all_in::<Marker>(conn, user_id).await?;
    let signals = record::fetch_all_in::<Signal>(conn, user_id).await?;
    let routines = routines::fetch_all_in(conn, user_id).await?;

    Ok(Snapshot::new(
        Local::now(),
        settings,
        actions,
        events,
        routines,
        markers,
        signals,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplyResult {
    Applied,
    Missing(Delete),
}

pub(crate) async fn apply_changes_in(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
    change_seq: i64,
    changes: &Changes,
) -> anyhow::Result<ApplyResult> {
    for write in changes.writes() {
        match write {
            Write::Action(item) => record::upsert(&mut *conn, user_id, change_seq, item).await?,
            Write::ActionTemplate(item) => {
                record::upsert(&mut *conn, user_id, change_seq, item).await?
            }
            Write::Event(item) => record::upsert(&mut *conn, user_id, change_seq, item).await?,
            Write::EventTemplate(item) => {
                record::upsert(&mut *conn, user_id, change_seq, item).await?
            }
            Write::Marker(item) => record::upsert(&mut *conn, user_id, change_seq, item).await?,
            Write::MarkerTemplate(item) => {
                record::upsert(&mut *conn, user_id, change_seq, item).await?
            }
            Write::Signal(item) => record::upsert(&mut *conn, user_id, change_seq, item).await?,
            Write::SignalTemplate(item) => {
                record::upsert(&mut *conn, user_id, change_seq, item).await?
            }
            Write::Routine(item) => {
                routines::upsert_in(&mut *conn, user_id, change_seq, item).await?
            }
        }
    }

    for target in changes.deletes() {
        let deleted = delete_in(&mut *conn, user_id, change_seq, *target).await?;
        if !deleted {
            return Ok(ApplyResult::Missing(*target));
        }
    }

    Ok(ApplyResult::Applied)
}

async fn delete_in(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
    change_seq: i64,
    target: Delete,
) -> anyhow::Result<bool> {
    let id = target.id();
    match target {
        Delete::Action(_) => {
            record::soft_delete::<Action>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::ActionTemplate(_) => {
            record::soft_delete::<ActionTemplate>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::Event(_) => record::soft_delete::<Event>(&mut *conn, user_id, change_seq, id).await,
        Delete::EventTemplate(_) => {
            record::soft_delete::<EventTemplate>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::Marker(_) => {
            record::soft_delete::<Marker>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::MarkerTemplate(_) => {
            record::soft_delete::<MarkerTemplate>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::Routine(_) => routines::soft_delete(&mut *conn, user_id, change_seq, id).await,
        Delete::Signal(_) => {
            record::soft_delete::<Signal>(&mut *conn, user_id, change_seq, id).await
        }
        Delete::SignalTemplate(_) => {
            record::soft_delete::<SignalTemplate>(&mut *conn, user_id, change_seq, id).await
        }
    }
}
