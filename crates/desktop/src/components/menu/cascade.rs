
use std::{cell::RefCell, rc::Rc};

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, Size, Style, Window, point, px, size,
};
use gpui_kit::foundation::direction::{LayoutDirection, PhysicalSide};

pub(super) struct CascadePanel {
    pub element: AnyElement,
    pub owner_offset: Pixels,
}

pub(super) type PanelBounds = Rc<RefCell<Vec<Bounds<Pixels>>>>;

pub(super) fn cascade(
    panels: Vec<CascadePanel>,
    bounds: PanelBounds,
    direction: LayoutDirection,
    gap: Pixels,
    margin: Pixels,
) -> impl IntoElement {
    Cascade {
        panels,
        bounds,
        direction,
        gap,
        margin,
    }
}

struct Cascade {
    panels: Vec<CascadePanel>,
    bounds: PanelBounds,
    direction: LayoutDirection,
    gap: Pixels,
    margin: Pixels,
}

impl IntoElement for Cascade {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Cascade {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = if let Some(root) = self.panels.first_mut() {
            root.element.request_layout(window, cx)
        } else {
            window.request_layout(
                Style {
                    size: size(px(0.).into(), px(0.).into()),
                    ..Style::default()
                },
                [],
                cx,
            )
        };
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((root, children)) = self.panels.split_first_mut() else {
            self.bounds.borrow_mut().clear();
            return;
        };

        root.element.prepaint(window, cx);
        let mut parent = PanelPlacement {
            bounds,
            side: self.direction.end(),
        };
        let mut placed = Vec::with_capacity(children.len() + 1);
        placed.push(bounds);
        let viewport = window.viewport_size();

        for panel in children {
            let layout_id = panel.element.request_layout(window, cx);
            let natural = window.with_absolute_element_offset(Point::default(), |window| {
                panel.element.layout_as_root(
                    size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                    window,
                    cx,
                );
                window.layout_bounds(layout_id)
            });
            let mut child = place_child(
                parent,
                natural.size,
                panel.owner_offset,
                viewport,
                self.gap,
                self.margin,
            );

            let origin = window.pixel_snap_point(child.bounds.origin - natural.origin);
            panel.element.prepaint_at(origin, window, cx);
            child.bounds.origin = natural.origin + origin;
            placed.push(child.bounds);
            parent = child;
        }

        *self.bounds.borrow_mut() = placed;
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        for panel in &mut self.panels {
            panel.element.paint(window, cx);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PanelPlacement {
    bounds: Bounds<Pixels>,
    side: PhysicalSide,
}

fn place_child(
    parent: PanelPlacement,
    child_size: Size<Pixels>,
    owner_offset: Pixels,
    viewport: Size<Pixels>,
    gap: Pixels,
    margin: Pixels,
) -> PanelPlacement {
    let gap = gap.max(px(0.));
    let margin = margin.max(px(0.));
    let left = margin.min(viewport.width / 2.);
    let top = margin.min(viewport.height / 2.);
    let right = viewport.width - left;
    let bottom = viewport.height - top;
    let left_x = parent.bounds.left() - gap - child_size.width;
    let right_x = parent.bounds.right() + gap;
    let candidate = |side| match side {
        PhysicalSide::Left => left_x,
        PhysicalSide::Right => right_x,
    };
    let fits = |side| {
        let x = candidate(side);
        x >= left && x + child_size.width <= right
    };
    let room = |side| match side {
        PhysicalSide::Left => parent.bounds.left() - gap - left,
        PhysicalSide::Right => right - right_x,
    };
    let preferred = parent.side;
    let opposite = preferred.opposite();
    let side = if fits(preferred) {
        preferred
    } else if fits(opposite) || room(opposite) > room(preferred) {
        opposite
    } else {
        preferred
    };

    PanelPlacement {
        bounds: Bounds::new(
            point(
                candidate(side)
                    .max(left)
                    .min((right - child_size.width).max(left)),
                (parent.bounds.top() + owner_offset)
                    .max(top)
                    .min((bottom - child_size.height).max(top)),
            ),
            child_size,
        ),
        side,
    }
}
