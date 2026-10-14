use gpui::{
    App, AppContext as _, Context, Entity, ParentElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
#[cfg(feature = "private-workiq")]
use gpui_kit::{
    controls::settings_row::SettingsList,
    navigation::accordion::{Accordion, AccordionSection},
};
use gpui_kit::{
    controls::{
        auth::{PasswordInput, PasswordInputEvent},
        settings_row::{SettingsRow, SettingsSection},
        toggle::Switch,
    },
    foundation::{Disableable as _, Sizable as _, StyledExt as _},
};
use gpui_kit_theme::ActiveTheme as _;

use crate::{
    components::{Button, Label},
    imports::{
        status::{IntegrationKind, IntegrationState, IntegrationStatuses},
        sync,
    },
    settings::Settings,
};

#[path = "import_settings_options.rs"]
mod options;
#[path = "import_settings_state.rs"]
mod state;

use options::{Channel, WindowEdge, WindowSelect};
use state::{Connection, KINDS, Operation, ProviderState};

pub(super) struct ImportSettings {
    windows: Vec<WindowSelect>,
    todoist_token: Entity<PasswordInput>,
    #[cfg(feature = "private-workiq")]
    advanced_open: bool,
}

impl ImportSettings {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let windows = KINDS
            .iter()
            .flat_map(|kind| {
                let edges: &[WindowEdge] = match kind {
                    IntegrationKind::Todoist => &[],
                    #[cfg(target_os = "macos")]
                    IntegrationKind::EventKit => &[WindowEdge::Future],
                    _ => &[WindowEdge::Past, WindowEdge::Future],
                };
                edges.iter().map(move |edge| (*kind, *edge))
            })
            .map(|(kind, edge)| WindowSelect::new(kind, edge, window, cx))
            .collect();
        let todoist_token = cx.new(|cx| {
            PasswordInput::new("settings.import.todoist.token", window, cx)
                .large()
                .name("Todoist API token")
                .placeholder("Paste your API token")
        });
        cx.subscribe(&todoist_token, |panel, _, event, cx| match event {
            PasswordInputEvent::Submit => panel.connect_todoist(cx),
            PasswordInputEvent::Change(_) => cx.notify(),
            _ => {}
        })
        .detach();
        Self {
            windows,
            todoist_token,
            #[cfg(feature = "private-workiq")]
            advanced_open: false,
        }
    }

    pub(super) fn activate(&mut self, cx: &mut Context<Self>) {
        for kind in KINDS {
            state::check(*kind, cx);
        }
    }

    fn connect_todoist(&mut self, cx: &mut Context<Self>) {
        let token = self.todoist_token.read(cx).value(cx).to_string();
        if token.trim().is_empty() {
            return;
        }
        let Some(task) = state::connect(IntegrationKind::Todoist, Some(token), cx) else {
            return;
        };
        self.todoist_token
            .update(cx, |input, cx| input.set_disabled(true, cx));
        cx.spawn(async move |panel, cx| {
            let success = task.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.todoist_token.update(cx, |input, cx| {
                    if success {
                        input.set_value("", cx);
                    }
                    input.set_disabled(false, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn sections(
        &mut self,
        searching: bool,
        cx: &mut Context<Self>,
    ) -> Vec<SettingsSection> {
        for &kind in KINDS {
            let connection = state::get(cx, kind);
            if state::credential(kind).is_some()
                && connection.connection == Connection::Unknown
                && connection.operation.is_none()
            {
                cx.defer(move |cx| state::check(kind, cx));
            }
        }
        let can_import = can_import_now(cx);
        let overview = SettingsSection::new("settings.import.overview", "Import").row(
            SettingsRow::new("settings.import.now.row", "Import from connected sources")
                .description("No Subroutine sign-in required. Source data is read-only.")
                .search_terms([
                    "Google",
                    "Outlook",
                    "Microsoft 365",
                    "Todoist",
                    "Apple",
                    "Calendar",
                    "Reminders",
                ])
                .control(
                    Button::new("settings.import.now")
                        .large()
                        .label("Import now")
                        .disabled(!can_import)
                        .when(can_import, |button| {
                            button.on_click(|_, _, cx| {
                                if can_import_now(cx) {
                                    sync::sync_now(cx);
                                }
                            })
                        }),
                ),
        );
        let mut sections = vec![overview];
        for kind in KINDS {
            if !searching && kind.provider_id() == "workiq" {
                continue;
            }
            sections.push(self.provider_section(*kind, cx));
        }
        sections
    }

    #[cfg(feature = "private-workiq")]
    pub(super) fn advanced(
        &mut self,
        label_width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Accordion {
        let section = self
            .provider_section(IntegrationKind::WorkIq, cx)
            .label_width(label_width);
        let panel = cx.entity().downgrade();
        Accordion::new("settings.import.accordion")
            .large()
            .section(
                AccordionSection::new("advanced", "Advanced")
                    .body(SettingsList::new("settings.import.advanced.list").section(section)),
            )
            .expanded(self.advanced_open.then(|| "advanced".into()))
            .on_toggle(move |_, open, _, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.advanced_open = open;
                    cx.notify();
                });
            })
    }

    fn provider_section(
        &mut self,
        kind: IntegrationKind,
        cx: &mut Context<Self>,
    ) -> SettingsSection {
        let settings = Settings::global(cx);
        let state = state::get(cx, kind);
        let enabled = state::enabled(&settings, kind);
        let id = format!("settings.import.{}", kind.provider_id());
        let can_toggle = can_set_enabled(kind, !enabled, cx);
        let mut section = SettingsSection::new(id.clone(), kind.label());
        if state::credential(kind).is_some() {
            section = self.connection_rows(section, kind, &state, cx);
        } else {
            let (label, explanation): (&str, String) = match kind {
                #[cfg(target_os = "macos")]
                IntegrationKind::EventKit => match crate::imports::apple::availability() {
                    Ok(()) => ("Connection", "macOS asks for Calendar and Reminders permission when you first enable import. Switch import off to stop reading; existing items are kept.".into()),
                    Err(reason) => ("Unavailable on this Mac", reason),
                },
                #[cfg(feature = "private-workiq")]
                IntegrationKind::WorkIq => ("Connection", "Private WorkIQ import uses the CLI’s own account. Install @microsoft/workiq, run workiq auth login and workiq accept-eula. Switch import off to stop reading; existing items are kept.".into()),
                _ => unreachable!(),
            };
            section = section
                .row(SettingsRow::new(format!("{id}.access"), label).description(explanation));
        }
        section = section.row(
            SettingsRow::new(format!("{id}.enabled.row"), "Enable import")
                .search_terms(["Import", kind.label()])
                .switch(
                    Switch::new(format!("{id}.enabled"))
                        .named(format!("Import from {}", kind.label()))
                        .large()
                        .on(enabled)
                        .disabled(!can_toggle)
                        .when(can_toggle, |switch| {
                            switch.on_change(move |enabled, _, cx| {
                                if can_set_enabled(kind, enabled, cx) {
                                    Settings::update(cx, |settings| {
                                        state::set_enabled(settings, kind, enabled)
                                    });
                                }
                            })
                        }),
                ),
        );
        let channels: &[Channel] = match kind {
            IntegrationKind::Todoist => &[],
            IntegrationKind::GoogleCalendar => &[Channel::AcceptedOnly],
            _ => &[Channel::Calendar, Channel::Tasks, Channel::AcceptedOnly],
        };
        for channel in channels {
            section = section.row(channel.row(kind, cx));
        }
        for select in self.windows.iter().filter(|select| select.kind == kind) {
            section = section.row(select.row(cx));
        }
        if matches!(
            kind,
            IntegrationKind::GoogleCalendar | IntegrationKind::Outlook
        ) && state.connection == Connection::Stored
        {
            section = calendar_rows(section, kind, &state, cx);
        }
        let status = IntegrationStatuses::get(cx, kind);
        let status_text =
            sync_status_label(&status.state, enabled, state::sync_in_flight(cx, kind))
                .unwrap_or_else(|| {
                    if !can_enable(kind, &state) {
                        "Connection unavailable"
                    } else if !state::imports_anything(&settings, kind) {
                        "No channels selected"
                    } else {
                        "Ready to import"
                    }
                });
        section = section.row(
            SettingsRow::new(format!("{id}.status"), "Status")
                .value(status_text)
                .when_some(status.last_sync, |row, last| {
                    row.description(format!(
                        "Last successful import: {} · {} items updated",
                        last.with_timezone(&chrono::Local).format("%b %e, %H:%M"),
                        status.last_imported
                    ))
                }),
        );
        if let IntegrationState::Error(error) = status.state {
            section = section.row(error_row(
                format!("{id}.import-error"),
                "Import failed",
                error,
                cx,
            ));
        }
        if let Some(feedback) = &state.feedback {
            section = match feedback {
                Ok(message) => section.row(
                    SettingsRow::new(format!("{id}.feedback"), "Connection")
                        .description(message.clone()),
                ),
                Err(error) => section.row(error_row(
                    format!("{id}.feedback"),
                    "Connection needs attention",
                    error.clone(),
                    cx,
                )),
            };
        }
        section
    }

    fn connection_rows(
        &mut self,
        mut section: SettingsSection,
        kind: IntegrationKind,
        state: &ProviderState,
        cx: &mut Context<Self>,
    ) -> SettingsSection {
        let id = format!("settings.import.{}", kind.provider_id());
        let busy = state::busy(cx, kind);
        let configured = kind == IntegrationKind::Todoist || state::registration(kind).is_some();
        let connection_label = match &state.connection {
            Connection::Unknown => "Not checked",
            Connection::Disconnected => "Not connected",
            Connection::Stored => "Credential saved in keychain",
            Connection::Environment => "Using TODOIST_API_KEY",
            Connection::Error(_) => "Keychain unavailable",
        };
        section = section.row(
            SettingsRow::new(format!("{id}.connection"), "Connection")
                .value(
                    state
                        .operation
                        .map(Operation::label)
                        .unwrap_or(connection_label),
                )
                .description("Disconnecting keeps existing imported items.")
                .control(
                    div()
                        .row()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new(format!("{id}.check"))
                                .large()
                                .label("Check connection")
                                .disabled(busy)
                                .when(!busy, |button| {
                                    button.on_click(move |_, _, cx| state::check(kind, cx))
                                }),
                        )
                        .when(state.connection == Connection::Stored, |row| {
                            row.child(
                                Button::new(format!("{id}.disconnect"))
                                    .large()
                                    .label("Disconnect")
                                    .disabled(busy)
                                    .when(!busy, |button| {
                                        button.on_click(move |_, _, cx| state::disconnect(kind, cx))
                                    }),
                            )
                        }),
                ),
        );
        if !configured {
            let message = match kind {
                IntegrationKind::GoogleCalendar => {
                    "Google connection is unavailable: this build has no OAuth registration. Configure SUBROUTINE_GOOGLE_CLIENT_ID and SUBROUTINE_GOOGLE_CLIENT_SECRET."
                }
                _ => {
                    "Microsoft connection is unavailable: this build has no OAuth registration. Configure SUBROUTINE_MICROSOFT_CLIENT_ID."
                }
            };
            section = section.row(
                SettingsRow::new(format!("{id}.unavailable"), "Unavailable in this build")
                    .description(message),
            );
        }
        if state.connection == Connection::Environment {
            section = section.row(SettingsRow::new(format!("{id}.environment"), "Environment token")
                .description("To remove this connection, unset TODOIST_API_KEY and restart. You can switch import off here."));
        }
        if kind == IntegrationKind::Todoist {
            self.todoist_token.update(cx, |input, cx| {
                if input.is_disabled() != busy {
                    input.set_disabled(busy, cx);
                }
            });
            let can_connect = !busy && !self.todoist_token.read(cx).value(cx).trim().is_empty();
            section = section.row(SettingsRow::new(format!("{id}.token.row"), "API token")
                .info("Copy the API token from your Todoist settings. It is verified before being saved to this device’s keychain.")
                .control(div().column().gap_2().child(self.todoist_token.clone())
                    .child(Button::new(format!("{id}.connect")).large().label("Save connection").disabled(!can_connect)
                        .when(can_connect, |button| button.on_click(cx.listener(|panel, _, _, cx| panel.connect_todoist(cx)))))));
        } else {
            let can_connect = configured
                && !busy
                && !matches!(state.connection, Connection::Unknown | Connection::Error(_));
            section = section.row(
                SettingsRow::new(format!("{id}.connect.row"), "Browser sign-in")
                    .description(if state.operation == Some(Operation::Connect) {
                        "Continue in your browser. The request expires after five minutes."
                    } else {
                        "Connect directly to this provider; no Subroutine account is used."
                    })
                    .control(
                        Button::new(format!("{id}.connect"))
                            .large()
                            .label(if state.connection == Connection::Stored {
                                "Reconnect"
                            } else {
                                "Connect"
                            })
                            .disabled(!can_connect)
                            .when(can_connect, |button| {
                                button.on_click(move |_, _, cx| {
                                    if let Some(task) = state::connect(kind, None, cx) {
                                        task.detach();
                                    }
                                })
                            }),
                    ),
            );
        }
        section
    }
}

fn can_import_now(cx: &App) -> bool {
    if state::any_operation_in_progress(cx) {
        return false;
    }
    let settings = Settings::global(cx);
    KINDS.iter().any(|&kind| {
        !state::busy(cx, kind)
            && state::imports_anything(&settings, kind)
            && can_enable(kind, &state::get(cx, kind))
    })
}

fn can_set_enabled(kind: IntegrationKind, enabled: bool, cx: &App) -> bool {
    let state = state::get(cx, kind);
    if enabled {
        !state::busy(cx, kind) && can_enable(kind, &state)
    } else {
        state.can_disable()
    }
}

fn sync_status_label(
    status: &IntegrationState,
    enabled: bool,
    running: bool,
) -> Option<&'static str> {
    if running || *status == IntegrationState::Syncing {
        return Some(if enabled {
            "Importing…"
        } else {
            "Finishing current import…"
        });
    }
    if !enabled {
        return Some("Off");
    }
    match status {
        IntegrationState::Live => Some("Last import succeeded"),
        IntegrationState::Error(_) => Some("Needs attention"),
        _ => None,
    }
}

fn can_enable(kind: IntegrationKind, state: &ProviderState) -> bool {
    #[cfg(target_os = "macos")]
    if kind == IntegrationKind::EventKit {
        return crate::imports::apple::availability().is_ok();
    }
    if state::credential(kind).is_none() {
        return true;
    }
    matches!(
        state.connection,
        Connection::Stored | Connection::Environment
    ) && (kind == IntegrationKind::Todoist || state::registration(kind).is_some())
}

fn error_row(id: String, label: &'static str, error: String, cx: &App) -> SettingsRow {
    SettingsRow::new(id, label).control(
        Label::new(error)
            .text_sm()
            .text_color(cx.theme().colors.danger),
    )
}

fn calendar_rows(
    mut section: SettingsSection,
    kind: IntegrationKind,
    state: &ProviderState,
    cx: &App,
) -> SettingsSection {
    let id = format!("settings.import.{}", kind.provider_id());
    let settings = Settings::global(cx);
    let available = !state::busy(cx, kind) && state::registration(kind).is_some();
    section = section.row(SettingsRow::new(format!("{id}.calendars"), "Calendars")
        .description("The primary calendar is used until you choose. Keep at least one selected; switch import off to pause.")
        .control(Button::new(format!("{id}.calendars.load")).large().label(if state.calendars.is_some() { "Refresh calendars" } else { "Choose calendars" }).disabled(!available)
            .when(available, |button| button.on_click(move |_, _, cx| state::load_calendars(kind, cx)))));
    let Some(calendars) = &state.calendars else {
        return section;
    };
    if calendars.is_empty() {
        return section.row(
            SettingsRow::new(format!("{id}.calendars.empty"), "No calendars returned").description(
                "The account did not expose any calendars. Check provider access before importing.",
            ),
        );
    }
    let is_enabled = |id: &str, primary: bool| match kind {
        IntegrationKind::GoogleCalendar => settings
            .integrations
            .google_calendar
            .calendar_enabled(id, primary),
        _ => settings.integrations.outlook.calendar_enabled(id, primary),
    };
    let selected_count = calendars
        .iter()
        .filter(|calendar| is_enabled(&calendar.id, calendar.is_default))
        .count();
    let known: Vec<_> = calendars
        .iter()
        .map(|calendar| (calendar.id.clone(), calendar.is_default))
        .collect();
    for calendar in calendars {
        let selected = is_enabled(&calendar.id, calendar.is_default);
        let can_toggle = available && (!selected || selected_count > 1);
        let calendar_id = calendar.id.clone();
        let known = known.clone();
        section = section.row(
            SettingsRow::new(
                format!("{id}.calendar.{}.row", calendar.id),
                calendar.name.clone(),
            )
            .search_terms(["Import", kind.label()])
            .when(calendar.is_default, |row| row.value("Primary"))
            .switch(
                Switch::new(format!("{id}.calendar.{}", calendar.id))
                    .named(calendar.name.clone())
                    .large()
                    .on(selected)
                    .disabled(!can_toggle)
                    .when(can_toggle, |switch| {
                        switch.on_change(move |enabled, _, cx| {
                            if state::busy(cx, kind) {
                                return;
                            }
                            Settings::update(cx, |settings| match kind {
                                IntegrationKind::GoogleCalendar => settings
                                    .integrations
                                    .google_calendar
                                    .set_calendar_enabled(&calendar_id, enabled, &known),
                                _ => settings.integrations.outlook.set_calendar_enabled(
                                    &calendar_id,
                                    enabled,
                                    &known,
                                ),
                            });
                        })
                    }),
            ),
        );
    }
    section
}
