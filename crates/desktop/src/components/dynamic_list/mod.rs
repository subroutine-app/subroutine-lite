mod card_states;
mod delegate;
mod list;

pub(crate) use card_states::DynamicListCardStates;
pub use delegate::*;
pub use list::*;

use gpui::{Pixels, px};

use crate::components::DEFAULT_ITEM_HEIGHT;

pub const DYNAMIC_LIST_ITEM_HEIGHT: Pixels = DEFAULT_ITEM_HEIGHT;

pub const DYNAMIC_LIST_ITEM_GAP: Pixels = px(8.);

pub fn apply_reorder<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    let item = items.remove(from);
    items.insert(to, item);
}
