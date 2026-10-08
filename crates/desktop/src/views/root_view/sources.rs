use super::{
    RootView, TOP_EDGE_INSET,
    navigation::WorkspaceRoute,
    panels::{WORKSPACE_PANEL_BORDER_WIDTH, WORKSPACE_PANEL_GAP},
};
use crate::{
    AppIcon,
    components::{
        Button, ButtonVariants as _, Label,
        menu::{MenuBuilder, open_context_menu},
    },
    selection::{SelectionManager, SelectionScope},
    views::{
        LIBRARY_HEADER_HEIGHT, LIST_VIEW_MAX_WIDTH, LIST_VIEW_MIN_WIDTH, SEARCH_HEADER_HEIGHT,
        SavedItemsFilter, SourceKind, SourceSort,
    },
};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_kit::{
    controls::search::{SearchInput, SearchInputEvent},
    foundation::{Sizable as _, StyledExt as _},
    layout::{ScrollEdgeEffect, ScrollFade},
    overlay::{GlassExt as _, GlassPreset},
};
use gpui_kit_theme::{ActiveTheme, Elevation, Surface};

fn library_drawer_top_padding() -> gpui::Pixels {
    if cfg!(target_os = "windows") {
        px(16.)
    } else {
        TOP_EDGE_INSET - WORKSPACE_PANEL_GAP - WORKSPACE_PANEL_BORDER_WIDTH
    }
}

fn library_drawer_header_height(searching: bool) -> gpui::Pixels {
    library_drawer_top_padding() + px(if searching { 160. } else { 40. })
}

#[derive(Clone)]
struct SourceFiltersChanged(Vec<SourceKind>);

#[derive(Clone, Copy)]
struct SourceSortChanged(SourceSort);

pub(super) struct SourceFilterPicker {
    selected: Vec<SourceKind>,
}

impl SourceFilterPicker {
    pub(super) fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            selected: Vec::new(),
        }
    }

    fn set_selected(&mut self, selected: Vec<SourceKind>, cx: &mut Context<Self>) {
        if self.selected == selected {
            return;
        }
        self.selected = selected;
        cx.notify();
    }

    fn toggle(&mut self, kind: SourceKind, cx: &mut Context<Self>) {
        let mut selected = self.selected.clone();
        if let Some(index) = selected.iter().position(|candidate| *candidate == kind) {
            selected.remove(index);
        } else {
            selected.push(kind);
            selected.sort_by_key(source_kind_rank);
        }
        self.set_selected(selected, cx);
        cx.emit(SourceFiltersChanged(self.selected.clone()));
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        self.set_selected(Vec::new(), cx);
        cx.emit(SourceFiltersChanged(Vec::new()));
    }
}

impl EventEmitter<SourceFiltersChanged> for SourceFilterPicker {}

impl Render for SourceFilterPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picker = cx.entity().downgrade();
        Button::new("source-sidebar.filters")
            .ghost()
            .small()
            .icon(AppIcon::Filter)
            .label(source_filter_summary(&self.selected))
            .on_click(move |event: &ClickEvent, window, cx| {
                open_context_menu(
                    source_filter_menu(&picker, cx),
                    event.position(),
                    window,
                    cx,
                );
            })
    }
}

pub(super) struct SourceSortPicker {
    selected: SourceSort,
}

impl SourceSortPicker {
    pub(super) fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            selected: SourceSort::default(),
        }
    }

    fn select(&mut self, sort: SourceSort, cx: &mut Context<Self>) {
        if self.selected == sort {
            return;
        }
        self.selected = sort;
        cx.emit(SourceSortChanged(sort));
        cx.notify();
    }
}

impl EventEmitter<SourceSortChanged> for SourceSortPicker {}

impl Render for SourceSortPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picker = cx.entity().downgrade();
        Button::new("source-sidebar.sort")
            .ghost()
            .small()
            .icon(AppIcon::SortVertical)
            .tooltip(format!("Sort: {}", self.selected.label()))
            .on_click(move |event: &ClickEvent, window, cx| {
                open_context_menu(source_sort_menu(&picker, cx), event.position(), window, cx);
            })
    }
}

fn source_kind_rank(kind: &SourceKind) -> usize {
    SourceKind::ALL
        .iter()
        .position(|candidate| candidate == kind)
        .unwrap_or(SourceKind::ALL.len())
}

fn source_filter_summary(selected: &[SourceKind]) -> String {
    match selected {
        [] => "All except completed".to_owned(),
        [kind] => kind.label().to_owned(),
        kinds => format!("{} filters", kinds.len()),
    }
}

fn source_sort_menu(picker: &gpui::WeakEntity<SourceSortPicker>, cx: &App) -> MenuBuilder {
    let Some(entity) = picker.upgrade() else {
        return MenuBuilder::new();
    };
    let selected = entity.read(cx).selected;

    SourceSort::ALL
        .into_iter()
        .fold(MenuBuilder::new(), |menu, sort| {
            let option_picker = picker.clone();
            menu.check(sort.label(), selected == sort, move |_, cx| {
                option_picker
                    .update(cx, |picker, cx| picker.select(sort, cx))
                    .ok();
            })
        })
}

fn source_filter_menu(picker: &gpui::WeakEntity<SourceFilterPicker>, cx: &App) -> MenuBuilder {
    let Some(entity) = picker.upgrade() else {
        return MenuBuilder::new();
    };
    let selected = entity.read(cx).selected.clone();
    let default_picker = picker.clone();
    let mut menu =
        MenuBuilder::new().check("All except completed", selected.is_empty(), move |_, cx| {
            default_picker
                .update(cx, |picker, cx| picker.clear(cx))
                .ok();
        });

    for group in ["Smart lists", "Live", "Saved"] {
        menu = menu.separator().label(group);
        for kind in SourceKind::ALL
            .into_iter()
            .filter(|kind| kind.group() == group)
        {
            let option_picker = picker.clone();
            menu = menu.check(kind.label(), selected.contains(&kind), move |_, cx| {
                option_picker
                    .update(cx, |picker, cx| picker.toggle(kind, cx))
                    .ok();
            });
        }
    }

    menu
}

impl RootView {
    pub(super) fn subscribe_source_controls(
        search: &Entity<SearchInput>,
        filters: &Entity<SourceFilterPicker>,
        sort: &Entity<SourceSortPicker>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            search,
            window,
            |view, _, event: &SearchInputEvent, window, cx| match event {
                SearchInputEvent::Change(query) => {
                    let query = query.as_ref();
                    view.search_view
                        .update(cx, |search, cx| search.set_query(query, cx));
                    view.unqueued_view
                        .update(cx, |unqueued, cx| unqueued.set_query(query, cx));
                    view.routines_view
                        .update(cx, |routines, cx| routines.set_query(query, cx));
                    view.saved_items_view
                        .update(cx, |saved, cx| saved.set_query(query, cx));
                    cx.notify();
                }
                SearchInputEvent::Submit
                    if view.route == WorkspaceRoute::Search
                        || (view.library_drawer_open
                            && view.library_drawer_route == WorkspaceRoute::Search) =>
                {
                    view.search_view
                        .update(cx, |search, cx| search.focus_first_result(window, cx));
                }
                SearchInputEvent::Cancel => {
                    let selection = SelectionManager::global(cx);
                    if selection.read(cx).has_selection_in(SelectionScope::Search) {
                        SelectionManager::clear_global(cx);
                    }
                    view.search_view.read(cx).focus_handle().focus(window, cx);
                }
                _ => {}
            },
        )
        .detach();

        cx.subscribe(filters, |view, _, event: &SourceFiltersChanged, cx| {
            view.set_source_kinds(event.0.clone(), cx)
        })
        .detach();

        cx.subscribe(sort, |view, _, event: &SourceSortChanged, cx| {
            view.search_view
                .update(cx, |search, cx| search.set_sort(event.0, cx));
        })
        .detach();
    }

    pub(super) fn clear_source_query(&self, cx: &mut Context<Self>) {
        self.source_search
            .update(cx, |search, cx| search.set_value("", cx));
    }

    pub(super) fn set_source_kinds(&mut self, kinds: Vec<SourceKind>, cx: &mut Context<Self>) {
        if self.source_kinds == kinds {
            return;
        }
        self.source_kinds = kinds;
        let selected = self.source_kinds.clone();
        self.source_filter_picker
            .update(cx, |picker, cx| picker.set_selected(selected, cx));
        let search_kinds = self.source_kinds.clone();
        self.search_view
            .update(cx, |search, cx| search.set_filters(search_kinds, cx));
        if let [kind] = self.source_kinds.as_slice()
            && let Some(filter) = match kind {
                SourceKind::SavedAction => Some(SavedItemsFilter::Actions),
                SourceKind::SavedEvent => Some(SavedItemsFilter::Events),
                _ => None,
            }
        {
            self.saved_items_view
                .update(cx, |saved, cx| saved.set_filter(filter, cx));
        }
        SelectionManager::clear_global(cx);
        cx.notify();
    }

    pub(crate) fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_overlay.is_some() {
            return;
        }
        self.toggle_library_drawer(WorkspaceRoute::Search, window, cx);
    }

    pub(super) fn render_source_header(
        &mut self,
        route: WorkspaceRoute,
        in_drawer: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match route {
            WorkspaceRoute::Home => "Home",
            WorkspaceRoute::Search => "Search",
            WorkspaceRoute::Unqueued => "Unqueued",
            WorkspaceRoute::Routines => "Routines",
            WorkspaceRoute::SavedItems => "Saved items",
            WorkspaceRoute::Main => "",
        };

        let searching = route == WorkspaceRoute::Search;
        let full_header_height = if searching {
            SEARCH_HEADER_HEIGHT
        } else {
            LIBRARY_HEADER_HEIGHT
        };
        let header_height = if in_drawer {
            library_drawer_header_height(searching)
        } else {
            full_header_height
        };
        let route_ident = match route {
            WorkspaceRoute::Home => "home",
            WorkspaceRoute::Search => "search",
            WorkspaceRoute::Unqueued => "unqueued",
            WorkspaceRoute::Routines => "routines",
            WorkspaceRoute::SavedItems => "saved-items",
            WorkspaceRoute::Main => "main",
        };
        let root = cx.entity().clone();
        let title_row = div()
            .row()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                Label::new(title)
                    .when_else(in_drawer, |title| title.text_lg(), |title| title.text_2xl())
                    .font_weight(gpui::FontWeight::SEMIBOLD),
            )
            .when(in_drawer, |row| {
                let expand_root = root.clone();
                let close_root = root.clone();
                row.child(
                    div()
                        .row()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new(format!("library-drawer.{route_ident}.expand"))
                                .ghost()
                                .compact()
                                .size_7()
                                .icon(AppIcon::ArrowRight)
                                .tooltip("Open full view")
                                .on_click(move |_, window, cx| {
                                    expand_root.update(cx, |root, cx| {
                                        root.open_source_full_view(route, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new(format!("library-drawer.{route_ident}.close"))
                                .ghost()
                                .compact()
                                .size_7()
                                .icon(AppIcon::Close)
                                .tooltip("Close library drawer")
                                .on_click(move |_, window, cx| {
                                    close_root.update(cx, |root, cx| {
                                        root.close_library_drawer(window, cx)
                                    });
                                }),
                        ),
                )
            });
        let header = div()
            .column()
            .w_full()
            .h(header_height)
            .max_w(if searching {
                px(760.)
            } else {
                LIST_VIEW_MAX_WIDTH + px(24.)
            })
            .mx_auto()
            .pt(if in_drawer {
                library_drawer_top_padding()
            } else {
                TOP_EDGE_INSET
            })
            .gap_3()
            .when_else(
                in_drawer,
                |header| header.px_4().pb_2(),
                |header| header.px_6().pb_4(),
            )
            .child(title_row)
            .when(searching, |header| {
                header.child(self.source_search.clone()).child(
                    div()
                        .row()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(self.source_filter_picker.clone())
                        .child(self.source_sort_picker.clone()),
                )
            });
        let header_frame = div()
            .id(format!("{route_ident}.header"))
            .size_full()
            .when(in_drawer, |frame| {
                frame
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(cx.theme().colors.hairline)
            })
            .child(header);
        if cfg!(any(target_os = "macos", target_os = "windows")) && !in_drawer {
            return header_frame.into_any_element();
        }
        header_frame
            .bg_glass()
            .glass_surface(Surface::Canvas)
            .glass(|glass| glass.protect_text_contrast(false))
            .glass_radius_px(0.0)
            .when(cfg!(not(target_os = "macos")), |frame| {
                frame.glass_preset(GlassPreset::Frosted)
            })
            .when(in_drawer, |frame| {
                frame.glass(|glass| {
                    glass
                        .surface(Surface::Overlay)
                        .preset(GlassPreset::Lens)
                        .refraction(0.0)
                        .blur(cx.theme().effects.glass_frost_blur)
                        .elevation(Elevation::Flat)
                })
            })
            .into_any_element()
    }

    pub(super) fn render_source_route(
        &mut self,
        route: WorkspaceRoute,
        in_drawer: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let searching = route == WorkspaceRoute::Search;
        let full_header_height = if searching {
            SEARCH_HEADER_HEIGHT
        } else {
            LIBRARY_HEADER_HEIGHT
        };
        let header_height = if in_drawer {
            library_drawer_header_height(searching)
        } else {
            full_header_height
        };
        let route_ident = route.id();
        let route_body = match route {
            WorkspaceRoute::Home => self.home_view.clone().into_any_element(),
            WorkspaceRoute::Search => self.search_view.clone().into_any_element(),
            WorkspaceRoute::Unqueued => self.unqueued_view.clone().into_any_element(),
            WorkspaceRoute::Routines => self.routines_view.clone().into_any_element(),
            WorkspaceRoute::SavedItems => self.saved_items_view.clone().into_any_element(),
            WorkspaceRoute::Main => div().into_any_element(),
        };
        let local_header = (in_drawer
            || cfg!(not(any(target_os = "macos", target_os = "windows"))))
        .then(|| self.render_source_header(route, in_drawer, cx));
        let edge_band = f32::from(header_height);

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                ScrollEdgeEffect::new(format!("{route_ident}.header-edge"))
                    .top(true)
                    .soft()
                    .band(edge_band)
                    .when(cfg!(target_os = "windows") && !in_drawer, |edge| {
                        edge.blur(0.)
                    })
                    .child(
                        ScrollFade::new(format!("{route_ident}.header-fade"))
                            .top(true)
                            .band(edge_band)
                            .text_only()
                            .child(
                                div().size_full().overflow_hidden().child(
                                    div()
                                        .absolute()
                                        .top(header_height - full_header_height)
                                        .bottom_0()
                                        .left_0()
                                        .right_0()
                                        .max_w(if searching {
                                            px(840.)
                                        } else {
                                            LIST_VIEW_MAX_WIDTH
                                        })
                                        .when(!searching, |body| body.min_w(LIST_VIEW_MIN_WIDTH))
                                        .mx_auto()
                                        .child(route_body),
                                ),
                            ),
                    ),
            )
            .when_some(local_header, |view, header| {
                view.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(header_height)
                        .block_mouse_except_scroll()
                        .child(header),
                )
            })
    }
}
