use gpui::ParentElement as _;
use gpui::{Div, SharedString, Styled as _, div};

pub struct Label;

impl Label {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(content: impl Into<SharedString>) -> Div {
        div().flex_none().child(content.into())
    }
}
