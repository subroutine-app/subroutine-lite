use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum LocalStoreError {
    #[error("could not access local data: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not read or write local data: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid server origin: {0}")]
    InvalidServerOrigin(String),
    #[error("database belongs to a different workspace: {0}")]
    WorkspaceMismatch(String),
    #[error("a valid account is required to open this workspace")]
    InvalidAccountIdentity,
    #[error("this file is not recognized as a Subroutine database; it was left unchanged")]
    UntaggedNonEmptyDatabase,
    #[error("this file belongs to another application; it was left unchanged")]
    WrongApplicationId { actual: i32, expected: i32 },
    #[error("this database requires a newer version of Subroutine")]
    UnsupportedSchema { actual: i32, supported: i32 },
    #[error("SQLite integrity check failed: {0}")]
    Integrity(String),
    #[error(
        "corrupt local data was quarantined at {}; recovery is required before this workspace can be opened: {reason}",
        directory.display()
    )]
    Quarantined { directory: PathBuf, reason: String },
    #[error("workspace data changed during this request; try again")]
    StaleCanonicalState,
    #[error("download account data before syncing changes")]
    NoAuthoritativeSnapshot,
    #[error("received older server data; the workspace was not changed")]
    SequenceRegression { current: i64, incoming: i64 },
    #[error("server data does not match this workspace")]
    DatasetMismatch { current: Uuid, incoming: Uuid },
    #[error("cannot replace workspace data while {pending} local change(s) are unsynchronized")]
    DatasetChangedWithPendingIntent {
        current: Option<Uuid>,
        incoming: Option<Uuid>,
        pending: usize,
    },
    #[error("could not apply this change: {0}")]
    InvalidMutation(String),
    #[error("pending change {0} was not found")]
    OutboxMutationNotFound(Uuid),
    #[error("local data is unavailable; reopen the workspace")]
    ActorUnavailable,
    #[error("this workspace is no longer active")]
    Inactive,
}

pub type Result<T> = std::result::Result<T, LocalStoreError>;
