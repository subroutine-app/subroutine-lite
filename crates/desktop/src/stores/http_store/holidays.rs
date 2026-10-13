use gpui::Context;
use local_store::HolidaySetup;
use subroutine_holidays::Region;

use super::AppDatabaseStore;
use crate::auth::AuthSession;

impl AppDatabaseStore {
    pub(crate) fn holiday_setup_pending(&self) -> bool {
        self.is_ready()
            && !self.has_remote_workspace()
            && self.holiday_setup == HolidaySetup::Pending
    }

    pub(crate) fn complete_holiday_setup(
        &mut self,
        region: Option<&Region>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.holiday_setup_pending()
            || !self.workspace_is_current(AuthSession::global(cx).scope())
        {
            return Err("Holiday setup is no longer available for this workspace.".into());
        }
        let persistence = self
            .persistence
            .as_ref()
            .ok_or("Local workspace unavailable.")?;
        let markers = region.map(subroutine_holidays::markers).unwrap_or_default();
        let projection = persistence.complete_holiday_setup(markers)?;
        self.replace_projection(projection, cx);
        Ok(())
    }
}
