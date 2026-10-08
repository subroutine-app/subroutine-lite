use gpui::{App, SharedString};
use gpui_kit::{display::empty::EmptyState, overlay::Command};
use gpui_kit_theme::{ActiveTheme as _, Appearance};

use super::items::{self, LibraryCommand};
use crate::{
    keys::ConfigurableCommand,
    stores::AppDatabaseStore,
    themes::{ThemeCatalog, ThemeEntry},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickerCommand {
    Library(LibraryCommand),
    ChooseTheme,
}

pub(super) enum PickerSelection {
    Applied,
    Theme(SharedString),
}

impl PickerCommand {
    pub(super) fn from_command(command: ConfigurableCommand) -> Option<Self> {
        if command == ConfigurableCommand::ChooseTheme {
            Some(Self::ChooseTheme)
        } else {
            LibraryCommand::from_command(command).map(Self::Library)
        }
    }

    pub(super) fn configurable_command(self) -> ConfigurableCommand {
        match self {
            Self::Library(command) => command.configurable_command(),
            Self::ChooseTheme => ConfigurableCommand::ChooseTheme,
        }
    }

    pub(super) fn palette_id(self) -> &'static str {
        match self {
            Self::Library(_) => "command-palette.library",
            Self::ChooseTheme => "command-palette.themes",
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Library(command) => command.title(),
            Self::ChooseTheme => "Choose theme",
        }
    }

    pub(super) fn placeholder(self) -> &'static str {
        match self {
            Self::Library(command) => command.placeholder(),
            Self::ChooseTheme => "Search themes…",
        }
    }

    pub(super) fn refresh_hint(self) -> &'static str {
        match self {
            Self::Library(_) => "Refresh to review the updated list before choosing an item.",
            Self::ChooseTheme => "Refresh to review the updated themes before choosing one.",
        }
    }

    pub(super) fn commands(self, cx: &App) -> Vec<Command> {
        match self {
            Self::Library(command) => command.commands(AppDatabaseStore::global(cx).read(cx)),
            Self::ChooseTheme => {
                theme_commands(cx.global::<ThemeCatalog>().entries(), &cx.theme().id)
            }
        }
    }

    pub(super) fn empty_state(self, has_items: bool, cx: &App) -> EmptyState {
        match self {
            Self::Library(command) => {
                command.empty_state(AppDatabaseStore::global(cx).read(cx), has_items)
            }
            Self::ChooseTheme if has_items => {
                EmptyState::new("command-palette.themes.empty", "No matching themes")
            }
            Self::ChooseTheme => {
                EmptyState::new("command-palette.themes.empty", "No themes available")
            }
        }
    }

    pub(super) fn warning(self, cx: &App) -> Option<&'static str> {
        match self {
            Self::Library(_) => items::warning(AppDatabaseStore::global(cx).read(cx)),
            Self::ChooseTheme => None,
        }
    }

    pub(super) fn invoke(
        self,
        row_id: &str,
        cx: &mut App,
    ) -> Result<PickerSelection, &'static str> {
        match self {
            Self::Library(command) => {
                AppDatabaseStore::global(cx)
                    .update(cx, |store, cx| command.invoke(row_id, store, cx))?;
                Ok(PickerSelection::Applied)
            }
            Self::ChooseTheme => resolve_theme(row_id, cx.global::<ThemeCatalog>().entries())
                .map(PickerSelection::Theme),
        }
    }
}

fn theme_commands(entries: &[ThemeEntry], active: &str) -> Vec<Command> {
    entries
        .iter()
        .map(|entry| {
            let label = if entry.id == active {
                format!("{} (current)", entry.name)
            } else {
                entry.name.to_string()
            };
            Command::new(format!("theme:{}", entry.id), label).section(match entry.appearance {
                Appearance::Light => "Light themes",
                Appearance::Dark => "Dark themes",
            })
        })
        .collect()
}

fn resolve_theme(row_id: &str, entries: &[ThemeEntry]) -> Result<SharedString, &'static str> {
    let id = row_id.strip_prefix("theme:").ok_or("That isn’t a theme.")?;
    entries
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.id.clone())
        .ok_or("That theme is no longer available.")
}
