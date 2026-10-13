use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_kit::{
    controls::select::{Select, SelectEvent, SelectOption},
    foundation::{Disableable as _, StyledExt as _},
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_holidays::{END_YEAR, REGIONS, Region, START_YEAR};

use crate::{
    components::{Button, ButtonVariants as _},
    stores::AppDatabaseStore,
};

pub(super) struct HolidaySetup {
    focus_handle: FocusHandle,
    region_select: Entity<Select>,
    region: Option<&'static Region>,
    error: Option<String>,
}

impl HolidaySetup {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let region = sys_locale::get_locale()
            .as_deref()
            .and_then(suggested_region);
        let region_select = cx.new(|cx| {
            let select = Select::new("holidays.region", window, cx)
                .name("Holiday region")
                .placeholder("Choose a region")
                .options(
                    REGIONS
                        .iter()
                        .map(|region| SelectOption::new(region.id(), region.name())),
                );
            match region {
                Some(region) => select.selected(region.id()),
                None => select,
            }
        });
        cx.subscribe(&region_select, |view, select, event: &SelectEvent, cx| {
            if let SelectEvent::Selected(id) = event {
                select.update(cx, |select, cx| select.set_selected(Some(id.clone()), cx));
                view.region = subroutine_holidays::region(id);
                view.error = None;
                cx.notify();
            }
        })
        .detach();
        Self {
            focus_handle: cx.focus_handle(),
            region_select,
            region,
            error: None,
        }
    }

    fn finish(&mut self, region: Option<&Region>, cx: &mut Context<Self>) {
        let result = AppDatabaseStore::global(cx)
            .update(cx, |store, cx| store.complete_holiday_setup(region, cx));
        match result {
            Ok(()) => {
                self.error = None;
                cx.emit(());
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
}

impl EventEmitter<()> for HolidaySetup {}

impl Focusable for HolidaySetup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HolidaySetup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tooltip = self.region.map_or_else(
            || "Choose a holiday region".to_owned(),
            |region| {
                format!(
                    "Add {} editable markers for {}. Dates may be outdated; no automatic updates.",
                    subroutine_holidays::holidays(region).len(),
                    region.name(),
                )
            },
        );
        div()
            .id("holidays.setup")
            .track_focus(&self.focus_handle)
            .semantic_in(
                cx,
                NodeSpec::new("holidays.setup", Role::Region).text("Regional holidays"),
            )
            .column()
            .w_full()
            .flex_none()
            .px_4()
            .py_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().colors.hairline)
            .child(
                div()
                    .row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().colors.text_muted)
                            .child(format!("Public holidays · {START_YEAR}–{END_YEAR}")),
                    )
                    .child(
                        div()
                            .w(px(300.))
                            .max_w_full()
                            .child(self.region_select.clone()),
                    )
                    .child(
                        Button::new("holidays.add")
                            .primary()
                            .label("Add holidays")
                            .disabled(self.region.is_none())
                            .tooltip(tooltip)
                            .on_click(cx.listener(|view, _, _, cx| {
                                if let Some(region) = view.region {
                                    view.finish(Some(region), cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("holidays.skip")
                            .ghost()
                            .label("Skip")
                            .on_click(cx.listener(|view, _, _, cx| view.finish(None, cx))),
                    ),
            )
            .when_some(self.error.as_ref(), |view, error| {
                view.child(
                    div()
                        .text_sm()
                        .semantic_in(
                            cx,
                            NodeSpec::new("holidays.error", Role::Status).text(error.clone()),
                        )
                        .child(error.clone()),
                )
            })
    }
}

fn suggested_region(locale: &str) -> Option<&'static Region> {
    let locale = locale.split(['.', '@']).next()?;
    let mut parts = locale.split(['-', '_']);
    parts.next()?;
    let region = parts.next()?;
    let region = if region.len() == 4 && region.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        parts.next()?
    } else {
        region
    };
    if region.len() != 2 || !region.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    subroutine_holidays::region(&region.to_ascii_uppercase())
}
