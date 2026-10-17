use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use super::*;

#[derive(Default)]
enum Write {
    #[default]
    Save,
    Fail,
    Pause {
        entered: tokio::sync::oneshot::Sender<()>,
        resume: mpsc::Receiver<()>,
    },
}

struct MemoryCredentials {
    bytes: Mutex<Option<Vec<u8>>>,
    writes: AtomicUsize,
    next: Mutex<Write>,
}

impl Credentials for MemoryCredentials {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.bytes.lock().unwrap().clone())
    }

    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        match std::mem::take(&mut *self.next.lock().unwrap()) {
            Write::Save => {}
            Write::Fail => return Err("test credential store unavailable".into()),
            Write::Pause { entered, resume } => {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
        *self.bytes.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.bytes.lock().unwrap() = None;
        Ok(())
    }
}

impl MemoryCredentials {
    fn saved(&self) -> SavedSession {
        serde_json::from_slice(&self.load().unwrap().unwrap()).unwrap()
    }
}

fn profile(name: Option<&str>) -> Profile {
    Profile {
        subject: "test-subject".into(),
        name: name.map(str::to_owned),
    }
}

fn fixture() -> (
    AuthSession,
    Arc<MemoryCredentials>,
    SavedSession,
    RequestScope,
) {
    let config = Config {
        issuer: "https://issuer.example".into(),
        client_id: "subroutine-desktop".into(),
        api: "https://api.example".into(),
        allow_loopback: false,
    };
    let now = chrono::Utc::now().timestamp();
    let saved = SavedSession {
        config: config.clone(),
        account_id: Uuid::new_v4(),
        tokens: Some(Tokens::from(
            subroutine_auth::client::Tokens::from_stored(
                "rotated-access".into(),
                "rotated-refresh".into(),
                now + 300,
                Some(now + 241),
            )
            .unwrap(),
        )),
        profile: Some(profile(Some("Old override"))),
        persistence: Persistence::Durable,
    };
    // The critical token write has already succeeded before profile reconciliation starts.
    let store = Arc::new(MemoryCredentials {
        bytes: Mutex::new(Some(serde_json::to_vec(&saved).unwrap())),
        writes: AtomicUsize::new(0),
        next: Mutex::new(Write::Save),
    });
    let session = AuthSession(Arc::new(Inner {
        config: Ok(Some(config)),
        credentials: Some(store.clone()),
        state: Mutex::new(SessionState {
            saved: Some(saved.clone()),
            authentication: AuthenticationState::Refreshing,
            revision: 1,
            ..Default::default()
        }),
        operation: tokio::sync::Mutex::new(()),
    }));
    let scope = session.scope().unwrap();
    (session, store, saved, scope)
}

#[tokio::test]
async fn changed_profile_and_name_reset_survive_serialized_offline_restore() {
    for name in [Some("Current provider name"), None] {
        let (session, store, mut saved, scope) = fixture();
        let _operation = session.0.operation.lock().await;
        session
            .persist_profile(scope, &mut saved, Some(profile(name)))
            .await
            .unwrap();
        let restored = store.saved();
        assert_eq!(restored.profile.unwrap().name.as_deref(), name);
        assert_eq!(restored.tokens.unwrap().refresh, "rotated-refresh");
        assert!(saved.persistence == Persistence::Durable);
        assert!(
            session
                .0
                .state
                .lock()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .persistence
                == Persistence::Durable
        );
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn absent_unchanged_and_other_subject_profiles_do_not_write() {
    let (session, store, mut saved, scope) = fixture();
    let _operation = session.0.operation.lock().await;
    let original = serde_json::to_value(&saved).unwrap();
    for fetched in [
        None,
        Some(profile(Some("Old override"))),
        Some(Profile {
            subject: "other-subject".into(),
            name: Some("Other name".into()),
        }),
    ] {
        session
            .persist_profile(scope, &mut saved, fetched)
            .await
            .unwrap();
    }
    assert_eq!(store.writes.load(Ordering::SeqCst), 0);
    assert_eq!(serde_json::to_value(&saved).unwrap(), original);
    assert!(saved.persistence == Persistence::Durable);
}

#[tokio::test]
async fn failed_profile_save_retries_pending_snapshot_without_losing_rotated_tokens() {
    let (session, store, mut saved, scope) = fixture();
    let _operation = session.0.operation.lock().await;
    *store.next.lock().unwrap() = Write::Fail;
    let error = session
        .persist_profile(scope, &mut saved, Some(profile(None)))
        .await
        .unwrap_err();
    let TokenError::Transient(message) = error else {
        panic!("profile writes must remain retryable")
    };
    session.fail(scope.generation, message, true);
    let mut retry = {
        let state = session.0.state.lock().unwrap();
        assert!(matches!(
            state.authentication,
            AuthenticationState::Offline(_)
        ));
        assert!(state.retry_at.is_some());
        assert!(!state.verified);
        let pending = state.saved.as_ref().unwrap();
        assert!(pending.persistence == Persistence::Pending);
        assert!(pending.profile.as_ref().unwrap().name.is_none());
        assert_eq!(pending.tokens.as_ref().unwrap().refresh, "rotated-refresh");
        pending.clone()
    };
    assert!(saved.persistence == Persistence::Pending);
    let durable = store.saved();
    assert_eq!(durable.tokens.unwrap().refresh, "rotated-refresh");
    assert_eq!(
        durable.profile.unwrap().name.as_deref(),
        Some("Old override")
    );
    // renew retries this pending write before considering another token exchange.
    session.persist(scope, &mut retry).await.unwrap();
    let restored = store.saved();
    assert!(restored.profile.unwrap().name.is_none());
    assert_eq!(restored.tokens.unwrap().refresh, "rotated-refresh");
    assert!(retry.persistence == Persistence::Durable);
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    assert_eq!(session.0.state.lock().unwrap().revision, 1);
}

#[tokio::test]
async fn profile_save_checks_account_scope_before_and_after_the_async_write() {
    let (session, store, mut saved, scope) = fixture();
    {
        let _operation = session.0.operation.lock().await;
        session
            .0
            .state
            .lock()
            .unwrap()
            .saved
            .as_mut()
            .unwrap()
            .account_id = Uuid::new_v4();
        assert!(matches!(
            session
                .persist_profile(scope, &mut saved, Some(profile(None)))
                .await,
            Err(TokenError::Transient(_))
        ));
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
    }

    let (session, store, mut saved, scope) = fixture();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (resume, resumed) = mpsc::channel();
    *store.next.lock().unwrap() = Write::Pause {
        entered,
        resume: resumed,
    };
    let writing = session.clone();
    let task = tokio::spawn(async move {
        let _operation = writing.0.operation.lock().await;
        writing
            .persist_profile(scope, &mut saved, Some(profile(None)))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(
        session
            .0
            .state
            .lock()
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .persistence
            == Persistence::Pending
    );
    session.begin_sign_out();
    resume.send(()).unwrap();
    assert!(matches!(task.await.unwrap(), Err(TokenError::Transient(_))));
    assert!(session.0.state.lock().unwrap().saved.is_none());
    assert_eq!(session.state(), AuthenticationState::SigningOut);
    // Sign-out's credential deletion is serialized after the in-flight save by the same lock.
    let _operation = session.0.operation.lock().await;
    session.credential(|store| store.delete()).await.unwrap();
    assert!(store.load().unwrap().is_none());
}
