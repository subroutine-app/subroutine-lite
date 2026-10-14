use gpui::{AppContext, Context, Task};
use subroutine_import::provider::SyncOutcome;

use super::AppDatabaseStore;

impl AppDatabaseStore {
    pub fn apply_provider_import(
        &mut self,
        outcome: SyncOutcome,
        cx: &mut Context<Self>,
    ) -> Task<Result<usize, String>> {
        if !self.is_ready() {
            return Task::ready(Err("The active workspace is not ready for imports.".into()));
        }
        let Some(persistence) = self.persistence.clone() else {
            return Task::ready(Err("Local workspace unavailable.".into()));
        };
        let generation = self.workspace_generation;
        let local_store = persistence.local_store().clone();
        cx.spawn(async move |this, cx| {
            let changed = cx
                .background_spawn(async move {
                    local_store
                        .apply_provider_import(outcome)
                        .map(|result| result.changed)
                        .map_err(|error| error.to_string())
                })
                .await?;
            this.update(cx, |store, cx| {
                if store.workspace_generation != generation {
                    return Err("The active workspace changed during the import.".into());
                }
                // Foreground edits may have committed since reconciliation finished.
                let projection = persistence.projection()?;
                store.replace_projection(projection, cx);
                if persistence.is_remote() {
                    store.start_pull(cx);
                }
                Ok(changed)
            })
            .map_err(|error| error.to_string())?
        })
    }
}
