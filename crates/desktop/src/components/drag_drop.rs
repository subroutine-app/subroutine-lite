use gpui::{div, prelude::FluentBuilder as _, px, *};
use gpui_kit_theme::ActiveTheme;
use std::fmt::Debug;
use std::rc::Rc;

type DragListener = dyn Fn(&mut Window, &mut App) + 'static;

pub struct DragData<T: Clone + Debug> {
    pub data: T,
    pub label: Option<SharedString>,
    pub preview_factory: Option<Rc<dyn Fn() -> AnyElement>>,
    pub preview_size: Option<Size<Pixels>>,
    pub position: Point<Pixels>,
}

impl<T: Clone + Debug> Clone for DragData<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            label: self.label.clone(),
            preview_factory: self.preview_factory.clone(),
            preview_size: self.preview_size,
            position: self.position,
        }
    }
}

impl<T: Clone + Debug> Debug for DragData<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragData")
            .field("data", &self.data)
            .field("label", &self.label)
            .field("preview_factory", &self.preview_factory.is_some())
            .field("preview_size", &self.preview_size)
            .field("position", &self.position)
            .finish()
    }
}

impl<T: Clone + Debug> DragData<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            label: None,
            preview_factory: None,
            preview_size: None,
            position: Point::default(),
        }
    }

    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_preview<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.preview_factory = Some(Rc::new(factory));
        self
    }

    pub fn with_preview_size(mut self, size: Size<Pixels>) -> Self {
        self.preview_size = Some(size);
        self
    }

    pub fn with_position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl<T: Clone + Debug + 'static> Render for DragData<T> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if let Some(factory) = &self.preview_factory {
            let preview = factory();

            let (offset_x, offset_y) = if let Some(preview_size) = self.preview_size {
                let padding = px(8.0);
                let max_x = (preview_size.width - padding).max(padding);
                let max_y = (preview_size.height - padding).max(padding);
                let clamped_x = self.position.x.max(padding).min(max_x);
                let clamped_y = self.position.y.max(padding).min(max_y);
                (self.position.x - clamped_x, self.position.y - clamped_y)
            } else {
                (px(0.0), px(0.0))
            };

            return div().absolute().left(offset_x).top(offset_y).child(preview);
        }

        let size = gpui::size(px(250.0), px(80.0));

        div()
            .pl(self.position.x - size.width / 2.0)
            .pt(self.position.y - size.height / 2.0)
            .child(
                div()
                    .flex()
                    .justify_center()
                    .items_center()
                    .min_w(size.width)
                    .max_w(px(300.0))
                    .min_h(size.height)
                    .px(px(16.0))
                    .py(px(12.0))
                    .bg(theme.colors.panel.opacity(0.95))
                    .border_1()
                    .border_color(theme.colors.hairline)
                    .text_color(theme.colors.text)
                    .font_family(theme.typography.sans.clone())
                    .text_size(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .rounded_md()
                    .shadow(vec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.3),
                        offset: point(px(0.0), px(4.0)),
                        blur_radius: px(12.0),
                        spread_radius: px(0.0),
                        style: ShadowStyle::Drop,
                    }])
                    .when_some(self.label.clone(), |this, label| this.child(label))
                    .when(self.label.is_none(), |this| this.child("Dragging...")),
            )
    }
}

#[derive(IntoElement)]
pub struct Draggable<T: Clone + Debug + 'static> {
    base: Stateful<Div>,
    drag_data: DragData<T>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
    on_drag_listener: Option<Box<DragListener>>,
}

impl<T: Clone + Debug + 'static> Draggable<T> {
    pub fn new(id: impl Into<ElementId>, drag_data: DragData<T>) -> Self {
        Self {
            base: div().id(id.into()),
            drag_data,
            children: Vec::new(),
            style: StyleRefinement::default(),
            on_drag_listener: None,
        }
    }

    pub fn on_drag_start(mut self, listener: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_drag_listener = Some(Box::new(listener));
        self
    }
}

impl<T: Clone + Debug + 'static> InteractiveElement for Draggable<T> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl<T: Clone + Debug + 'static> StatefulInteractiveElement for Draggable<T> {}

impl<T: Clone + Debug + 'static> Styled for Draggable<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + Debug + 'static> ParentElement for Draggable<T> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl<T: Clone + Debug + 'static> RenderOnce for Draggable<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let drag_data = self.drag_data.clone();
        let user_style = self.style;

        let listener = self.on_drag_listener;

        self.base
            .cursor(CursorStyle::OpenHand)
            .on_drag(
                drag_data,
                move |data: &DragData<T>, position, window, cx| {
                    if let Some(ref listener) = listener {
                        listener(window, cx);
                    }
                    cx.new(|_| data.clone().with_position(position))
                },
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct DropZone<T: Clone + Debug + 'static> {
    base: Stateful<Div>,
    active: bool,
    children: Vec<AnyElement>,
    user_style: StyleRefinement,
    transparent_background: bool,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Clone + Debug + 'static> DropZone<T> {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id.into()),
            active: false,
            children: Vec::new(),
            user_style: StyleRefinement::default(),
            transparent_background: false,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn transparent_background(mut self) -> Self {
        self.transparent_background = true;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children<I>(mut self, children: impl IntoIterator<Item = I>) -> Self
    where
        I: IntoElement,
    {
        for child in children {
            self.children.push(child.into_any_element());
        }
        self
    }
}

impl<T: Clone + Debug + 'static> Styled for DropZone<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.user_style
    }
}

impl<T: Clone + Debug + 'static> InteractiveElement for DropZone<T> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl<T: Clone + Debug + 'static> StatefulInteractiveElement for DropZone<T> {}

impl<T: Clone + Debug + 'static> ParentElement for DropZone<T> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl<T: Clone + Debug + 'static> RenderOnce for DropZone<T> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let user_style = self.user_style;

        let border_style = StyleRefinement {
            corner_radii: user_style.corner_radii.clone(),
            ..Default::default()
        };
        let border_overlay = self.active.then(|| {
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme.radii.card))
                .border_color(theme.colors.accent)
                .border(px(1.))
                .map(|mut this| {
                    this.style().refine(&border_style);
                    this
                })
        });
        let bg_color = if self.transparent_background {
            gpui::transparent_black()
        } else {
            theme.colors.canvas
        };

        self.base
            .relative()
            .flex()
            .flex_col()
            .items_start()
            .justify_start()
            .gap(px(8.0))
            .w_full()
            .rounded(px(theme.radii.card))
            .bg(bg_color)
            .drag_over::<DragData<T>>(move |style, _, _, _| {
                style.bg(theme.colors.accent.opacity(0.1))
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .children(self.children)
            .children(border_overlay)
    }
}
