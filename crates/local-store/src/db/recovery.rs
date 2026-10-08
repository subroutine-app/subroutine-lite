use rusqlite::{Connection, ErrorCode, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::{LocalStoreError, Result};

const QUARANTINE_MARKER_SUFFIX: &str = ".quarantined.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuarantineRecord {
    directory: PathBuf,
    reason: String,
}

pub(super) fn check(path: &Path) -> Result<()> {
    if let Some(record) = load_quarantine_record(path)? {
        return Err(LocalStoreError::Quarantined {
            directory: record.directory,
            reason: record.reason,
        });
    }

    if path.exists() {
        let check_connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        if let Err(error) = run_integrity_check(&check_connection) {
            if is_corruption_error(&error) {
                let reason = error.to_string();
                drop(check_connection);
                let directory = quarantine_database(path, &reason)?;
                return Err(LocalStoreError::Quarantined { directory, reason });
            }
            return Err(error);
        }
    }
    Ok(())
}

fn run_integrity_check(connection: &Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if result == "ok" {
        Ok(())
    } else {
        Err(LocalStoreError::Integrity(result))
    }
}

fn is_corruption_error(error: &LocalStoreError) -> bool {
    match error {
        LocalStoreError::Integrity(_) => true,
        LocalStoreError::Sqlite(rusqlite::Error::SqliteFailure(error, _)) => matches!(
            error.code,
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
        ),
        _ => false,
    }
}

fn quarantine_marker_path(path: &Path) -> PathBuf {
    path_with_suffix(path, QUARANTINE_MARKER_SUFFIX)
}

pub(crate) fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn load_quarantine_record(path: &Path) -> Result<Option<QuarantineRecord>> {
    let marker = quarantine_marker_path(path);
    match fs::read(marker) {
        Ok(encoded) => serde_json::from_slice(&encoded)
            .map(Some)
            .map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn quarantine_database(path: &Path, reason: &str) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("workspace.sqlite3"));
    let mut quarantine_name = file_name.to_os_string();
    quarantine_name.push(format!(".quarantine-{}", Uuid::now_v7()));
    let directory = parent.join(quarantine_name);
    fs::create_dir(&directory)?;

    let record = QuarantineRecord {
        directory: directory.clone(),
        reason: reason.to_owned(),
    };
    let mut marker = File::create(quarantine_marker_path(path))?;
    marker.write_all(&serde_json::to_vec_pretty(&record)?)?;
    marker.sync_all()?;

    for source in [
        path.to_path_buf(),
        path_with_suffix(path, "-wal"),
        path_with_suffix(path, "-shm"),
    ] {
        if source.exists() {
            let destination = directory.join(
                source
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("workspace.sqlite3")),
            );
            fs::rename(source, destination)?;
        }
    }
    Ok(directory)
}
