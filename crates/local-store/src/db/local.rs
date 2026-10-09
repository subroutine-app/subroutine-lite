use rusqlite::TransactionBehavior;
use subroutine_core::{Event, OptimisticPatch, ResourceKey, ResourceValue};
use uuid::Uuid;

use super::{
    Database,
    projection::{commit_projection, patch::preserve_missing_availability},
    rows::{delete_rows, load_rows, rewrite_routine_ordinals, upsert_rows},
};
use crate::{LocalStoreError, Projection, Result, WorkspaceIdentity};

impl Database {
    pub(crate) fn set_event_busy_override(
        &mut self,
        event_id: Uuid,
        busy_override: Option<bool>,
    ) -> Result<Projection> {
        if !matches!(self.identity, WorkspaceIdentity::Local { .. }) {
            return Err(LocalStoreError::InvalidMutation(
                "direct override edits require a local-only workspace".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let events: Vec<Event> = load_rows(&tx, "event")?;
        let Some(mut event) = events.into_iter().find(|event| event.id == event_id) else {
            return Err(LocalStoreError::InvalidMutation(format!(
                "event {event_id} not found"
            )));
        };
        event.busy_override = busy_override;
        upsert_rows(&tx, "event", &[event], |row| row.id)?;
        commit_projection(tx)
    }

    pub(crate) fn apply_local_patch(&mut self, mut patch: OptimisticPatch) -> Result<Projection> {
        if !matches!(self.identity, WorkspaceIdentity::Local { .. }) {
            return Err(LocalStoreError::InvalidMutation(
                "direct local patches are restricted to local-only workspaces".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        preserve_missing_availability(
            &mut patch,
            &load_rows(&tx, "event")?,
            &load_rows(&tx, "event_template")?,
        );
        let OptimisticPatch {
            writes,
            deletes,
            routine_order,
        } = patch;
        for value in writes {
            match value {
                ResourceValue::Action(action) => {
                    upsert_rows(&tx, "action", std::slice::from_ref(&action), |row| row.id)?;
                }
                ResourceValue::Event(event) => {
                    upsert_rows(&tx, "event", std::slice::from_ref(&event), |row| row.id)?;
                }
                ResourceValue::Routine(routine) => {
                    upsert_rows(&tx, "routine", std::slice::from_ref(&routine), |row| row.id)?;
                }
                ResourceValue::Marker(marker) => {
                    upsert_rows(&tx, "marker", std::slice::from_ref(&marker), |row| row.id)?;
                }
                ResourceValue::Signal(signal) => {
                    upsert_rows(&tx, "signal", std::slice::from_ref(&signal), |row| row.id)?;
                }
                ResourceValue::ActionTemplate(template) => {
                    upsert_rows(
                        &tx,
                        "action_template",
                        std::slice::from_ref(&template),
                        |row| row.id,
                    )?;
                }
                ResourceValue::EventTemplate(template) => {
                    upsert_rows(
                        &tx,
                        "event_template",
                        std::slice::from_ref(&template),
                        |row| row.id,
                    )?;
                }
                ResourceValue::MarkerTemplate(template) => {
                    upsert_rows(
                        &tx,
                        "marker_template",
                        std::slice::from_ref(&template),
                        |row| row.id,
                    )?;
                }
                ResourceValue::SignalTemplate(template) => {
                    upsert_rows(
                        &tx,
                        "signal_template",
                        std::slice::from_ref(&template),
                        |row| row.id,
                    )?;
                }
            }
        }
        for key in deletes {
            match key {
                ResourceKey::Action { id } => delete_rows(&tx, "action", &[id])?,
                ResourceKey::Event { id } => delete_rows(&tx, "event", &[id])?,
                ResourceKey::Routine { id } => delete_rows(&tx, "routine", &[id])?,
                ResourceKey::Marker { id } => delete_rows(&tx, "marker", &[id])?,
                ResourceKey::Signal { id } => delete_rows(&tx, "signal", &[id])?,
                ResourceKey::ActionTemplate { id } => delete_rows(&tx, "action_template", &[id])?,
                ResourceKey::EventTemplate { id } => delete_rows(&tx, "event_template", &[id])?,
                ResourceKey::MarkerTemplate { id } => delete_rows(&tx, "marker_template", &[id])?,
                ResourceKey::SignalTemplate { id } => delete_rows(&tx, "signal_template", &[id])?,
            }
        }
        if let Some(routine_order) = routine_order {
            rewrite_routine_ordinals(&tx, &routine_order)?;
        }
        commit_projection(tx)
    }
}
