use std::{rc::Rc, time::Duration};

use super::cascade::{CascadePanel, PanelBounds, cascade};

use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::direction::{ActiveDirection as _, LayoutDirection};
use gpui_kit::foundation::{Ident, StyledExt as _, text};
use gpui_kit::motion;
use gpui_kit::overlay::popover::{self, MenuKey};
use gpui_kit::overlay::{FocusTrap, GlassExt as _, GlassPreset, Kbd, Overlay, Placement};
use gpui_kit_assets::{Icon, icon};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme as _, Radius, Space, Surface, Theme, TypeScale};

const GLYPH_SLOT: f32 = 14.0;
const SUBMENU_HOVER_DELAY: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuItemKind {
    Command,
    Check(bool),
    Separator,
    Section,
    Submenu(Vec<MenuItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuItem {
    id: SharedString,
    label: SharedString,
    kind: MenuItemKind,
    shortcut: Option<SharedString>,
}

impl MenuItem {
    pub(super) fn command(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: MenuItemKind::Command,
            shortcut: None,
        }
    }

    pub(super) fn check(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            kind: MenuItemKind::Check(checked),
            ..Self::command(id, label)
        }
    }

    pub(super) fn separator(id: impl Into<SharedString>) -> Self {
        Self {
            kind: MenuItemKind::Separator,
            ..Self::command(id, "")
        }
    }

    pub(super) fn section(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            kind: MenuItemKind::Section,
            ..Self::command(id, label)
        }
    }

    pub(super) fn submenu(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        items: impl IntoIterator<Item = MenuItem>,
    ) -> Self {
        Self {
            kind: MenuItemKind::Submenu(items.into_iter().collect()),
            ..Self::command(id, label)
        }
    }

    pub(super) fn set_shortcut(&mut self, shortcut: SharedString) {
        self.shortcut = Some(shortcut);
    }

    pub(super) fn id(&self) -> &SharedString {
        &self.id
    }

    fn is_selectable(&self) -> bool {
        matches!(
            self.kind,
            MenuItemKind::Command | MenuItemKind::Check(_) | MenuItemKind::Submenu(_)
        )
    }

    fn children(&self) -> Option<&[MenuItem]> {
        match &self.kind {
            MenuItemKind::Submenu(children) => Some(children),
            _ => None,
        }
    }

    pub(super) fn children_mut(&mut self) -> Option<&mut [MenuItem]> {
        match &mut self.kind {
            MenuItemKind::Submenu(children) => Some(children),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Activation {
    Invoked(SharedString),
    OpenedSubmenu,
    Ignored,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MenuState {
    path: Vec<usize>,
    active: Option<usize>,
}

fn level<'a>(items: &'a [MenuItem], path: &[usize]) -> &'a [MenuItem] {
    let mut level = items;
    for index in path {
        match level.get(*index).and_then(MenuItem::children) {
            Some(children) => level = children,
            None => break,
        }
    }
    level
}

fn item_at<'a>(items: &'a [MenuItem], path: &[usize]) -> Option<&'a MenuItem> {
    let (last, parents) = path.split_last()?;
    level(items, parents).get(*last)
}

fn first_selectable(items: &[MenuItem]) -> Option<usize> {
    items.iter().position(MenuItem::is_selectable)
}

impl MenuState {
    fn current<'a>(&self, items: &'a [MenuItem]) -> &'a [MenuItem] {
        level(items, &self.path)
    }

    fn reset(&mut self) {
        self.path.clear();
        self.active = None;
    }

    fn step(&mut self, items: &[MenuItem], delta: isize) {
        let level = self.current(items);
        let count = level.len();
        let Some(start) = popover::step(self.active, count, delta) else {
            return;
        };
        let mut index = start;
        for _ in 0..count {
            if level[index].is_selectable() {
                self.active = Some(index);
                return;
            }
            index = (index as isize + delta.signum()).rem_euclid(count as isize) as usize;
        }
    }

    fn jump(&mut self, items: &[MenuItem], letter: char) -> bool {
        let labels: Vec<Option<&str>> = self
            .current(items)
            .iter()
            .map(|item| item.is_selectable().then(|| item.label.as_ref()))
            .collect();
        match popover::jump_to(&labels, self.active, letter) {
            Some(index) => {
                self.active = Some(index);
                true
            }
            None => false,
        }
    }

    fn enter(&mut self, items: &[MenuItem]) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        let Some(item) = self.current(items).get(active) else {
            return false;
        };
        if !item.is_selectable() || item.children().is_none() {
            return false;
        }
        self.path.push(active);
        self.active = first_selectable(self.current(items));
        true
    }

    fn leave(&mut self) -> bool {
        match self.path.pop() {
            Some(index) => {
                self.active = Some(index);
                true
            }
            None => false,
        }
    }

    fn hover(&mut self, items: &[MenuItem], path: &[usize]) -> bool {
        let Some((last, parents)) = path.split_last() else {
            return false;
        };
        let Some(item) = item_at(items, path).filter(|item| item.is_selectable()) else {
            return false;
        };
        if item.children().is_some() && self.path.starts_with(path) {
            return false;
        }
        let previous = self.clone();
        if let Some(children) = item.children() {
            self.path = path.to_vec();
            self.active = first_selectable(children);
        } else {
            self.path = parents.to_vec();
            self.active = Some(*last);
        }
        *self != previous
    }

    fn activate(&mut self, items: &[MenuItem], path: &[usize]) -> Activation {
        let Some((last, parents)) = path.split_last() else {
            return Activation::Ignored;
        };
        let Some(item) = level(items, parents).get(*last) else {
            return Activation::Ignored;
        };
        if !item.is_selectable() {
            return Activation::Ignored;
        }

        self.path = parents.to_vec();
        self.active = Some(*last);
        if item.children().is_some() {
            self.path = path.to_vec();
            self.active = first_selectable(self.current(items));
            return Activation::OpenedSubmenu;
        }
        Activation::Invoked(item.id.clone())
    }
}

pub(super) enum ContextMenuEvent {
    Invoked(SharedString),
    Dismissed,
    Closed,
}

impl EventEmitter<ContextMenuEvent> for CompactContextMenu {}

type Activate<V> = Rc<dyn Fn(&mut V, Vec<usize>, &mut Window, &mut Context<V>)>;
type Hover<V> = Rc<dyn Fn(&mut V, Vec<usize>, bool, &mut Context<V>)>;

fn row_padding_x(theme: &Theme) -> f32 {
    theme.space(Space::Xs) + theme.borders.thick
}

fn row_padding_y(theme: &Theme) -> f32 {
    theme.space(Space::Xxs) + theme.borders.thick
}

fn menu_row_height(theme: &Theme) -> f32 {
    theme.typography.body.line_height + 2.0 * row_padding_y(theme)
}

fn menu_heading_height(theme: &Theme) -> f32 {
    theme.typography.caption.line_height + 2.0 * theme.space(Space::Xxs)
}

fn menu_separator_height(theme: &Theme) -> f32 {
    theme.space(Space::Xs)
}

fn rows_above(items: &[MenuItem], theme: &Theme) -> f32 {
    items
        .iter()
        .map(|item| match item.kind {
            MenuItemKind::Separator => menu_separator_height(theme),
            MenuItemKind::Section => menu_heading_height(theme),
            _ => menu_row_height(theme),
        })
        .sum()
}

fn panel_ident(ident: &Ident, items: &[MenuItem], base: &[usize]) -> Ident {
    match item_at(items, base) {
        Some(owner) => ident.child(owner.id.as_ref()).child("surface"),
        None => ident.child("surface"),
    }
}

#[allow(clippy::too_many_arguments)]
fn panels<V: 'static>(
    ident: &Ident,
    items: &[MenuItem],
    state: &MenuState,
    root_parent: SharedString,
    theme: &Theme,
    cx: &mut Context<V>,
    activate: Activate<V>,
    hover: Hover<V>,
) -> Vec<CascadePanel> {
    let depth = state.path.len();
    let mut rendered = Vec::with_capacity(depth + 1);

    for level_depth in 0..=depth {
        let base = state.path[..level_depth].to_vec();
        let parent = if level_depth == 0 {
            root_parent.clone()
        } else {
            item_at(items, &base)
                .map(|item| ident.child(item.id.as_ref()).semantic_id())
                .unwrap_or_else(|| root_parent.clone())
        };
        let active = (level_depth == depth).then_some(state.active).flatten();
        let opened = (level_depth < depth).then(|| state.path[level_depth]);
        let offset = if level_depth > 0 {
            let owner = state.path[level_depth - 1];
            let siblings = level(items, &state.path[..level_depth - 1]);
            rows_above(&siblings[..owner.min(siblings.len())], theme)
        } else {
            0.0
        };
        let panel = panel(
            ident,
            panel_ident(ident, items, &base),
            level(items, &base),
            &base,
            parent,
            active,
            opened,
            theme,
            cx,
            activate.clone(),
            hover.clone(),
        );
        rendered.push(CascadePanel {
            element: panel,
            owner_offset: px(offset),
        });
    }

    rendered
}

#[allow(clippy::too_many_arguments)]
fn panel<V: 'static>(
    ident: &Ident,
    surface_ident: Ident,
    items: &[MenuItem],
    base: &[usize],
    parent: SharedString,
    active: Option<usize>,
    opened: Option<usize>,
    theme: &Theme,
    cx: &mut Context<V>,
    activate: Activate<V>,
    hover: Hover<V>,
) -> AnyElement {
    let reserve_glyphs = items
        .iter()
        .any(|item| matches!(item.kind, MenuItemKind::Check(_)));
    let rows = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let mut path = base.to_vec();
            path.push(index);
            row(
                ident,
                item,
                path,
                parent.clone(),
                active == Some(index),
                opened == Some(index),
                reserve_glyphs,
                index,
                items.len(),
                theme,
                cx,
                activate.clone(),
                hover.clone(),
            )
        })
        .collect::<Vec<_>>();

    div()
        .id(surface_ident.element_id())
        .occlude()
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .bg_glass()
        .glass_surface(Surface::Overlay)
        .glass_radius(Radius::Control)
        .glass(|glass| glass.protect_text_contrast(false))
        .when(cfg!(not(target_os = "macos")), |frame| {
            frame.glass_preset(GlassPreset::Frosted)
        })
        .rounded(px(theme.radii.control))
        .border_1()
        .border_color(theme.colors.hairline)
        .min_w(px(theme.measures.compact_menu_min_width))
        .p(px(theme.space(Space::Xxs)))
        .children(rows)
        .into_any_element()
}

fn compact_row(theme: &Theme, highlighted: bool) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme.space(Space::Xs)))
        .px(px(row_padding_x(theme)))
        .h(px(menu_row_height(theme)))
        .radius(theme, Radius::Small)
        .when(highlighted, |element| element.bg(theme.colors.hover))
        .when(!highlighted, |element| {
            element.hover(|style| style.bg(theme.colors.hover))
        })
}

#[allow(clippy::too_many_arguments)]
fn row<V: 'static>(
    ident: &Ident,
    item: &MenuItem,
    path: Vec<usize>,
    parent: SharedString,
    active: bool,
    opened: bool,
    reserve_glyphs: bool,
    index: usize,
    count: usize,
    theme: &Theme,
    cx: &mut Context<V>,
    activate: Activate<V>,
    hover: Hover<V>,
) -> AnyElement {
    let row_ident = ident.child(item.id.as_ref());
    match &item.kind {
        MenuItemKind::Separator => div()
            .h(px(menu_separator_height(theme)))
            .flex()
            .items_center()
            .child(
                div()
                    .w_full()
                    .h(px(theme.borders.hairline))
                    .bg(theme.colors.divider),
            )
            .semantic_in(
                cx,
                NodeSpec::new(row_ident.semantic_id(), Role::Separator).parent(parent),
            )
            .into_any_element(),
        MenuItemKind::Section => div()
            .h(px(menu_heading_height(theme)))
            .px(px(row_padding_x(theme)))
            .py(px(theme.space(Space::Xxs)))
            .child(
                text(theme, TypeScale::Caption, item.label.clone())
                    .text_color(theme.colors.text_muted),
            )
            .semantic_in(
                cx,
                NodeSpec::new(row_ident.semantic_id(), Role::Heading)
                    .parent(parent)
                    .level(2)
                    .text(item.label.clone()),
            )
            .into_any_element(),
        kind => {
            let checked = match kind {
                MenuItemKind::Check(checked) => Some(*checked),
                _ => None,
            };
            let submenu = item.children().is_some();
            let mut spec = NodeSpec::new(row_ident.semantic_id(), Role::MenuItem)
                .parent(parent)
                .text(item.label.clone())
                .hovered(active);
            if let Some(checked) = checked {
                spec = spec.checked(checked);
            }
            if submenu {
                spec = spec.expanded(opened);
            }

            let hover_path = path.clone();
            let row = compact_row(theme, active || opened)
                .id(row_ident.element_id())
                .when(active, |element| element.aria_active_descendant())
                .cursor_pointer()
                .active(|style| style.bg(theme.colors.active))
                .when(reserve_glyphs, |element| {
                    element.child(
                        div()
                            .flex()
                            .flex_none()
                            .w(px(GLYPH_SLOT))
                            .justify_center()
                            .children(checked.filter(|checked| *checked).map(|_| {
                                icon(Icon::Check)
                                    .size(px(theme.control.xs.icon_size))
                                    .text_color(theme.colors.text)
                            })),
                    )
                })
                .child(
                    text(theme, TypeScale::Body, item.label.clone())
                        .flex_1()
                        .text_color(theme.colors.text),
                )
                .children(item.shortcut.clone().map(Kbd::new))
                .when(submenu, |element| {
                    let glyph = if cx.layout_direction().is_rtl() {
                        Icon::AltArrowLeft
                    } else {
                        Icon::AltArrowRight
                    };
                    element.child(
                        icon(glyph)
                            .size(px(theme.control.xs.icon_size))
                            .text_color(theme.colors.text_muted),
                    )
                })
                .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
                    hover(view, hover_path.clone(), *hovered, cx);
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    activate(view, path.clone(), window, cx);
                    cx.stop_propagation();
                }))
                .semantic_in(cx, spec);

            motion::row_in(row_ident.child("in").element_id(), theme, index, count, row)
                .into_any_element()
        }
    }
}

enum Handled {
    Moved,
    Activate(Vec<usize>),
    Close,
    None,
}

fn submenu_intent(key: MenuKey, direction: LayoutDirection) -> Option<bool> {
    match (key, direction) {
        (MenuKey::Right, LayoutDirection::LeftToRight)
        | (MenuKey::Left, LayoutDirection::RightToLeft) => Some(true),
        (MenuKey::Left, LayoutDirection::LeftToRight)
        | (MenuKey::Right, LayoutDirection::RightToLeft) => Some(false),
        _ => None,
    }
}

fn handle_key(
    state: &mut MenuState,
    items: &[MenuItem],
    event: &KeyDownEvent,
    direction: LayoutDirection,
) -> Handled {
    let key = popover::classify_key(
        event.keystroke.key.as_str(),
        event.keystroke.modifiers.platform,
        event.keystroke.modifiers.control,
    );
    if let Some(enter) = submenu_intent(key, direction) {
        let moved = if enter {
            state.enter(items)
        } else {
            state.leave()
        };
        return if moved { Handled::Moved } else { Handled::None };
    }

    match key {
        MenuKey::Down => {
            state.step(items, 1);
            Handled::Moved
        }
        MenuKey::Up => {
            state.step(items, -1);
            Handled::Moved
        }
        MenuKey::Enter => match state.active {
            Some(active) => {
                let mut path = state.path.clone();
                path.push(active);
                Handled::Activate(path)
            }
            None => Handled::Moved,
        },
        MenuKey::Escape => {
            if state.leave() {
                Handled::Moved
            } else {
                Handled::Close
            }
        }
        _ => match popover::typed_letter(event.keystroke.key.as_str(), event.keystroke.modifiers) {
            Some(letter) if state.jump(items, letter) => Handled::Moved,
            _ => Handled::None,
        },
    }
}

pub(crate) struct CompactContextMenu {
    ident: Ident,
    focus_handle: FocusHandle,
    items: Vec<MenuItem>,
    open: bool,
    position: Point<Pixels>,
    pending_focus: bool,
    state: MenuState,
    panel_bounds: PanelBounds,
    hovered_path: Option<Vec<usize>>,
    hover_generation: u64,
    trap: FocusTrap,
}

impl CompactContextMenu {
    pub(super) fn new(
        ident: impl Into<Ident>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            ident: ident.into(),
            focus_handle: cx.focus_handle(),
            items: Vec::new(),
            open: false,
            position: gpui::point(px(0.0), px(0.0)),
            pending_focus: false,
            state: MenuState::default(),
            panel_bounds: PanelBounds::default(),
            hovered_path: None,
            hover_generation: 0,
            trap: FocusTrap::new(),
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn contains_position(&self, position: Point<Pixels>) -> bool {
        self.open
            && self
                .panel_bounds
                .borrow()
                .iter()
                .any(|bounds| bounds.contains(&position))
    }

    pub(super) fn set_items(&mut self, items: Vec<MenuItem>, cx: &mut Context<Self>) {
        self.items = items;
        self.state.reset();
        self.cancel_hover();
        self.panel_bounds.borrow_mut().clear();
        cx.notify();
    }

    pub(super) fn open_at(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.position = position;
        self.state.reset();
        self.cancel_hover();
        self.panel_bounds.borrow_mut().clear();
        self.state.active = first_selectable(&self.items);
        if !self.open {
            self.open = true;
            self.pending_focus = true;
            self.trap.engage(window, cx);
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.pending_focus = false;
        self.state.reset();
        self.cancel_hover();
        self.panel_bounds.borrow_mut().clear();
        self.trap.release(window, cx);
        cx.emit(ContextMenuEvent::Closed);
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        cx.emit(ContextMenuEvent::Dismissed);
        self.close(window, cx);
    }

    fn cancel_hover(&mut self) {
        self.hovered_path = None;
        self.hover_generation = self.hover_generation.wrapping_add(1);
    }

    fn hover_row(&mut self, path: Vec<usize>, hovered: bool, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        if !hovered {
            if self.hovered_path.as_ref() == Some(&path) {
                self.cancel_hover();
            }
            return;
        }
        self.cancel_hover();
        self.hovered_path = Some(path.clone());
        let generation = self.hover_generation;
        cx.spawn(async move |menu, cx| {
            cx.background_executor().timer(SUBMENU_HOVER_DELAY).await;
            menu.update(cx, |menu, cx| {
                if menu.open
                    && menu.hover_generation == generation
                    && menu.hovered_path.as_ref() == Some(&path)
                    && menu.state.hover(&menu.items, &path)
                {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn take(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_hover();
        match self.state.activate(&self.items, &path) {
            Activation::Invoked(id) => {
                cx.emit(ContextMenuEvent::Invoked(id));
                self.close(window, cx);
            }
            Activation::OpenedSubmenu => cx.notify(),
            Activation::Ignored => {}
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.cancel_hover();
        match handle_key(&mut self.state, &self.items, event, cx.layout_direction()) {
            Handled::Moved => {
                cx.notify();
                cx.stop_propagation();
            }
            Handled::Activate(path) => {
                self.take(path, window, cx);
                cx.stop_propagation();
            }
            Handled::Close => {
                self.dismiss(window, cx);
                cx.stop_propagation();
            }
            Handled::None => {}
        }
    }
}

impl Focusable for CompactContextMenu {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CompactContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let menu_id = self.ident.child("menu").semantic_id();

        let overlay = self.open.then(|| {
            if self.pending_focus {
                self.pending_focus = false;
                self.focus_handle.focus(window, cx);
            }
            let activate: Activate<Self> =
                Rc::new(|menu: &mut Self, path, window, cx| menu.take(path, window, cx));
            let hover: Hover<Self> = Rc::new(|menu, path, hovered, cx| {
                menu.hover_row(path, hovered, cx);
            });
            let panels = panels(
                &self.ident,
                &self.items,
                &self.state,
                menu_id.clone(),
                &theme,
                cx,
                activate,
                hover,
            );
            let content = div()
                .child(cascade(
                    panels,
                    self.panel_bounds.clone(),
                    cx.layout_direction(),
                    px(theme.space(Space::Xxs)),
                    px(theme.space(Space::Xs)),
                ))
                .track_focus(&self.focus_handle)
                .key_context("ContextMenu")
                .on_key_down(cx.listener(Self::on_key_down))
                .on_mouse_down_out(
                    cx.listener(|menu, event: &gpui::MouseDownEvent, window, cx| {
                        if !menu.contains_position(event.position) {
                            menu.dismiss(window, cx);
                        }
                    }),
                )
                .semantic_in(
                    cx,
                    NodeSpec::new(menu_id.clone(), Role::Menu)
                        .parent(self.ident.semantic_id())
                        .expanded(true)
                        .focus(&self.focus_handle),
                );

            Overlay::new(self.ident.child("overlay"))
                .placement(Placement::At(self.position))
                .child(content)
                .into_any_element()
        });

        div()
            .id(self.ident.element_id())
            .children(overlay)
            .semantic_in(
                cx,
                NodeSpec::new(self.ident.semantic_id(), Role::Region).expanded(self.open),
            )
    }
}
