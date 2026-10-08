use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, Hsla, InteractiveElement, Interactivity,
    IntoElement, ParentElement, RenderOnce, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::{Disableable, FocusRing as _, Selectable, Sizable, StyledExt as _};
use gpui_kit::overlay::{GlassExt as _, GlassPreset, Tooltip};
use gpui_kit_theme::{ActiveTheme as _, ControlSize, Radius, Surface, Theme, Variant};

use crate::components::ext::StyledRefineExt as _;
use crate::icons::Icon;

type ClickHandler = dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Secondary,
    Primary,
    Ghost,
    Outline,
    Danger,
}

pub trait ButtonVariants: Sized {
    fn with_variant(self, variant: ButtonVariant) -> Self;

    fn primary(self) -> Self {
        self.with_variant(ButtonVariant::Primary)
    }
    fn ghost(self) -> Self {
        self.with_variant(ButtonVariant::Ghost)
    }
    fn outline(self) -> Self {
        self.with_variant(ButtonVariant::Outline)
    }
    fn danger(self) -> Self {
        self.with_variant(ButtonVariant::Danger)
    }
}

struct Paint {
    background: Hsla,
    foreground: Hsla,
    hover: Hsla,
    active: Hsla,
    border: Option<Hsla>,
}

impl ButtonVariant {
    fn paint(self, theme: &Theme, selected: bool) -> Paint {
        let colors = &theme.colors;
        let transparent = gpui::transparent_black();
        let critical = crate::presentation::UxColor::Critical.color(theme);

        let mut paint = match self {
            Self::Primary => {
                let shades = theme.variant_colors(Variant::Filled, &colors.primary_fill.into());
                Paint {
                    background: colors.primary_fill,
                    foreground: colors.text_on_primary_fill,
                    hover: shades.background_hover,
                    active: shades.background_active,
                    border: None,
                }
            }
            Self::Secondary => Paint {
                background: colors.control,
                foreground: colors.text,
                hover: colors.control_hover,
                active: colors.control_pressed,
                border: Some(colors.hairline),
            },
            Self::Ghost => Paint {
                background: transparent,
                foreground: colors.text,
                hover: colors.hover,
                active: colors.active,
                border: None,
            },
            Self::Outline => Paint {
                background: transparent,
                foreground: colors.text,
                hover: colors.hover,
                active: colors.active,
                border: Some(colors.hairline_strong),
            },
            Self::Danger => Paint {
                background: critical,
                foreground: colors.text_on_accent,
                hover: critical,
                active: critical,
                border: None,
            },
        };

        if selected && matches!(self, Self::Ghost | Self::Outline | Self::Secondary) {
            paint.background = colors.selected;
            paint.hover = colors.selected.blend(colors.hover);
            paint.active = colors.selected.blend(colors.active);
        }

        paint
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Selection {
    #[default]
    None,
    Toggle,
    Current,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ButtonSurface {
    #[default]
    Control,
    GlassPill,
}

#[derive(IntoElement)]
pub struct Button {
    base: Stateful<gpui::Div>,
    style: StyleRefinement,
    variant: ButtonVariant,
    surface: ButtonSurface,
    size: ControlSize,
    label: Option<SharedString>,
    icon: Option<Icon>,
    children: Vec<AnyElement>,
    disabled: bool,
    selection: Selection,
    compact: bool,
    tab_stop: bool,
    hover: Option<StyleRefinement>,
    focus_handle: Option<FocusHandle>,
    tooltip: Option<SharedString>,
    on_click: Option<Box<ClickHandler>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            variant: ButtonVariant::default(),
            surface: ButtonSurface::default(),
            size: ControlSize::Md,
            label: None,
            icon: None,
            children: Vec::new(),
            disabled: false,
            selection: Selection::None,
            compact: false,
            tab_stop: true,
            hover: None,
            focus_handle: None,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<ButtonIcon>) -> Self {
        self.icon = Some(icon.into().0);
        self
    }

    pub fn current(mut self, current: bool) -> Self {
        self.selection = if current {
            Selection::Current
        } else {
            Selection::None
        };
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn glass_pill(mut self) -> Self {
        self.surface = ButtonSurface::GlassPill;
        self.variant = ButtonVariant::Ghost;
        self.rounded_full()
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn xsmall(self) -> Self {
        self.control_size(ControlSize::Xs)
    }
}

pub struct ButtonIcon(Icon);

impl From<Icon> for ButtonIcon {
    fn from(icon: Icon) -> Self {
        Self(icon)
    }
}

impl<T: crate::icons::IconNamed> From<T> for ButtonIcon {
    fn from(named: T) -> Self {
        Self(Icon::new(named))
    }
}

impl ButtonVariants for Button {
    fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.selection = if selected {
            Selection::Toggle
        } else {
            Selection::None
        };
        self
    }
}

impl Sizable for Button {
    fn control_size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }

    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.hover = Some(f(StyleRefinement::default()));
        self
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.surface == ButtonSurface::GlassPill && !self.disabled && self.focus_handle.is_none()
        {
            let id = gpui::Element::id(&self.base).expect("buttons have an element id");
            self.focus_handle = Some(
                window
                    .use_keyed_state((id, "focus"), cx, |_, cx| cx.focus_handle())
                    .read(cx)
                    .clone()
                    .tab_stop(self.tab_stop),
            );
        }
        let theme = cx.theme();
        let metrics = theme.control.get(self.size);
        let paint = self.variant.paint(theme, self.selection != Selection::None);
        let radius = px(theme.radii.control);
        let disabled_opacity = theme.opacity.disabled;

        let padding_x = if self.compact { 0.0 } else { metrics.padding_x };
        let interactive = !self.disabled && self.selection != Selection::Current;

        let mut button = self
            .base
            .row()
            .justify_center()
            .flex_none()
            .h(px(metrics.height))
            .px(px(padding_x))
            .gap(px(metrics.gap))
            .rounded(radius)
            .text_size(px(metrics.font_size))
            .bg(paint.background)
            .text_color(paint.foreground)
            .when_some(paint.border, |this, border| {
                this.border_1().border_color(border)
            })
            .when(self.surface == ButtonSurface::GlassPill, |this| {
                this.border_1().border_color(theme.colors.hairline)
            })
            .when(interactive, |this| {
                let hover = self.hover.clone();
                this.cursor_pointer()
                    .hover(move |style| match hover {
                        Some(caller) => caller,
                        None => style.bg(paint.hover),
                    })
                    .active(|this| this.bg(paint.active))
            })
            .when(self.disabled, |this| this.opacity(disabled_opacity))
            .when(self.tab_stop && !self.disabled, |this| {
                this.focusable().tab_stop(true)
            })
            .when_some(self.focus_handle.as_ref(), |this, handle| {
                this.track_focus(handle)
            });

        if let Some(mut icon) = self.icon {
            if !icon.has_size() {
                icon = icon.size(px(metrics.icon_size));
            }
            button = button.child(icon);
        }
        if let Some(label) = self.label {
            button = button.child(label);
        }
        button = button.children(self.children);

        let button = button
            .when_some(self.tooltip, |this, text| {
                this.tooltip(move |_window, cx| Tooltip::new(text.clone(), text.clone()).view(cx))
            })
            .when_some(self.on_click.filter(|_| interactive), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .refine_style(&self.style);

        match self.surface {
            ButtonSurface::Control => button
                .when(!self.disabled, |this| this.focus_ring(theme))
                .into_any_element(),
            ButtonSurface::GlassPill => {
                let focused = !self.disabled
                    && window.focus_is_visible()
                    && self
                        .focus_handle
                        .as_ref()
                        .is_some_and(|handle| handle.is_focused(window));
                button
                    .bg_glass()
                    .glass_surface(Surface::Overlay)
                    .glass_radius(Radius::Pill)
                    .glass(|glass| glass.protect_text_contrast(false).focused(focused))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    })
                    .into_any_element()
            }
        }
    }
}
