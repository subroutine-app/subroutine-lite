use std::time::Duration;

use anyhow::Context as _;
use gpui::{App, Global, Hsla, Task, Window, WindowAppearance, WindowBackgroundAppearance};
use gpui_kit_theme::{ActiveTheme as _, Appearance};
use raw_window_handle::RawWindowHandle;
use windows::{
    Foundation::{EventHandler, TypedEventHandler},
    System::Power::{EnergySaverStatus, PowerManager},
    UI::ViewManagement::{AccessibilitySettings, UISettings},
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWM_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmIsCompositionEnabled, DwmSetWindowAttribute,
        },
        System::SystemInformation::OSVERSIONINFOW,
        UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION},
    },
    core::{BOOL, IInspectable, Result},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Backdrop {
    #[default]
    Opaque,
    Mica,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Request {
    backdrop: Backdrop,
    appearance: Appearance,
    system_appearance: WindowAppearance,
}

#[derive(Default)]
pub(crate) struct WindowMaterial {
    request: Option<Request>,
    backdrop: Backdrop,
}

impl WindowMaterial {
    pub(crate) fn background(&mut self, window: &mut Window, cx: &App) -> Hsla {
        let request = Request {
            backdrop: if crate::settings::Settings::global(cx).frosted_background
                && !cx.theme().reduce_transparency
                && allows_transparency(cx)
            {
                Backdrop::Mica
            } else {
                Backdrop::Opaque
            },
            appearance: cx.theme().appearance,
            // GPUI reapplies system DWM colors when this changes, even with an app override.
            system_appearance: window.appearance(),
        };
        if self.request != Some(request) {
            self.backdrop = match apply_backdrop(request, window, cx) {
                Ok(backdrop) => backdrop,
                Err(error) => {
                    tracing::warn!(%error, "Native window backdrop unavailable; using an opaque background");
                    window.set_background_appearance(WindowBackgroundAppearance::Opaque);
                    Backdrop::Opaque
                }
            };
            self.request = Some(request);
            window.refresh();
        }
        match self.backdrop {
            Backdrop::Opaque => cx.theme().colors.canvas.alpha(1.0),
            // Mica is itself opaque. Only the root paint exposes it, not the desktop.
            Backdrop::Mica => gpui::black().alpha(0.0),
        }
    }
}

fn apply_backdrop(request: Request, window: &mut Window, cx: &App) -> anyhow::Result<Backdrop> {
    if !cx.has_global::<SystemTransparency>() {
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        return Ok(Backdrop::Opaque);
    }
    let handle = raw_window_handle::HasWindowHandle::window_handle(window)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        anyhow::bail!("expected a Win32 window handle");
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    apply_native_backdrop(request, window, hwnd).inspect_err(|_| {
        if let Err(error) = set_backdrop(hwnd, DWMSBT_NONE) {
            tracing::warn!(%error, "Failed to clear rejected native backdrop");
        }
    })?;
    Ok(request.backdrop)
}

fn apply_native_backdrop(request: Request, window: &mut Window, hwnd: HWND) -> anyhow::Result<()> {
    let dark_mode = BOOL::from(request.appearance == Appearance::Dark);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark_mode as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        )
    }
    .context("set native window appearance")?;

    // GPUI 27103c3's Mica setter ignores DWM failures and Opaque does not clear
    // DWMWA_SYSTEMBACKDROP_TYPE. Own the checked DWM lifecycle until it does.
    match request.backdrop {
        Backdrop::Mica => {
            window.set_background_appearance(WindowBackgroundAppearance::MicaBackdrop);
            set_backdrop(hwnd, DWMSBT_MAINWINDOW).context("enable Mica")?;
        }
        Backdrop::Opaque => {
            window.set_background_appearance(WindowBackgroundAppearance::Opaque);
            set_backdrop(hwnd, DWMSBT_NONE).context("clear native backdrop")?;
        }
    }
    Ok(())
}

fn set_backdrop(hwnd: HWND, backdrop: DWM_SYSTEMBACKDROP_TYPE) -> Result<()> {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop as *const DWM_SYSTEMBACKDROP_TYPE).cast(),
            size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
    }
}

struct SystemTransparency {
    settings: SettingsObserver,
    _updates: Task<()>,
    _polling: Option<Task<()>>,
}

impl Global for SystemTransparency {}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<SystemTransparency>() || !supports_mica() {
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

fn supports_mica() -> bool {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    unsafe { RtlGetVersion(&mut version) }.is_ok()
        && supports_system_backdrop(
            version.dwMajorVersion,
            version.dwMinorVersion,
            version.dwBuildNumber,
        )
}

fn supports_system_backdrop(major: u32, minor: u32, build: u32) -> bool {
    // The documented DWM API starts at Windows 11 22H2, not the original 22000 release.
    (major, minor, build) >= (10, 0, 22621)
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
