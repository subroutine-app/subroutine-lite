use std::time::Duration;

use gpui::{App, Global, Task};
use windows::{
    Foundation::{EventHandler, TypedEventHandler},
    System::Power::{EnergySaverStatus, PowerManager},
    UI::ViewManagement::{AccessibilitySettings, UISettings},
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Graphics::Dwm::DwmIsCompositionEnabled,
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            SystemInformation::OSVERSIONINFOW,
        },
        UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION},
    },
    core::{IInspectable, Result, s, w},
};

struct SystemTransparency {
    settings: SettingsObserver,
    _updates: Task<()>,
    _polling: Option<Task<()>>,
}

impl Global for SystemTransparency {}

pub(super) fn init(cx: &mut App) {
    if cx.has_global::<SystemTransparency>() || !supports_acrylic() {
        return;
    }

    let (changes_tx, changes_rx) = flume::bounded(1);
    let settings = match SettingsObserver::new(changes_tx) {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(%error, "Windows material settings unavailable; using opaque windows");
            return;
        }
    };
    // Some Windows installations expose the settings but cannot register every event.
    // Poll only in that case so accessibility changes still reach an idle window.
    let polling = settings.needs_polling().then(|| {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                // A successful listener or a render may have observed an intermediate
                // setting, so comparing only consecutive polls can miss a change.
                cx.update(|cx| cx.refresh_windows());
            }
        })
    });
    let updates = cx.spawn(async move |cx| {
        while changes_rx.recv_async().await.is_ok() {
            cx.update(|cx| cx.refresh_windows());
        }
    });
    cx.set_global(SystemTransparency {
        settings,
        _updates: updates,
        _polling: polling,
    });
    cx.on_app_quit(|cx| {
        // Release WinRT objects before GPUI tears down the OLE apartment.
        drop(cx.remove_global::<SystemTransparency>());
        async {}
    })
    .detach();
    cx.refresh_windows();
}

pub(super) fn allows_transparency(cx: &App) -> bool {
    cx.try_global::<SystemTransparency>().is_some_and(|state| {
        state.settings.allows_transparency().unwrap_or(false)
            && unsafe {
                GetSystemMetrics(SM_REMOTESESSION) == 0
                    && DwmIsCompositionEnabled().is_ok_and(|enabled| enabled.as_bool())
            }
    })
}

fn supports_acrylic() -> bool {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    if !unsafe { RtlGetVersion(&mut version) }.is_ok()
        || (
            version.dwMajorVersion,
            version.dwMinorVersion,
            version.dwBuildNumber,
        ) < (10, 0, 17763)
    {
        return false;
    }

    // GPUI owns acrylic rendering; only probe its required entry point here.
    unsafe {
        GetModuleHandleW(w!("user32.dll")).is_ok_and(|module| {
            GetProcAddress(module, s!("SetWindowCompositionAttribute")).is_some()
        })
    }
}

struct SettingsObserver {
    ui: UISettings,
    accessibility: AccessibilitySettings,
    effects_changed: Option<i64>,
    contrast_changed: Option<i64>,
    energy_saver_changed: Option<i64>,
}

impl SettingsObserver {
    fn new(changes: flume::Sender<()>) -> Result<Self> {
        // GPUI initializes OLE on this thread before application initialization.
        let mut observer = Self {
            ui: UISettings::new()?,
            accessibility: AccessibilitySettings::new()?,
            effects_changed: None,
            contrast_changed: None,
            energy_saver_changed: None,
        };

        // Only senders cross callback threads. Full/disconnected queues need no action.
        let effects = changes.clone();
        observer.effects_changed = notification_token(
            "AdvancedEffectsEnabledChanged",
            observer.ui.AdvancedEffectsEnabledChanged(
                &TypedEventHandler::<UISettings, IInspectable>::new(move |_, _| {
                    let _ = effects.try_send(());
                    Ok(())
                }),
            ),
        );
        let contrast = changes.clone();
        observer.contrast_changed = notification_token(
            "HighContrastChanged",
            observer
                .accessibility
                .HighContrastChanged(
                    &TypedEventHandler::<AccessibilitySettings, IInspectable>::new(move |_, _| {
                        let _ = contrast.try_send(());
                        Ok(())
                    }),
                ),
        );
        let energy_saver = EventHandler::<IInspectable>::new(move |_, _| {
            let _ = changes.try_send(());
            Ok(())
        });
        observer.energy_saver_changed = notification_token(
            "EnergySaverStatusChanged",
            PowerManager::EnergySaverStatusChanged(&energy_saver),
        );
        Ok(observer)
    }

    fn needs_polling(&self) -> bool {
        self.effects_changed.is_none()
            || self.contrast_changed.is_none()
            || self.energy_saver_changed.is_none()
    }

    fn allows_transparency(&self) -> Result<bool> {
        Ok(self.ui.AdvancedEffectsEnabled()?
            && !self.accessibility.HighContrast()?
            && matches!(
                PowerManager::EnergySaverStatus()?,
                EnergySaverStatus::Disabled | EnergySaverStatus::Off
            ))
    }
}

fn notification_token(event: &str, result: Result<i64>) -> Option<i64> {
    result
        .inspect_err(|error| {
            tracing::warn!(%error, event, "Windows material notification unavailable; polling settings");
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{Error, HRESULT};

    #[test]
    fn successful_notification_keeps_its_cleanup_token() {
        assert_eq!(notification_token("test", Ok(42)), Some(42));
    }

    #[test]
    fn unavailable_notification_does_not_abort_other_registrations() {
        let element_not_found = Error::from_hresult(HRESULT(0x80070490u32 as i32));
        let tokens = [
            notification_token("AdvancedEffectsEnabledChanged", Ok(1)),
            notification_token("HighContrastChanged", Err(element_not_found)),
            notification_token("EnergySaverStatusChanged", Ok(3)),
        ];
        assert_eq!(tokens, [Some(1), None, Some(3)]);
    }
}

impl Drop for SettingsObserver {
    fn drop(&mut self) {
        // Failed registrations have no token; retain and release every successful one.
        for result in [
            self.energy_saver_changed
                .map(PowerManager::RemoveEnergySaverStatusChanged),
            self.contrast_changed
                .map(|token| self.accessibility.RemoveHighContrastChanged(token)),
            self.effects_changed
                .map(|token| self.ui.RemoveAdvancedEffectsEnabledChanged(token)),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(error) = result {
                tracing::warn!(%error, "Failed to unregister Windows material notification");
            }
        }
    }
}
