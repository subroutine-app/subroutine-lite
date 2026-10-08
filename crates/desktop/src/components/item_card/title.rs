use gpui::{
    App, ClickEvent, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_kit::{
    controls::input::{Cancel as InputCancel, Submit as InputSubmit},
    foundation::StyledExt as _,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::AnyItem;

use crate::{
    components::{
        Label,
        ext::{ElementExt, InteractiveElementExt},
    },
    item_manager::ItemManager,
    item_subject::ItemSubject,
};

use super::neutral_card_colors;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::components) enum ItemCardTitleScale {
    Standard,
    Large,
    Display,
}

impl ItemCardTitleScale {
    pub(super) fn line_height(self) -> Pixels {
        match self {
            Self::Standard => px(20.),
            Self::Large => px(26.),
            Self::Display => px(32.),
        }
    }

    pub(super) fn leading_extent(self) -> Pixels {
        match self {
            Self::Standard => px(20.),
            Self::Large => px(24.),
            Self::Display => px(32.),
        }
    }

    pub(super) fn header_gap(self) -> Pixels {
        match self {
            Self::Standard => px(6.),
            Self::Large => px(8.),
            Self::Display => px(10.),
        }
    }

    fn apply(self, element: Div) -> Div {
        let element = match self {
            Self::Standard => element.text_sm(),
            Self::Large => element.text_lg(),
            Self::Display => element.text_2xl().font_weight(FontWeight::MEDIUM),
        };
        element
            .line_height(self.line_height())
            .h(self.line_height())
    }
}

pub(in crate::components) fn render_dynamic_title(
    item: &AnyItem,
    compact: bool,
    editable: bool,
    title_scale: ItemCardTitleScale,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    render_subject_dynamic_title(
        ItemSubject::Live(item.clone()),
        compact,
        editable,
        title_scale,
        window,
        cx,
    )
}

fn render_subject_dynamic_title(
    subject: ItemSubject,
    compact: bool,
    editable: bool,
    title_scale: ItemCardTitleScale,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let cloned = subject.clone();
    let item_id = subject.id();
    let subject_key = subject.key();
    let title = SharedString::from(subject.title().to_owned());
    let manager = ItemManager::global(cx);
    let session = editable
        .then(|| manager.read(cx).editing_session(subject_key))
        .flatten();
    let has_type_picker = manager.read(cx).draft_type(item_id).is_some();
    let colors = neutral_card_colors(cx.theme());
    let recognition = crate::presentation::RecognitionPaint::new(cx.theme());

    let begin = manager.clone();
    let commit = manager.clone();
    let cancel = manager.clone();

    if let Some((_, _, _, Some(revision))) = session.as_ref() {
        let manager = manager.clone();
        let revision = *revision;
        window.on_next_frame(move |_, cx| {
            manager.update(cx, |manager, cx| {
                manager.acknowledge_highlight_layout(subject_key, revision, cx);
            });
        });
        window.request_animation_frame();
    }

    let washes: Vec<Div> = match session.as_ref() {
        Some((input, spans, bounds, _)) => spans
            .iter()
            .filter_map(|span| {
                crate::components::text_input::range_bounds(input, span, bounds.size, window, cx)
            })
            .map(|bounds| {
                let pad = px(3.);
                div()
                    .absolute()
                    .top(bounds.origin.y)
                    .left(bounds.origin.x - pad)
                    .h(bounds.size.height)
                    .w(bounds.size.width + pad * 1.5)
                    .rounded_md()
                    .bg(recognition.fill)
                    .border_1()
                    .border_color(recognition.border)
            })
            .collect(),
        None => Vec::new(),
    };

    div()
        .row()
        .id(("item-dynamic-title", item_id.as_u64_pair().1))
        .w_full()
        .px_1()
        .items_center()
        .when_else(
            session.is_some(),
            move |this| {
                let Some((editor, _, _, _)) = session.clone() else {
                    return this;
                };
                let commit_outside = commit.clone();
                let commit_key = commit.clone();
                let report = manager.clone();
                this.child(
                    div()
                        .row()
                        .relative()
                        .w_full()
                        .items_center()
                        .map(|this| title_scale.apply(this))
                        .text_color(colors.fg)
                        .when(!has_type_picker, |this| {
                            this.on_mouse_down_out(move |_, window, cx| {
                                commit_outside.update(cx, |manager, cx| {
                                    manager.commit_open_edit(window, cx);
                                });
                            })
                        })
                        .capture_action::<InputSubmit>(move |_, window, cx| {
                            cx.stop_propagation();
                            commit_key.update(cx, |manager, cx| {
                                manager.submit_open_edit(window, cx);
                            });
                        })
                        .capture_action::<InputCancel>({
                            let cancel = cancel.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                cancel.update(cx, |manager, cx| {
                                    manager.discard_edit(window, cx);
                                });
                            }
                        })
                        .on_prepaint(move |bounds, _, cx| {
                            report.update(cx, |manager, _| {
                                manager.report_input_bounds(subject_key, bounds);
                            });
                        })
                        .children(washes)
                        .child(editor),
                )
            },
            move |this| {
                this.when(editable, |this| {
                    this.on_double_click(move |_: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        begin.update(cx, |this, cx| match &cloned {
                            ItemSubject::Live(item) => this.begin_edit(item, false, window, cx),
                            ItemSubject::Saved(item) => this.begin_saved_edit(item, window, cx),
                        })
                    })
                })
                .child(
                    title_scale
                        .apply(Label::new(title).min_w_0().flex_1())
                        .text_color(colors.fg)
                        .truncate()
                        .m_0()
                        .p_0()
                        .when(
                            !compact && title_scale != ItemCardTitleScale::Display,
                            |this| this.font_weight(FontWeight::MEDIUM),
                        )
                        .cursor_default(),
                )
            },
        )
}
