use super::SyncStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceContent {
    Loading,
    Unavailable,
    Ready,
}

impl WorkspaceContent {
    pub(super) fn remote(authoritative_snapshot: bool, sync_status: &SyncStatus) -> Self {
        match (authoritative_snapshot, sync_status) {
            (true, _) => Self::Ready,
            (false, SyncStatus::Syncing) => Self::Loading,
            (false, _) => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_account_cache_requires_confirmation_but_cached_data_works_offline() {
        assert_eq!(
            WorkspaceContent::remote(false, &SyncStatus::Syncing),
            WorkspaceContent::Loading,
        );
        for status in [
            SyncStatus::Idle,
            SyncStatus::Offline,
            SyncStatus::AuthenticationRequired,
        ] {
            assert_eq!(
                WorkspaceContent::remote(false, &status),
                WorkspaceContent::Unavailable
            );
            assert_eq!(
                WorkspaceContent::remote(true, &status),
                WorkspaceContent::Ready
            );
        }
        assert_eq!(
            WorkspaceContent::remote(true, &SyncStatus::Syncing),
            WorkspaceContent::Ready
        );
    }
}
