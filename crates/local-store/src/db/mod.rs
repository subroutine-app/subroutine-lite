mod identity;

mod local;
pub(crate) mod outbox;
pub(crate) mod projection;
pub(crate) mod recovery;
mod rows;
pub(crate) mod schema;
pub(crate) mod sync;

use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
use uuid::Uuid;

use crate::{
    LocalStoreError, Projection, Result, StoreDiagnostics, WorkspaceIdentity, WorkspaceSpec,
};
use identity::initialize_identity;
use projection::load_projection;
use schema::{configure_connection, diagnostics, migrate, verify_application_id};

pub(crate) struct Database {
    connection: Connection,
    identity: WorkspaceIdentity,
}

impl Database {
    pub(crate) fn open(path: &Path, workspace: WorkspaceSpec) -> Result<Self> {
        recovery::check(path)?;
        let mut connection = Connection::open(path)?;
        let claim_application_id = verify_application_id(&connection)?;
        configure_connection(&connection)?;
        migrate(&mut connection, path, claim_application_id)?;
        let identity = initialize_identity(&mut connection, workspace)?;
        Ok(Self {
            connection,
            identity,
        })
    }

    pub(crate) fn identity(&self) -> &WorkspaceIdentity {
        &self.identity
    }

    pub(crate) fn diagnostics(&self) -> Result<StoreDiagnostics> {
        diagnostics(&self.connection)
    }

    pub(crate) fn projection(&self) -> Result<Projection> {
        load_projection(&self.connection)
    }

    pub(crate) fn purge_remote(&mut self) -> Result<()> {
        if !matches!(self.identity, WorkspaceIdentity::Remote { .. }) {
            return Err(LocalStoreError::InvalidMutation(
                "only a remote account workspace may remove offline account data".into(),
            ));
        }
        self.connection.pragma_update(None, "secure_delete", "ON")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM outbox", [])?;
        tx.execute("DELETE FROM integration_entries", [])?;
        tx.execute("DELETE FROM canonical_resources", [])?;
        tx.execute(
            "UPDATE sync_state
             SET dataset_id = NULL, canonical_seq = 0, authoritative_snapshot = 0
             WHERE singleton = 1",
            [],
        )?;
        tx.commit()?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
}

fn parse_uuid(value: &str, field: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|error| {
        LocalStoreError::WorkspaceMismatch(format!("invalid {field} in database: {error}"))
    })
}

fn parse_sql_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn non_nil_uuid(value: Uuid) -> Option<Uuid> {
    (!value.is_nil()).then_some(value)
}
