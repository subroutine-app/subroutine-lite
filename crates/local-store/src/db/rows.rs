use rusqlite::{Connection, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use subroutine_core::{AllData, DataDelta};
use uuid::Uuid;

use super::{parse_sql_uuid, projection::patch::validate_routine_order};
use crate::{LocalStoreError, Result};

const RESOURCE_VALUE_VERSION: i64 = 1;

pub(super) fn insert_snapshot_rows(tx: &Transaction<'_>, data: &AllData) -> Result<()> {
    insert_rows(tx, "action", &data.actions, |row| row.id)?;
    insert_rows(tx, "event", &data.events, |row| row.id)?;
    insert_rows(tx, "routine", &data.routines, |row| row.id)?;
    insert_rows(tx, "marker", &data.markers, |row| row.id)?;
    insert_rows(tx, "signal", &data.signals, |row| row.id)?;
    insert_rows(tx, "action_template", &data.action_templates, |row| row.id)?;
    insert_rows(tx, "event_template", &data.event_templates, |row| row.id)?;
    insert_rows(tx, "marker_template", &data.marker_templates, |row| row.id)?;
    insert_rows(tx, "signal_template", &data.signal_templates, |row| row.id)?;
    Ok(())
}

fn insert_rows<T: Serialize>(
    tx: &Transaction<'_>,
    kind: &str,
    rows: &[T],
    id: impl Fn(&T) -> Uuid,
) -> Result<()> {
    for (ordinal, row) in rows.iter().enumerate() {
        tx.execute(
            "INSERT INTO canonical_resources (
                kind, resource_id, value_version, value_json, ordinal
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                kind,
                id(row).to_string(),
                RESOURCE_VALUE_VERSION,
                serde_json::to_vec(row)?,
                ordinal as i64
            ],
        )?;
    }
    Ok(())
}

pub(super) fn apply_delta_rows(tx: &Transaction<'_>, delta: &DataDelta) -> Result<()> {
    upsert_rows(tx, "action", &delta.actions, |row| row.id)?;
    upsert_rows(tx, "event", &delta.events, |row| row.id)?;
    upsert_rows(tx, "routine", &delta.routines, |row| row.id)?;
    upsert_rows(tx, "marker", &delta.markers, |row| row.id)?;
    upsert_rows(tx, "signal", &delta.signals, |row| row.id)?;
    upsert_rows(tx, "action_template", &delta.action_templates, |row| row.id)?;
    upsert_rows(tx, "event_template", &delta.event_templates, |row| row.id)?;
    upsert_rows(tx, "marker_template", &delta.marker_templates, |row| row.id)?;
    upsert_rows(tx, "signal_template", &delta.signal_templates, |row| row.id)?;

    delete_rows(tx, "action", &delta.tombstones.actions)?;
    delete_rows(tx, "event", &delta.tombstones.events)?;
    delete_rows(tx, "routine", &delta.tombstones.routines)?;
    delete_rows(tx, "marker", &delta.tombstones.markers)?;
    delete_rows(tx, "signal", &delta.tombstones.signals)?;
    delete_rows(tx, "action_template", &delta.tombstones.action_templates)?;
    delete_rows(tx, "event_template", &delta.tombstones.event_templates)?;
    delete_rows(tx, "marker_template", &delta.tombstones.marker_templates)?;
    delete_rows(tx, "signal_template", &delta.tombstones.signal_templates)?;

    if !delta.routine_order.is_empty() {
        for (ordinal, id) in delta.routine_order.iter().enumerate() {
            tx.execute(
                "UPDATE canonical_resources SET ordinal = ?1
                 WHERE kind = 'routine' AND resource_id = ?2",
                params![ordinal as i64, id.to_string()],
            )?;
        }
    }
    Ok(())
}

pub(super) fn upsert_rows<T: Serialize>(
    tx: &Transaction<'_>,
    kind: &str,
    rows: &[T],
    id: impl Fn(&T) -> Uuid,
) -> Result<()> {
    for row in rows {
        let resource_id = id(row).to_string();
        tx.execute(
            "INSERT INTO canonical_resources (
                kind, resource_id, value_version, value_json, ordinal
             ) VALUES (
                ?1, ?2, ?3, ?4,
                COALESCE(
                    (SELECT ordinal FROM canonical_resources WHERE kind = ?1 AND resource_id = ?2),
                    (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM canonical_resources WHERE kind = ?1)
                )
             )
             ON CONFLICT(kind, resource_id) DO UPDATE SET
                value_version = excluded.value_version,
                value_json = excluded.value_json",
            params![
                kind,
                resource_id,
                RESOURCE_VALUE_VERSION,
                serde_json::to_vec(row)?
            ],
        )?;
    }
    Ok(())
}

pub(super) fn delete_rows(tx: &Transaction<'_>, kind: &str, ids: &[Uuid]) -> Result<()> {
    for id in ids {
        tx.execute(
            "DELETE FROM canonical_resources WHERE kind = ?1 AND resource_id = ?2",
            params![kind, id.to_string()],
        )?;
    }
    Ok(())
}

pub(super) fn rewrite_routine_ordinals(tx: &Transaction<'_>, routine_order: &[Uuid]) -> Result<()> {
    let mut statement =
        tx.prepare("SELECT resource_id FROM canonical_resources WHERE kind = 'routine'")?;
    let routine_ids = statement
        .query_map([], |row| parse_sql_uuid(row.get(0)?))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    validate_routine_order(routine_ids.iter().copied(), routine_order)?;

    for (ordinal, id) in routine_order.iter().enumerate() {
        tx.execute(
            "UPDATE canonical_resources SET ordinal = ?1
             WHERE kind = 'routine' AND resource_id = ?2",
            params![ordinal as i64, id.to_string()],
        )?;
    }
    Ok(())
}

pub(super) fn load_rows<T: DeserializeOwned>(
    connection: &Connection,
    kind: &str,
) -> Result<Vec<T>> {
    let mut statement = connection.prepare(
        "SELECT value_version, value_json FROM canonical_resources
         WHERE kind = ?1 ORDER BY ordinal, resource_id",
    )?;
    let encoded = statement
        .query_map([kind], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    encoded
        .into_iter()
        .map(|(version, value)| {
            if version != RESOURCE_VALUE_VERSION {
                return Err(LocalStoreError::InvalidMutation(format!(
                    "unsupported {kind} value version {version}"
                )));
            }
            serde_json::from_slice(&value).map_err(Into::into)
        })
        .collect()
}
