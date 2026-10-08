use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    Action, App, AppContext as _, Bounds, Entity, Focusable, Pixels, Point, SharedString,
    Subscription, Window, point,
};

mod cascade;
mod compact_context_menu;

use compact_context_menu::{CompactContextMenu, ContextMenuEvent, MenuItem};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

type ShortcutAction = Box<dyn Action>;

#[derive(Default)]
pub struct MenuBuilder {
    items: Vec<MenuItem>,
    handlers: HashMap<SharedString, Handler>,
    shortcut_actions: HashMap<SharedString, ShortcutAction>,
    next_id: usize,
}

impl MenuBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    fn mint(&mut self) -> SharedString {
        let id = SharedString::from(format!("row-{}", self.next_id));
        self.next_id += 1;
        id
    }

    pub fn label(mut self, text: impl Into<SharedString>) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::section(id, text));
        self
    }

    pub fn separator(mut self) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::separator(id));
        self
    }

    pub fn item(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::command(id.clone(), label));
        self.handlers.insert(id, Rc::new(on_click));
        self
    }

    pub fn item_with_keybinding<A: Action>(
        mut self,
        label: impl Into<SharedString>,
        action: A,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::command(id.clone(), label));
        self.handlers.insert(id.clone(), Rc::new(on_click));
        self.shortcut_actions.insert(id, Box::new(action));
        self
    }

    pub fn check(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::check(id.clone(), label, checked));
        self.handlers.insert(id, Rc::new(on_click));
        self
    }

    pub fn check_with_keybinding<A: Action>(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        action: A,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let id = self.mint();
        self.items.push(MenuItem::check(id.clone(), label, checked));
        self.handlers.insert(id.clone(), Rc::new(on_click));
        self.shortcut_actions.insert(id, Box::new(action));
        self
    }

    pub fn submenu(
        mut self,
        label: impl Into<SharedString>,
        build: impl FnOnce(Self) -> Self,
    ) -> Self {
        let id = self.mint();
        let nested = build(Self {
            items: Vec::new(),
            handlers: HashMap::new(),
            shortcut_actions: HashMap::new(),
            next_id: self.next_id,
        });
        self.next_id = nested.next_id;
        self.handlers.extend(nested.handlers);
        self.shortcut_actions.extend(nested.shortcut_actions);
        self.items.push(MenuItem::submenu(id, label, nested.items));
        self
    }

    pub fn when(self, condition: bool, build: impl FnOnce(Self) -> Self) -> Self {
        if condition { build(self) } else { self }
    }

    pub fn when_some<T>(self, value: Option<T>, build: impl FnOnce(Self, T) -> Self) -> Self {
        match value {
            Some(value) => build(self, value),
            None => self,
        }
    }

    fn into_parts(
        self,
    ) -> (
        Vec<MenuItem>,
        HashMap<SharedString, Handler>,
        HashMap<SharedString, ShortcutAction>,
    ) {
        (self.items, self.handlers, self.shortcut_actions)
    }
}

pub struct ContextMenuHost {
    menu: Entity<CompactContextMenu>,
    _subscription: Subscription,
}

struct MenuCenter {
    menu: Entity<CompactContextMenu>,
    handlers: Rc<RefCell<HashMap<SharedString, Handler>>>,
}

impl gpui::Global for MenuCenter {}

impl ContextMenuHost {
    pub fn new<V: 'static>(
        ident: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut gpui::Context<V>,
    ) -> Self {
        let ident = ident.into();
        let menu = cx.new(|cx| CompactContextMenu::new(ident.clone(), window, cx));
        let handlers: Rc<RefCell<HashMap<SharedString, Handler>>> = Rc::default();

        let routed = Rc::clone(&handlers);
        let subscription = cx.subscribe_in(
            &menu,
            window,
            move |_view, _menu, event: &ContextMenuEvent, window, cx| match event {
                ContextMenuEvent::Invoked(id) => {
                    let handler = routed.borrow().get(id).cloned();
                    if let Some(handler) = handler {
                        handler(window, cx);
                    }
                }
                ContextMenuEvent::Closed => routed.borrow_mut().clear(),
                _ => {}
            },
        );

        Self {
            menu: menu.clone(),
            _subscription: subscription,
        }
        .registered(menu, handlers, cx)
    }

    fn registered(
        self,
        menu: Entity<CompactContextMenu>,
        handlers: Rc<RefCell<HashMap<SharedString, Handler>>>,
        cx: &mut App,
    ) -> Self {
        cx.set_global(MenuCenter { menu, handlers });
        self
    }

    pub(crate) fn entity(&self) -> &Entity<CompactContextMenu> {
        &self.menu
    }
}

fn resolve_shortcuts(
    items: &mut [MenuItem],
    shortcut_actions: &HashMap<SharedString, ShortcutAction>,
    window: &Window,
) {
    for item in items {
        let shortcut = shortcut_actions.get(item.id()).and_then(|action| {
            window
                .highest_precedence_binding_for_action(action.as_ref())
                .map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|shortcut| !shortcut.is_empty())
                .map(SharedString::from)
        });
        if let Some(shortcut) = shortcut {
            item.set_shortcut(shortcut);
        }
        if let Some(children) = item.children_mut() {
            resolve_shortcuts(children, shortcut_actions, window);
        }
    }
}

fn open_with(
    menu: &Entity<CompactContextMenu>,
    handlers: &Rc<RefCell<HashMap<SharedString, Handler>>>,
    builder: MenuBuilder,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if builder.is_empty() {
        return;
    }
    let (mut items, built, shortcut_actions) = builder.into_parts();
    resolve_shortcuts(&mut items, &shortcut_actions, window);
    *handlers.borrow_mut() = built;
    menu.update(cx, |menu, cx| {
        menu.set_items(items, cx);
        menu.open_at(position, window, cx);
    });
}

pub(crate) fn context_menu_has_focus(window: &Window, cx: &App) -> bool {
    cx.try_global::<MenuCenter>().is_some_and(|center| {
        let menu = center.menu.read(cx);
        menu.is_open() && menu.focus_handle(cx).contains_focused(window, cx)
    })
}

pub(crate) fn context_menu_contains_position(position: Point<Pixels>, cx: &App) -> bool {
    cx.try_global::<MenuCenter>()
        .is_some_and(|center| center.menu.read(cx).contains_position(position))
}

pub fn open_context_menu(
    builder: MenuBuilder,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some((menu, handlers)) = cx
        .try_global::<MenuCenter>()
        .map(|center| (center.menu.clone(), Rc::clone(&center.handlers)))
    else {
        return false;
    };
    open_with(&menu, &handlers, builder, position, window, cx);
    true
}

pub fn open_context_menu_at_bounds(
    builder: MenuBuilder,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    open_context_menu(builder, context_menu_anchor(bounds), window, cx)
}

fn context_menu_anchor(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.left(), bounds.bottom())
}
