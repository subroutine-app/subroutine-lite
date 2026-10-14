use std::{fs, path::PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::Connection;
use subroutine_core::{
    Action, AllData, ClientMutation, Event, MUTATION_PROTOCOL_VERSION, Marker, MutationOperation,
    MutationRequest, OptimisticPatch, ResourceKey,
};
use subroutine_import::provider::SyncOutcome;
use uuid::Uuid;

use crate::{LocalStore, LocalStoreError, WorkspaceIdentity, WorkspaceSpec};

const PROVIDER: &str = "test-calendar";

struct Workspace {
    root: PathBuf,
    spec: WorkspaceSpec,
}

impl Workspace {
    fn new(remote: bool) -> Self {
        let root = std::env::temp_dir().join(format!("provider-import-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let spec = if remote {
            WorkspaceSpec::Remote {
                server_origin: "https://example.test".into(),
                account_id: Uuid::new_v4(),
            }
        } else {
            WorkspaceSpec::Local
        };
        Self { root, spec }
    }

    fn open(&self) -> LocalStore {
        LocalStore::open(self.root.join("workspace.sqlite"), self.spec.clone()).unwrap()
    }

    fn connection(&self) -> Connection {
        Connection::open(self.root.join("workspace.sqlite")).unwrap()
    }

    fn ready(&self) -> LocalStore {
        let store = self.open();
        if matches!(self.spec, WorkspaceSpec::Remote { .. }) {
            snapshot(&store, AllData::default());
        }
        store
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn snapshot(store: &LocalStore, data: AllData) {
    store
        .install_snapshot(
            AllData {
                dataset_id: Uuid::new_v4(),
                seq: 7,
                ..data
            },
            store.state().unwrap(),
        )
        .unwrap();
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
}

fn time() -> DateTime<Utc> {
    date().and_hms_opt(9, 0, 0).unwrap().and_utc()
}

fn event() -> Event {
    let mut event = Event::new("Meeting", time(), Duration::hours(1));
    event.source_busy = Some(false);
    event
}

fn event_outcome(version: &str) -> SyncOutcome {
    let mut outcome = SyncOutcome::new(PROVIDER);
    outcome.push_event("meeting".into(), Some(version.into()), event());
    outcome
}

fn sample(version: Option<&str>) -> SyncOutcome {
    let mut outcome = SyncOutcome::new(PROVIDER);
    outcome.push_action(
        "task".into(),
        version.map(str::to_owned),
        Action::new("Task"),
    );
    outcome.push_event("meeting".into(), version.map(str::to_owned), event());
    outcome.push_marker(
        "day".into(),
        version.map(str::to_owned),
        Marker::new("Day", date()),
    );
    outcome
}

fn persist(store: &LocalStore, operation: MutationOperation, optimistic_patch: OptimisticPatch) {
    if matches!(store.identity(), WorkspaceIdentity::Local { .. }) {
        store.apply_local_patch(optimistic_patch).unwrap();
    } else {
        let state = store.state().unwrap();
        store
            .enqueue(ClientMutation {
                request: MutationRequest {
                    protocol_version: MUTATION_PROTOCOL_VERSION,
                    dataset_id: state.dataset_id.unwrap(),
                    client_id: store.identity().client_id(),
                    mutation_id: Uuid::now_v7(),
                    base_seq: state.canonical_seq,
                    operation,
                },
                optimistic_patch,
            })
            .unwrap();
    }
}

#[test]
fn overlapping_batches_are_idempotent_and_conflicting_duplicates_roll_back() {
    for remote in [false, true] {
        let workspace = Workspace::new(remote);
        let store = workspace.ready();
        let batch = || {
            let mut outcome = sample(Some("v1"));
            outcome.items.extend(sample(Some("v1")).items);
            outcome
        };
        let first = store.apply_provider_import(batch()).unwrap();
        assert_eq!(first.changed, 3);
        let repeated = store.apply_provider_import(batch()).unwrap();
        assert_eq!(repeated.changed, 0);
        assert_eq!(
            repeated.projection.pending_count,
            first.projection.pending_count
        );
        assert_eq!(
            serde_json::to_value(&repeated.projection.data).unwrap(),
            serde_json::to_value(&first.projection.data).unwrap()
        );
        assert_eq!(count(&workspace.connection(), "integration_entries"), 3);

        let mut conflict = event_outcome("v1");
        conflict.push_action(
            "new".into(),
            None,
            Action::new("Must not be partially saved"),
        );
        conflict.push_event(
            "meeting".into(),
            Some("v1".into()),
            Event::new("Conflicting title", time(), Duration::hours(1)),
        );
        assert!(matches!(
            store.apply_provider_import(conflict),
            Err(LocalStoreError::InvalidMutation(_))
        ));
        let after = store.load_projection().unwrap();
        assert_eq!(after.pending_count, first.projection.pending_count);
        assert_eq!(
            serde_json::to_value(after.data).unwrap(),
            serde_json::to_value(first.projection.data).unwrap()
        );
        assert_eq!(count(&workspace.connection(), "integration_entries"), 3);
    }
}

fn count(connection: &Connection, table: &str) -> usize {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn conversions_and_exclusions_can_return_but_local_deletions_cannot() {
    for remote in [false, true] {
        let workspace = Workspace::new(remote);
        let store = workspace.ready();
        let first = store.apply_provider_import(event_outcome("v1")).unwrap();
        let id = first.projection.data.events[0].id;
        let unseen = store
            .apply_provider_import(SyncOutcome::new(PROVIDER))
            .unwrap();
        assert_eq!(unseen.changed, 0);
        assert_eq!(unseen.projection.data.events.len(), 1);

        let mut all_day = SyncOutcome::new(PROVIDER);
        all_day.push_marker(
            "meeting".into(),
            Some("v1".into()),
            Marker::new("Meeting", date()),
        );
        let converted = store.apply_provider_import(all_day).unwrap();
        assert_eq!(converted.changed, 1);
        assert!(converted.projection.data.events.is_empty());
        assert_eq!(converted.projection.data.markers[0].id, id);
        let timed = store.apply_provider_import(event_outcome("v1")).unwrap();
        assert_eq!(timed.changed, 1);
        assert!(timed.projection.data.markers.is_empty());
        assert_eq!(timed.projection.data.events[0].id, id);

        for expected in [1, 0] {
            let mut excluded = SyncOutcome::new(PROVIDER);
            excluded.exclude("meeting");
            let result = store.apply_provider_import(excluded).unwrap();
            assert_eq!(result.changed, expected);
            assert!(result.projection.data.events.is_empty());
        }
        drop(store);
        let store = workspace.open();
        let returned = store.apply_provider_import(event_outcome("v1")).unwrap();
        assert_eq!(returned.changed, 1);
        assert_eq!(returned.projection.data.events[0].id, id);

        let deletes = vec![ResourceKey::Event { id }];
        persist(
            &store,
            MutationOperation::DeleteResources {
                resources: deletes.clone(),
            },
            OptimisticPatch {
                deletes,
                ..Default::default()
            },
        );
        let deleted = store.apply_provider_import(event_outcome("v2")).unwrap();
        assert_eq!(deleted.changed, 0);
        assert!(deleted.projection.data.events.is_empty());
        let mut excluded = SyncOutcome::new(PROVIDER);
        excluded.exclude("meeting");
        store.apply_provider_import(excluded).unwrap();
        drop(store);
        let store = workspace.open();
        let still_deleted = store.apply_provider_import(event_outcome("v3")).unwrap();
        assert_eq!(still_deleted.changed, 0);
        assert!(still_deleted.projection.data.events.is_empty());
        assert!(
            workspace
                .connection()
                .query_row(
                    "SELECT ignored FROM integration_entries WHERE external_id = 'meeting'",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
    }
}

#[test]
fn readiness_and_transaction_failure_leave_no_partial_identity_or_intent() {
    for remote in [false, true] {
        let workspace = Workspace::new(remote);
        let store = workspace.open();
        if remote {
            assert!(matches!(
                store.apply_provider_import(sample(Some("v1"))),
                Err(LocalStoreError::NoAuthoritativeSnapshot)
            ));
            assert_eq!(count(&workspace.connection(), "integration_entries"), 0);
            assert_eq!(count(&workspace.connection(), "outbox"), 0);
            snapshot(&store, AllData::default());
        }
        let connection = workspace.connection();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_identity BEFORE INSERT ON integration_entries
             WHEN NEW.external_id = 'meeting'
             BEGIN SELECT RAISE(ABORT, 'injected identity write failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.apply_provider_import(sample(Some("v1"))),
            Err(LocalStoreError::Sqlite(_))
        ));
        assert_eq!(count(&connection, "integration_entries"), 0);
        assert_eq!(count(&connection, "outbox"), 0);
        assert_eq!(count(&connection, "canonical_resources"), 0);
        connection
            .execute_batch("DROP TRIGGER reject_identity")
            .unwrap();
        let retried = store.apply_provider_import(sample(Some("v1"))).unwrap();
        assert_eq!(retried.changed, 3);
        assert_eq!(
            store
                .apply_provider_import(sample(Some("v1")))
                .unwrap()
                .changed,
            0
        );
    }
}
