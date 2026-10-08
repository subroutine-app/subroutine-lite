#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use tracing_subscriber::EnvFilter;

mod app;
mod assets;
mod auth;
mod branding;
mod color;
mod components;
mod dates;
mod easing;
mod haptics;
mod icons;

mod item_manager;
mod item_subject;
mod keys;
mod notifications;
mod paths;
mod presentation;
mod selection;
mod settings;
mod stores;
mod themes;
mod utils;
mod views;
mod window_state;

pub use icons::AppIcon;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--print-config")) {
        let connection = auth::Config::from_env().map_err(anyhow::Error::msg)?;
        let storage = paths::get().map_err(anyhow::Error::msg)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "offline_only": connection.is_none(),
                "connection": connection,
                "storage": storage,
            }))?
        );
        return Ok(());
    }
    app::run()
}
