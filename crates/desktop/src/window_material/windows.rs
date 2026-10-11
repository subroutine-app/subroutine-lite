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
            tracing::warn!(%error, "Windows material notifications unavailable; using opaque windows");
            return;
        }
    };
    let updates = cx.spawn(async move |cx| {
        while changes_rx.recv_async().await.is_ok() {
            cx.update(|cx| cx.refresh_windows());
        }
    });
    cx.set_global(SystemTransparency {
        settings,
        _updates: updates,
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
        observer.effects_changed = Some(observer.ui.AdvancedEffectsEnabledChanged(
            &TypedEventHandler::<UISettings, IInspectable>::new(move |_, _| {
                let _ = effects.try_send(());
                Ok(())
            }),
        )?);
        let contrast = changes.clone();
        observer.contrast_changed = Some(observer.accessibility.HighContrastChanged(
            &TypedEventHandler::<AccessibilitySettings, IInspectable>::new(move |_, _| {
                let _ = contrast.try_send(());
                Ok(())
            }),
        )?);
        let energy_saver = EventHandler::<IInspectable>::new(move |_, _| {
            let _ = changes.try_send(());
            Ok(())
        });
        observer.energy_saver_changed =
            Some(PowerManager::EnergySaverStatusChanged(&energy_saver)?);
        Ok(observer)
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

impl Drop for SettingsObserver {
    fn drop(&mut self) {
        // Optional tokens also roll back partially completed registration.
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
