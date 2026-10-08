use rusqlite::{Connection, DatabaseName, TransactionBehavior};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

use super::recovery::path_with_suffix;
use crate::{LocalStoreError, Result};

pub(crate) const APPLICATION_ID: i32 = 0x5355_4252; // "SUBR"
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const MIGRATION_1: &str = include_str!("schema.sql");
const MIGRATIONS: &[&str] = &[MIGRATION_1];
pub(crate) const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreDiagnostics {
    pub application_id: i32,
    pub schema_version: i32,
    pub journal_mode: String,
    pub foreign_keys: bool,
    pub synchronous_full: bool,
    pub busy_timeout_ms: u64,
}

pub(super) fn configure_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(())
}

pub(super) fn verify_application_id(connection: &Connection) -> Result<bool> {
    let application_id: i32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id == APPLICATION_ID {
        return Ok(false);
    }
    if application_id != 0 {
        return Err(LocalStoreError::WrongApplicationId {
            actual: application_id,
            expected: APPLICATION_ID,
        });
    }

    let user_table_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    let schema_version: i32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if user_table_count == 0 && schema_version == 0 {
        Ok(true)
    } else {
        Err(LocalStoreError::UntaggedNonEmptyDatabase)
    }
}

fn backup_before_migration(
    connection: &Connection,
    path: &Path,
    from_version: i32,
    to_version: i32,
) -> Result<PathBuf> {
    let backup = path_with_suffix(
        path,
        &format!(
            ".backup-v{from_version}-to-v{to_version}-{}.sqlite3",
            Uuid::now_v7()
        ),
    );
    connection.backup(DatabaseName::Main, &backup, None)?;
    Ok(backup)
}

pub(super) fn migrate(
    connection: &mut Connection,
    path: &Path,
    claim_application_id: bool,
) -> Result<()> {
    debug_assert_eq!(SCHEMA_VERSION, MIGRATIONS.len() as i32);
    migrate_with(connection, path, claim_application_id, MIGRATIONS).map(|_| ())
}

pub(crate) fn migrate_with(
    connection: &mut Connection,
    path: &Path,
    claim_application_id: bool,
    migrations: &[&str],
) -> Result<Option<PathBuf>> {
    let supported = i32::try_from(migrations.len())
        .map_err(|_| LocalStoreError::InvalidMutation("too many SQLite migrations".into()))?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > supported {
        return Err(LocalStoreError::UnsupportedSchema {
            actual: version,
            supported,
        });
    }
    if version == supported {
        return Ok(None);
    }

    let backup = (version > 0)
        .then(|| backup_before_migration(connection, path, version, supported))
        .transpose()?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for migration in migrations.iter().skip(version as usize) {
        tx.execute_batch(migration)?;
    }
    if claim_application_id {
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    }
    tx.pragma_update(None, "user_version", supported)?;
    tx.commit()?;
    Ok(backup)
}

pub(super) fn diagnostics(connection: &Connection) -> Result<StoreDiagnostics> {
    let application_id = connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let schema_version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    let busy_timeout_ms: u64 =
        connection.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    Ok(StoreDiagnostics {
        application_id,
        schema_version,
        journal_mode,
        foreign_keys: foreign_keys != 0,
        synchronous_full: synchronous == 2,
        busy_timeout_ms,
    })
}
