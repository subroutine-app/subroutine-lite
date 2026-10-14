#[cfg(all(feature = "eventkit", target_os = "macos"))]
pub mod eventkit;

#[cfg(feature = "google-calendar")]
pub mod google_calendar;

#[cfg(any(feature = "outlook", feature = "workiq"))]
pub mod graph;

#[cfg(feature = "outlook")]
pub mod outlook;

#[cfg(feature = "todoist")]
pub mod todoist;

#[cfg(feature = "workiq")]
pub mod workiq;
