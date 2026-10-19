use std::{os::windows::fs::OpenOptionsExt, sync::Mutex};

use super::*;

#[derive(Default)]
struct Legacy {
    bytes: Mutex<Option<Vec<u8>>>,
    fail_delete: Mutex<bool>,
}

impl Credentials for Legacy {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.bytes.lock().unwrap().clone())
    }

    fn save(&self, _: &[u8]) -> Result<(), String> {
        panic!("new sessions must never be written to the size-limited legacy store");
    }

    fn delete(&self) -> Result<(), String> {
        if *self.fail_delete.lock().unwrap() {
            return Err("test deletion failure".into());
        }
        *self.bytes.lock().unwrap() = None;
        Ok(())
    }
}

struct Fixture {
    root: PathBuf,
    store: ProtectedSession,
    legacy: Legacy,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("subroutine-auth-test-{}", Uuid::new_v4()));
        Self {
            store: ProtectedSession::new(&root, "oidc:v1:test-config"),
            root,
            legacy: Legacy::default(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn round_trip_sessions_beyond_credential_manager_limit() {
    let fixture = Fixture::new();
    for size in [0, 2560, 2561, 8192, 64 * 1024] {
        let bytes = vec![b'a'; size];
        fixture.store.save(&bytes, &fixture.legacy).unwrap();
        assert_eq!(fixture.store.load(&fixture.legacy).unwrap(), Some(bytes));
    }
    let secret = "synthetic session with Unicode: 日本語🔐"
        .repeat(300)
        .into_bytes();
    fixture.store.save(&secret, &fixture.legacy).unwrap();
    let encoded = fs::read(&fixture.store.path).unwrap();
    assert!(!encoded.windows(64).any(|chunk| chunk == &secret[..64]));
    assert_eq!(fixture.store.load(&fixture.legacy).unwrap(), Some(secret));
}

#[test]
fn checkpoint_replaces_large_session_without_resurrecting_legacy_tokens() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(&vec![b'a'; 8192], &fixture.legacy)
        .unwrap();
    *fixture.legacy.bytes.lock().unwrap() = Some(b"old-refresh-token".to_vec());
    let checkpoint = br#"{"tokens":null}"#;
    fixture.store.save(checkpoint, &fixture.legacy).unwrap();
    let restarted = ProtectedSession::new(&fixture.root, "oidc:v1:test-config");
    assert_eq!(
        restarted.load(&fixture.legacy).unwrap().unwrap(),
        checkpoint
    );
    assert!(fixture.legacy.load().unwrap().is_none());
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
}

#[test]
fn legacy_is_migrated_before_removal() {
    let fixture = Fixture::new();
    let bytes = b"legacy-session".to_vec();
    *fixture.legacy.bytes.lock().unwrap() = Some(bytes.clone());
    assert_eq!(
        fixture.store.load(&fixture.legacy).unwrap(),
        Some(bytes.clone())
    );
    assert!(fixture.store.path.is_file());
    assert!(fixture.legacy.load().unwrap().is_none());
    assert_eq!(fixture.store.load(&fixture.legacy).unwrap(), Some(bytes));
}

#[test]
fn failed_migration_write_keeps_legacy_session() {
    let fixture = Fixture::new();
    fs::write(&fixture.root, b"not a directory").unwrap();
    *fixture.legacy.bytes.lock().unwrap() = Some(b"legacy-session".to_vec());
    assert!(fixture.store.load(&fixture.legacy).is_err());
    assert!(fixture.legacy.load().unwrap().is_some());
    fs::remove_file(&fixture.root).unwrap();
}

#[test]
fn interrupted_legacy_retirement_retries_without_using_stale_session() {
    let fixture = Fixture::new();
    *fixture.legacy.bytes.lock().unwrap() = Some(b"old-refresh-token".to_vec());
    *fixture.legacy.fail_delete.lock().unwrap() = true;
    assert!(fixture.store.save(b"new-session", &fixture.legacy).is_err());
    assert!(fixture.store.path.is_file());
    assert!(fixture.store.load(&fixture.legacy).is_err());
    *fixture.legacy.fail_delete.lock().unwrap() = false;
    assert_eq!(
        fixture.store.load(&fixture.legacy).unwrap().unwrap(),
        b"new-session"
    );
    assert!(fixture.legacy.load().unwrap().is_none());
}

#[test]
fn failed_atomic_replace_preserves_committed_session_and_removes_temp_file() {
    let fixture = Fixture::new();
    fixture.store.save(b"committed", &fixture.legacy).unwrap();
    // Refuse delete-sharing to force the actual Windows replacement API to fail.
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&fixture.store.path)
        .unwrap();
    assert!(fixture.store.save(b"uncommitted", &fixture.legacy).is_err());
    drop(locked);
    assert_eq!(
        fixture.store.load(&fixture.legacy).unwrap().unwrap(),
        b"committed"
    );
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
}

#[test]
fn corrupt_ciphertext_never_falls_back_to_legacy() {
    let fixture = Fixture::new();
    fixture.store.save(b"new-session", &fixture.legacy).unwrap();
    *fixture.legacy.bytes.lock().unwrap() = Some(b"old-refresh-token".to_vec());
    let mut encoded = fs::read(&fixture.store.path).unwrap();
    let last = encoded.len() - 1;
    encoded[last] ^= 1;
    fs::write(&fixture.store.path, &encoded).unwrap();
    assert!(fixture.store.load(&fixture.legacy).is_err());
    assert!(fixture.legacy.load().unwrap().is_some());
    fs::write(&fixture.store.path, b"unknown format").unwrap();
    assert!(fixture.store.load(&fixture.legacy).is_err());
}

#[test]
fn ciphertext_is_bound_to_configuration() {
    let fixture = Fixture::new();
    fixture.store.save(b"session", &fixture.legacy).unwrap();
    let other = ProtectedSession::new(&fixture.root, "oidc:v1:other-config");
    fs::copy(&fixture.store.path, &other.path).unwrap();
    assert!(other.load(&fixture.legacy).is_err());
}

#[test]
fn sign_out_retires_legacy_before_removing_new_session_and_is_idempotent() {
    let fixture = Fixture::new();
    fixture.store.save(b"new-session", &fixture.legacy).unwrap();
    *fixture.legacy.bytes.lock().unwrap() = Some(b"old-session".to_vec());
    *fixture.legacy.fail_delete.lock().unwrap() = true;
    assert!(fixture.store.delete(&fixture.legacy).is_err());
    assert!(fixture.store.path.exists());
    *fixture.legacy.fail_delete.lock().unwrap() = false;
    fixture.store.delete(&fixture.legacy).unwrap();
    fixture.store.delete(&fixture.legacy).unwrap();
    assert!(fixture.store.load(&fixture.legacy).unwrap().is_none());
}

#[test]
fn size_limits_preserve_previous_session() {
    let fixture = Fixture::new();
    fixture.store.save(b"committed", &fixture.legacy).unwrap();
    assert!(
        fixture
            .store
            .save(&vec![b'a'; MAX_SESSION_BYTES + 1], &fixture.legacy)
            .is_err()
    );
    assert_eq!(
        fixture.store.load(&fixture.legacy).unwrap().unwrap(),
        b"committed"
    );
    fs::write(&fixture.store.path, vec![0; MAX_FILE_BYTES + 1]).unwrap();
    assert!(fixture.store.load(&fixture.legacy).is_err());
}

#[test]
fn uncommitted_temporary_files_are_not_recovered() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.root).unwrap();
    fs::write(
        fixture.store.path.with_extension("orphan.tmp"),
        b"uncommitted",
    )
    .unwrap();
    assert!(fixture.store.load(&fixture.legacy).unwrap().is_none());
}
