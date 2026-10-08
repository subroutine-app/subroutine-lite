use gpui::{AnyElement, App, Context, ElementId, IntoElement, Pixels, Window, div};

use super::{DYNAMIC_LIST_ITEM_HEIGHT, DynamicListState};

#[allow(unused_variables)]
pub trait DynamicListDelegate: Clone + Sized + 'static {
    type Item: IntoElement;

    fn items_count(&self, cx: &App) -> usize;

    fn item_id(&self, ix: usize, cx: &App) -> ElementId;

    fn layout_epoch(&self) -> u64 {
        0
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {}

    fn pause_layout(&mut self) {}

    fn item_height(&self, ix: usize, cx: &App) -> Pixels {
        DYNAMIC_LIST_ITEM_HEIGHT
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<Self::Item>;

    fn can_drag(&self, ix: usize, cx: &App) -> bool {
        true
    }

    fn render_drag_preview(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<AnyElement> {
        self.render_item(ix, window, cx)
            .map(IntoElement::into_any_element)
    }

    fn move_item(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    );

    fn render_empty(
        &mut self,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        div()
    }
}
