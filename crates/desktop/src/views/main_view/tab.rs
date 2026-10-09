use gpui::{App, Context, Focusable, InteractiveElement, KeyContext, Render, Window, actions};

use crate::{
    keys::key,
    selection::{self, Dismiss, SelectionScope},
    stores::AppDatabaseStore,
};

pub const COMMAND_KEY_CONTEXT: &str = "MainView && !ContextMenu";

actions!(main_view, [NextTab, PreviousTab, RefreshPipeline, GoToNow,]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        key("cmd-r", RefreshPipeline, Some(COMMAND_KEY_CONTEXT)),
        key("cmd-)", GoToNow, Some(COMMAND_KEY_CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectedMainView {
    #[default]
    Timeline,
    Calendar,
    Queue,
    Focus,
}

impl SelectedMainView {
    pub const ALL: [Self; 4] = [Self::Queue, Self::Timeline, Self::Calendar, Self::Focus];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Timeline => "timeline",
            Self::Calendar => "calendar",
            Self::Queue => "queue",
            Self::Focus => "focus",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::Calendar => "Calendar",
            Self::Queue => "Queue",
            Self::Focus => "Focus",
        }
    }

    pub(super) fn context_name(self) -> &'static str {
        match self {
            Self::Timeline => "TimelineView",
            Self::Calendar => "CalendarView",
            Self::Queue => "QueueView",
            Self::Focus => "FocusView",
        }
    }

    fn command_context(self) -> KeyContext {
        let mut context = KeyContext::default();
        context.add("MainView");
        context.set("active_view", self.id());
        context
    }

    fn key_context(self) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(self.context_name());
        context.add(selection::VIEW_KEY_CONTEXT);
        context
    }
}

pub trait MainViewTab: Render + Focusable + Sized {
    const TAB: SelectedMainView;

    fn scope() -> Option<SelectionScope> {
        None
    }

    fn go_to_now(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        false
    }

    fn bind_view_actions<E: InteractiveElement>(&self, element: E, _cx: &mut Context<Self>) -> E {
        element
    }

    fn view_command_scope<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        let element = element
            .key_context(Self::TAB.command_context())
            .on_action(cx.listener(|this, _: &GoToNow, window, cx| this.go_to_now(window, cx)))
            .on_action(|_: &RefreshPipeline, _, cx| {
                let _ =
                    AppDatabaseStore::global(cx).update(cx, |store, cx| store.refresh_pipeline(cx));
            });
        self.bind_view_actions(element, cx)
    }

    fn tab_root<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        let home = self.focus_handle(cx);
        element
            .track_focus(&home)
            .key_context(Self::TAB.key_context())
            .on_action(cx.listener(move |this, _: &Dismiss, window, cx| {
                if this.dismissed(window, cx) {
                    return;
                }
                if !selection::dismiss_view(Self::scope(), Some(&home), window, cx) {
                    cx.propagate();
                }
            }))
    }
}
