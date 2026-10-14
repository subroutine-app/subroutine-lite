use std::{path::Path, sync::OnceLock};

use objc2_foundation::{NSBundle, NSString};
use subroutine_import::providers::eventkit::EventKitProvider;

pub fn availability() -> Result<(), String> {
    static AVAILABILITY: OnceLock<Result<(), String>> = OnceLock::new();
    AVAILABILITY
        .get_or_init(|| objc2::rc::autoreleasepool(|_| check()))
        .clone()
}

fn check() -> Result<(), String> {
    EventKitProvider::availability().map_err(|error| error.to_string())?;
    let bundle = NSBundle::mainBundle();
    let identifier = bundle.bundleIdentifier().map(|value| value.to_string());
    let bundle_path = bundle.bundlePath().to_string();
    let has_description = |key| {
        bundle
            .objectForInfoDictionaryKey(&NSString::from_str(key))
            .as_deref()
            .and_then(|value| value.downcast_ref::<NSString>())
            .is_some_and(|value| !value.to_string().trim().is_empty())
    };
    if identifier.as_deref() != Some(crate::branding::APP_ID)
        || Path::new(&bundle_path)
            .extension()
            .and_then(|value| value.to_str())
            != Some("app")
        || !has_description("NSCalendarsFullAccessUsageDescription")
        || !has_description("NSRemindersFullAccessUsageDescription")
    {
        return Err("Apple import requires the packaged Subroutine Lite app with Calendar and Reminders privacy descriptions. Build it with script/bundle-mac; cargo run does not provide these permissions.".into());
    }
    Ok(())
}
