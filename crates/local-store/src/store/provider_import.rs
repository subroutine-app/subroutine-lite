use subroutine_import::provider::SyncOutcome;

use super::{LocalStore, worker::Command};
use crate::{ProviderImportResult, Result};

impl LocalStore {
    pub fn apply_provider_import(&self, outcome: SyncOutcome) -> Result<ProviderImportResult> {
        self.call(|reply| Command::ApplyProviderImport(outcome, reply))
    }
}
