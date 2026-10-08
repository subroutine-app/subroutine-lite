use std::{
    ffi::OsString,
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

const STORAGE_FILE: &str = "desktop-storage.json";
static PATHS: OnceLock<StoragePaths> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoragePaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

pub(crate) fn get() -> Result<&'static StoragePaths, String> {
    cached_paths(&PATHS, || {
        let defaults = StoragePaths::defaults()?;
        resolve(
            &defaults,
            std::env::var_os("SUBROUTINE_LITE_DATA_DIR"),
            std::env::var_os("SUBROUTINE_LITE_CONFIG_DIR"),
        )
    })
}

fn cached_paths(
    cache: &OnceLock<StoragePaths>,
    load: impl FnOnce() -> Result<StoragePaths, String>,
) -> Result<&StoragePaths, String> {
    if let Some(paths) = cache.get() {
        return Ok(paths);
    }
    let paths = load()?;
    let _ = cache.set(paths);
    Ok(cache.get().expect("a successful selection was installed"))
}

impl StoragePaths {
    fn defaults() -> Result<Self, String> {
        Ok(Self {
            data_dir: dirs::data_local_dir()
                .ok_or("The operating system did not provide a local data directory.")?
                .join("Subroutine Lite"),
            config_dir: dirs::config_dir()
                .ok_or("The operating system did not provide a configuration directory.")?
                .join("Subroutine Lite"),
        })
    }
}

fn resolve(
    defaults: &StoragePaths,
    data: Option<OsString>,
    config: Option<OsString>,
) -> Result<StoragePaths, String> {
    if data.is_some() || config.is_some() {
        return Ok(StoragePaths {
            data_dir: override_path(data, &defaults.data_dir, "SUBROUTINE_LITE_DATA_DIR")?,
            config_dir: override_path(config, &defaults.config_dir, "SUBROUTINE_LITE_CONFIG_DIR")?,
        });
    }
    let selection = defaults.config_dir.join(STORAGE_FILE);
    match std::fs::symlink_metadata(&selection) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(format!(
                "Storage selection must be a regular file: {}",
                selection.display()
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(defaults.clone()),
        Err(error) => return Err(format!("Cannot inspect {}: {error}", selection.display())),
    }
    let file = File::open(&selection)
        .map_err(|error| format!("Cannot read {}: {error}", selection.display()))?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read {}: {error}", selection.display()))?;
    let paths: StoragePaths = serde_json::from_slice(&bytes)
        .ok()
        .filter(|_| bytes.len() <= 16 * 1024)
        .ok_or_else(|| {
            format!(
                "Invalid storage selection at {}. No fallback workspace was opened.",
                selection.display()
            )
        })?;
    for directory in [&paths.data_dir, &paths.config_dir] {
        if !directory.is_absolute() || !directory.is_dir() {
            return Err(format!(
                "Storage selected by {} is missing or inaccessible: {}. Restore access or explicitly select another location; no fallback workspace was opened.",
                selection.display(),
                directory.display()
            ));
        }
    }
    Ok(paths)
}

fn override_path(value: Option<OsString>, default: &Path, name: &str) -> Result<PathBuf, String> {
    match value {
        Some(value) if value.is_empty() => Err(format!("{name} must not be empty.")),
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(default.to_owned()),
    }
}
