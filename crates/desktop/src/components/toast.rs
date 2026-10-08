use std::time::Duration;

use gpui::SharedString;
use gpui_kit::{foundation::Ident, overlay::Toast};

const IN_APP_TOAST_TIMEOUT: Duration = Duration::from_secs(6);

pub(crate) fn timed_toast(ident: impl Into<Ident>, message: impl Into<SharedString>) -> Toast {
    Toast::new(ident, message).timeout(IN_APP_TOAST_TIMEOUT)
}
