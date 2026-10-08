use gpui::{App, Axis, Bounds, ClickEvent, InteractiveElement, ParentElement, Pixels, Stateful};
use gpui::{Refineable as _, StyleRefinement, Styled, Window, canvas};

pub trait AxisExt {
    fn is_horizontal(&self) -> bool;
    fn is_vertical(&self) -> bool;
}

impl AxisExt for Axis {
    fn is_horizontal(&self) -> bool {
        *self == Axis::Horizontal
    }

    fn is_vertical(&self) -> bool {
        *self == Axis::Vertical
    }
}

pub trait InteractiveElementExt: InteractiveElement {
    fn on_double_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_click(move |event, window, cx| {
            if event.click_count() == 2 {
                listener(event, window, cx);
            }
        });
        self
    }
}

impl<E: InteractiveElement> InteractiveElementExt for Stateful<E> {}

pub trait ElementExt: ParentElement + Sized {
    fn on_prepaint<F>(self, f: F) -> Self
    where
        F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    {
        self.child(
            canvas(
                move |bounds, window, cx| f(bounds, window, cx),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }
}

impl<T: ParentElement> ElementExt for T {}

pub trait StyledRefineExt: Styled + Sized {
    fn refine_style(mut self, refinement: &StyleRefinement) -> Self {
        self.style().refine(refinement);
        self
    }
}

impl<T: Styled> StyledRefineExt for T {}
