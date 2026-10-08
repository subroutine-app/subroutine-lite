use super::RootView;
use crate::{
    AppIcon,
    app::{SignIn, SignOut},
    auth::{AuthSession, AuthenticationState},
    components::{Button, ButtonVariants as _, Label, timed_toast},
    stores::{AppDatabaseStore, StoreStatus},
};
use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Styled, div,
    prelude::FluentBuilder, px,
};
use gpui_kit::{
    display::badge::Tone,
    foundation::{Disableable as _, StyledExt as _},
    overlay::toast,
};
use gpui_kit_theme::ActiveTheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthenticationScreenAction {
    SignIn,
    SignOut,
}

struct AuthenticationScreen {
    title: &'static str,
    caption: Option<String>,
    action: Option<AuthenticationScreenAction>,
}

fn authentication_screen(
    store_status: &StoreStatus,
    authentication: &AuthenticationState,
    sign_in_configured: bool,
) -> Option<AuthenticationScreen> {
    if *store_status == StoreStatus::Ready {
        return None;
    }

    let server_requires_authentication = *store_status == StoreStatus::AuthenticationRequired;
    match authentication {
        AuthenticationState::SignedOut if sign_in_configured || server_requires_authentication => {
            Some(AuthenticationScreen {
                title: if sign_in_configured {
                    "Sign in to Subroutine Lite"
                } else {
                    "Can’t access this account"
                },
                caption: None,
                action: sign_in_configured.then_some(AuthenticationScreenAction::SignIn),
            })
        }
        AuthenticationState::SigningIn => Some(AuthenticationScreen {
            title: "Finish signing in",
            caption: Some("Continue in your browser.".to_owned()),
            action: None,
        }),
        AuthenticationState::Restoring
        | AuthenticationState::Refreshing
        | AuthenticationState::SigningOut => Some(AuthenticationScreen {
            title: "Restoring your session…",
            caption: None,
            action: None,
        }),
        AuthenticationState::Offline(message) => Some(AuthenticationScreen {
            title: "Offline",
            caption: Some(message.clone()),
            action: None,
        }),
        AuthenticationState::Error(message) => Some(AuthenticationScreen {
            title: "Sign-in problem",
            caption: Some(message.clone()),
            action: sign_in_configured.then_some(AuthenticationScreenAction::SignIn),
        }),
        AuthenticationState::SignedIn if server_requires_authentication => {
            Some(AuthenticationScreen {
                title: "Sign-in required",
                caption: Some(
                    "Your session wasn’t accepted. Sign out, then sign in again.".to_owned(),
                ),
                action: Some(AuthenticationScreenAction::SignOut),
            })
        }
        AuthenticationState::SignedOut | AuthenticationState::SignedIn => None,
    }
}

impl RootView {
    pub(super) fn render_status(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let store = AppDatabaseStore::global(cx);
        let store_status = store.read(cx).status();
        if store.read(cx).local_retry_in_flight() {
            return Some(self.status_screen(
                "account.workspace-loading",
                "Opening account workspace…",
                Some("Your saved data and queued edits stay in their original workspace.".into()),
                Vec::new(),
                cx,
            ));
        }
        if store_status != StoreStatus::Ready {
            self.cancel_drag_navigation();
            self.layout_state
                .update(cx, |layout, _| layout.invalidate_layout());
        }
        if store_status == StoreStatus::NotConfigured {
            return Some(self.status_screen(
                "server-status.not-configured",
                "Can’t connect to your server",
                Some("Check your server connection.".to_owned()),
                Vec::new(),
                cx,
            ));
        }

        let auth = AuthSession::global(cx);
        let authentication = auth.state();
        let sign_in_configured = auth.is_interactive_sign_in_configured();

        if let Some(screen) =
            authentication_screen(&store_status, &authentication, sign_in_configured)
        {
            let actions = screen
                .action
                .map(|action| self.authentication_button(action))
                .into_iter()
                .collect();
            return Some(self.status_screen(
                "authentication-status",
                screen.title,
                screen.caption,
                actions,
                cx,
            ));
        }

        if store_status != StoreStatus::Ready {
            return Some(match store_status {
                StoreStatus::Error(message) => {
                    let retrying = store.read(cx).local_retry_in_flight();

                    self.status_screen(
                        "local-data-status.error",
                        "Can’t open local data",
                        None,
                        self.local_data_recovery_buttons(retrying, &message),
                        cx,
                    )
                }
                StoreStatus::NotConfigured
                | StoreStatus::AuthenticationRequired
                | StoreStatus::Ready => unreachable!(),
            });
        }

        None
    }

    fn status_screen(
        &self,
        id: &'static str,
        title: &'static str,
        caption: Option<String>,
        actions: Vec<Button>,
        cx: &App,
    ) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .inset_0()
            .bg(cx.theme().colors.canvas)
            .text_color(cx.theme().colors.text)
            .child(
                div()
                    .flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .child(
                        div()
                            .column()
                            .items_center()
                            .gap_3()
                            .w(px(320.))
                            .child(crate::branding::logotype("status.brand", 300., cx))
                            .child(
                                Label::new("Subroutine Lite")
                                    .debug_selector(|| "status.product-name".into())
                                    .text_lg()
                                    .text_center(),
                            )
                            .child(Label::new(title).text_lg().text_center())
                            .when_some(caption, |screen, caption| {
                                screen.child(
                                    Label::new(caption)
                                        .text_sm()
                                        .text_center()
                                        .text_color(cx.theme().colors.text_muted),
                                )
                            })
                            .children(actions),
                    ),
            )
            .when(cfg!(target_os = "windows"), |screen| {
                screen
                    .child(Self::render_windows_titlebar(px(0.), true))
                    .child(Self::render_windows_title(px(10.)))
            })
            .child(self.toasts.clone())
            .into_any_element()
    }

    fn authentication_button(&self, action: AuthenticationScreenAction) -> Button {
        match action {
            AuthenticationScreenAction::SignIn => Button::new("authentication.sign-in")
                .primary()
                .label("Sign In")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(SignIn), cx)),
            AuthenticationScreenAction::SignOut => Button::new("authentication.sign-out")
                .label("Sign Out")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(SignOut), cx)),
        }
    }

    fn local_data_recovery_buttons(&self, retrying: bool, detail: &str) -> Vec<Button> {
        vec![
            Button::new("local-data.retry")
                .primary()
                .label(if retrying { "Retrying…" } else { "Retry" })
                .disabled(retrying)
                .when(!retrying, |button| {
                    button.on_click(|_, _, cx| {
                        AppDatabaseStore::global(cx)
                            .update(cx, |store, cx| store.retry_local_data(cx));
                    })
                }),
            Button::new("local-data.show-folder")
                .label("Show Data Folder")
                .on_click(|_, window, cx| {
                    let result = AppDatabaseStore::local_data_location().and_then(|path| {
                        std::fs::create_dir_all(&path)
                            .map_err(|error| format!("create {}: {error}", path.display()))?;
                        open::that(&path)
                            .map_err(|error| format!("open {}: {error}", path.display()))
                    });
                    if let Err(error) = result {
                        tracing::warn!(%error, "could not show the data folder");
                        toast::push(
                            window,
                            cx,
                            timed_toast(
                                "local-data.show-folder.failed",
                                "Couldn’t show the data folder.",
                            )
                            .tone(Tone::Warning),
                        );
                    }
                }),
            Button::new("local-data.error-details")
                .ghost()
                .icon(AppIcon::Info)
                .label("Details")
                .tooltip(detail.to_owned()),
        ]
    }
}
