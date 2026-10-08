use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{AppearancePreference, Settings};

use format::PersistedSettings;

mod format;

const SETTINGS_FILE: &str = "desktop-settings.json";

pub(crate) fn settings_path() -> Option<PathBuf> {
    match crate::paths::get() {
        Ok(paths) => Some(paths.config_dir.join(SETTINGS_FILE)),
        Err(error) => {
            tracing::error!(%error, "settings location is unavailable");
            None
        }
    }
}

fn legacy_settings_path() -> Option<PathBuf> {
    if std::env::var_os("SUBROUTINE_LITE_CONFIG_DIR").is_some() {
        return None;
    }
    crate::paths::get()
        .ok()
        .map(|paths| paths.data_dir.join(SETTINGS_FILE))
}

fn exists_or_is_unreadable(path: &Path) -> bool {
    path.try_exists().unwrap_or(true)
}

fn load_persisted(path: &Path) -> Result<Option<(PersistedSettings, bool)>, String> {
    let encoded = match fs::read(path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let value: serde_json::Value = serde_json::from_slice(&encoded)
        .map_err(|error| format!("decode {}: {error}", path.display()))?;

    format::decode(value, path).map(Some)
}

fn save_persisted(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&PersistedSettings::from_settings(settings))
        .map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)
        .map_err(|error| format!("create {}: {error}", temporary.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    file.sync_all()
        .map_err(|error| format!("sync {}: {error}", temporary.display()))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| format!("replace {}: {error}", path.display()))
}

fn quarantine_invalid(path: &Path) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SETTINGS_FILE);
    let quarantine = path.with_file_name(format!("{file_name}.invalid-{timestamp}"));
    fs::rename(path, &quarantine).map_err(|error| {
        format!(
            "quarantine {} as {}: {error}",
            path.display(),
            quarantine.display()
        )
    })?;
    Ok(quarantine)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPersistenceOperation {
    Read,
    Migrate,
    Save,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPersistenceIssue {
    operation: SettingsPersistenceOperation,
    path: Option<PathBuf>,
}

impl SettingsPersistenceIssue {
    fn new(operation: SettingsPersistenceOperation, path: Option<PathBuf>) -> Self {
        Self { operation, path }
    }

    pub fn title(&self) -> &'static str {
        match self.operation {
            SettingsPersistenceOperation::Read => "Settings couldn’t be loaded",
            SettingsPersistenceOperation::Migrate => "Settings couldn’t be upgraded",
            SettingsPersistenceOperation::Save => "Settings couldn’t be saved",
        }
    }

    pub fn reason(&self) -> &'static str {
        if self.path.is_none() {
            return "The configuration folder couldn’t be found.";
        }

        match self.operation {
            SettingsPersistenceOperation::Read => "The settings file is unreadable or invalid.",
            SettingsPersistenceOperation::Migrate | SettingsPersistenceOperation::Save => {
                "Check folder permissions and free disk space."
            }
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Settings {
    pub(super) fn load(legacy_appearance: Option<AppearancePreference>) -> Self {
        let mut settings = Self::default();
        let Some(path) = settings_path() else {
            settings.persistence_issue = Some(SettingsPersistenceIssue::new(
                SettingsPersistenceOperation::Read,
                None,
            ));
            return settings;
        };
        let source = if exists_or_is_unreadable(&path) {
            Some(path.clone())
        } else {
            legacy_settings_path().filter(|legacy| exists_or_is_unreadable(legacy))
        };
        if let Some(source) = source {
            settings.restore_from(&source, &path, legacy_appearance);
        } else if let Some(appearance) = legacy_appearance {
            settings.appearance = appearance;
            if let Err(error) = settings.save_to(&path, SettingsPersistenceOperation::Migrate) {
                tracing::warn!(%error, "could not migrate legacy appearance setting");
            }
        }
        settings
    }

    fn restore_from(
        &mut self,
        source: &Path,
        destination: &Path,
        legacy_appearance: Option<AppearancePreference>,
    ) {
        let (mut persisted, migrated) = match load_persisted(source) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => return,
            Err(error) => {
                self.quarantine(source, SettingsPersistenceOperation::Read, &error);
                return;
            }
        };
        let migrated_appearance = persisted.appearance.is_none() && legacy_appearance.is_some();
        if let Some(appearance) = legacy_appearance
            && persisted.appearance.is_none()
        {
            persisted.appearance = Some(appearance.id().into());
        }
        if let Err(error) = persisted.apply_to(self) {
            let operation = if migrated {
                SettingsPersistenceOperation::Migrate
            } else {
                SettingsPersistenceOperation::Read
            };
            self.quarantine(source, operation, &error);
            return;
        }
        if (migrated || migrated_appearance || source != destination)
            && let Err(error) = self.save_to(destination, SettingsPersistenceOperation::Migrate)
        {
            tracing::warn!(%error, "could not migrate desktop settings");
        }
    }

    fn quarantine(&mut self, path: &Path, operation: SettingsPersistenceOperation, error: &str) {
        self.persistence_issue = Some(SettingsPersistenceIssue::new(
            operation,
            Some(path.to_path_buf()),
        ));
        match quarantine_invalid(path) {
            Ok(quarantine) => tracing::warn!(
                %error,
                path = %quarantine.display(),
                "invalid or unreadable desktop settings were quarantined"
            ),
            Err(quarantine_error) => tracing::warn!(
                %error,
                %quarantine_error,
                "invalid or unreadable desktop settings could not be quarantined"
            ),
        }
    }

    pub(super) fn persist(&mut self) -> Result<(), String> {
        let Some(path) = settings_path() else {
            self.persistence_issue = Some(SettingsPersistenceIssue::new(
                SettingsPersistenceOperation::Save,
                None,
            ));
            return Err("could not locate the desktop settings directory".into());
        };
        self.save_to(&path, SettingsPersistenceOperation::Save)
    }

    fn save_to(
        &mut self,
        path: &Path,
        operation: SettingsPersistenceOperation,
    ) -> Result<(), String> {
        match save_persisted(path, self) {
            Ok(()) => {
                self.persistence_issue = None;
                Ok(())
            }
            Err(error) => {
                self.persistence_issue = Some(SettingsPersistenceIssue::new(
                    operation,
                    Some(path.to_path_buf()),
                ));
                Err(error)
            }
        }
    }

    pub fn persistence_issue(&self) -> Option<&SettingsPersistenceIssue> {
        self.persistence_issue.as_ref()
    }

    pub fn configuration_folder() -> Option<PathBuf> {
        settings_path().and_then(|path| path.parent().map(Path::to_path_buf))
    }

    pub fn open_configuration_folder() -> Result<(), String> {
        let folder = Self::configuration_folder()
            .ok_or_else(|| "configuration folder is unavailable".to_owned())?;
        fs::create_dir_all(&folder)
            .map_err(|error| format!("create {}: {error}", folder.display()))?;
        open::that(&folder).map_err(|error| format!("open {}: {error}", folder.display()))
    }
}
