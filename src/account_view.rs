use freya::prelude::*;

use freya_components::input::InputColorsThemePartial;

use crate::{
    data::FOLDERS,
    icons::*,
    state::AppState,
    theme::DARK,
    widgets::{ProfileAvatar, divider, icon},
};

const GMAIL_LOGO: &[u8] = include_bytes!("../assets/provider-logos/gmail.svg");
const OUTLOOK_LOGO: &[u8] = include_bytes!("../assets/provider-logos/outlook.svg");
const ICLOUD_LOGO: &[u8] = include_bytes!("../assets/provider-logos/icloud.svg");
const YAHOO_LOGO: &[u8] = include_bytes!("../assets/provider-logos/yahoo.svg");
const FASTMAIL_LOGO: &[u8] = include_bytes!("../assets/provider-logos/fastmail.svg");

/// Multi-step account setup flow, inspired by SubspaceMail's wizard-style onboarding.
///
/// Steps:
/// 1. **Account** — email address, display name, and password or Google sign-in
/// 2. **Server** — IMAP host and port (993 for TLS, 143 for STARTTLS)
/// 3. **Verify** — review settings and connect
///
/// Gmail uses Google's installed-app OAuth flow and XOAUTH2 for IMAP and SMTP.

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SetupStep {
    #[default]
    Account,
    Server,
    Verify,
}

impl SetupStep {
    fn label(self) -> &'static str {
        match self {
            SetupStep::Account => "Account",
            SetupStep::Server => "Server",
            SetupStep::Verify => "Verify",
        }
    }

    fn icon_bytes(self) -> &'static [u8] {
        match self {
            SetupStep::Account => MAIL,
            SetupStep::Server => SETTINGS,
            SetupStep::Verify => SEARCH,
        }
    }

    fn index(self) -> usize {
        match self {
            SetupStep::Account => 0,
            SetupStep::Server => 1,
            SetupStep::Verify => 2,
        }
    }

    fn all() -> [SetupStep; 3] {
        [SetupStep::Account, SetupStep::Server, SetupStep::Verify]
    }
}

#[derive(PartialEq)]
pub struct AddAccountView {
    pub state: AppState,
}

impl Component for AddAccountView {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let demo = state.store.read().demo;
        let error = state.store.read().add_error.clone();

        // Form state
        let email = use_state(String::new);
        let account_name = use_state(String::new);
        let sender_name = use_state(String::new);
        let password = use_state(String::new);
        let gmail_selected = use_state(|| false);
        let gmail_authenticating = use_state(|| false);
        let gmail_auth_status = use_state(String::new);
        let host = use_state(String::new);
        let port = use_state(|| "993".to_string());
        let smtp_host = use_state(String::new);
        let smtp_port = use_state(|| "587".to_string());
        let mut working = use_state(|| false);
        let current_step = use_state(|| SetupStep::Account);
        let show_provider_picker = use_state(|| true);

        // Auto-detect status: tracks the async DNS/autoconfig discovery
        let detecting = use_state(|| false);
        let detect_status = use_state(|| String::new());

        // A failure from the worker re-enables the button.
        if error.is_some() && *working.peek() {
            working.set(false);
        }

        // Runs discovery away from the UI executor and only returns results to Freya state.
        let discover = {
            let email = email.clone();
            let host = host.clone();
            let port = port.clone();
            let smtp_host = smtp_host.clone();
            let smtp_port = smtp_port.clone();
            let mut detecting = detecting.clone();
            let mut detect_status = detect_status.clone();
            move |force: bool| {
                if !force && !host.read().is_empty() && !smtp_host.read().is_empty() {
                    return;
                }
                let addr = email.read().trim().to_string();
                detecting.set(true);
                detect_status.set("Looking up server settings…".to_string());

                let result = crate::mail::discovery::discover_in_background(addr);
                let mut result_host = host.clone();
                let mut result_port = port.clone();
                let mut result_smtp_host = smtp_host.clone();
                let mut result_smtp_port = smtp_port.clone();
                let mut result_detecting = detecting.clone();
                let mut result_status = detect_status.clone();
                freya::prelude::spawn(async move {
                    let result = result.recv().await.unwrap_or_else(|_| {
                        crate::mail::discovery::DiscoveryResult::Failed(
                            "Server discovery could not be started.".to_string(),
                        )
                    });
                    match result {
                        crate::mail::discovery::DiscoveryResult::Found(s) => {
                            if force || result_host.read().is_empty() {
                                result_host.set(s.imap_host);
                                result_port.set(s.imap_port.to_string());
                            }
                            if force || result_smtp_host.read().is_empty() {
                                result_smtp_host.set(s.smtp_host);
                                result_smtp_port.set(s.smtp_port.to_string());
                            }
                            result_status
                                .set("✓ Server settings detected automatically".to_string());
                        }
                        crate::mail::discovery::DiscoveryResult::Failed(msg) => {
                            result_status.set(format!("✗ {msg}"));
                        }
                    }
                    result_detecting.set(false);
                });
            }
        };

        let mut col = rect()
            .width(Size::fill())
            .height(Size::fill())
            .padding(16.)
            .spacing(16.)
            .child(
                rect()
                    .width(Size::fill())
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(12.)
                    .child(
                        label()
                            .text("Add a mailbox")
                            .font_size(20.)
                            .font_weight(FontWeight::BOLD)
                            .color(DARK.text),
                    ),
            );

        if demo {
            return col.child(
                label()
                    .text("Demo mode: accounts are disabled. Run without --demo to add one.")
                    .color(DARK.muted),
            );
        }

        if !*show_provider_picker.read() {
            col = col.child(StepIndicator {
                current: *current_step.read(),
            });
        }

        // Step content (scrollable so buttons aren't cut off)
        let step_content = if *show_provider_picker.read() {
            let mut show_picker = show_provider_picker.clone();
            let other_handler = EventHandler::new(move |_| show_picker.set(false));
            rect()
                .spacing(14.)
                .child(
                    label()
                        .text("Choose your email provider to get started.")
                        .color(DARK.muted)
                        .font_size(14.),
                )
                .child(
                    rect()
                        .spacing(10.)
                        .child(provider_choice(
                            "Gmail",
                            GMAIL_LOGO,
                            true,
                            Some({
                                let mut show_picker = show_provider_picker.clone();
                                let mut gmail_selected = gmail_selected.clone();
                                let mut host = host.clone();
                                let mut port = port.clone();
                                let mut smtp_host = smtp_host.clone();
                                let mut smtp_port = smtp_port.clone();
                                EventHandler::new(move |_| {
                                    gmail_selected.set(true);
                                    host.set("imap.gmail.com".to_string());
                                    port.set("993".to_string());
                                    smtp_host.set("smtp.gmail.com".to_string());
                                    smtp_port.set("465".to_string());
                                    show_picker.set(false);
                                })
                            }),
                        ))
                        .child(provider_choice("Outlook", OUTLOOK_LOGO, false, None))
                        .child(provider_choice("iCloud", ICLOUD_LOGO, false, None))
                        .child(provider_choice("Yahoo", YAHOO_LOGO, false, None))
                        .child(provider_choice("Fastmail", FASTMAIL_LOGO, false, None))
                        .child(provider_choice(
                            "Other (IMAP / SMTP)",
                            MAIL,
                            true,
                            Some(other_handler),
                        )),
                )
        } else {
            match *current_step.read() {
                SetupStep::Account => {
                    let mut fields = rect()
                        .spacing(12.)
                        .child(
                            label()
                                .text("Enter your email details to get started.")
                                .color(DARK.muted)
                                .font_size(13.),
                        )
                        .child(AddAccountFormField {
                            label: "Account name",
                            placeholder: "Work Email (shown in sidebar)",
                            value: account_name.clone(),
                            secret: false,
                        })
                        .child(AddAccountFormField {
                            label: "Sender name",
                            placeholder: "John Doe (shown to recipients)",
                            value: sender_name.clone(),
                            secret: false,
                        });
                    if *gmail_selected.read() {
                        fields = fields
                        .child(label().text("Sign in securely with Google. Your password is never shared with Photon Mail.").color(DARK.muted).font_size(12.))
                        .child(label().text(if email.read().is_empty() {
                            "Google account: not connected".to_string()
                        } else {
                            format!("Google account: {}", email.read())
                        }).color(DARK.text).font_size(13.))
                        .child({
                            let mut gmail_authenticating = gmail_authenticating.clone();
                            let mut gmail_auth_status = gmail_auth_status.clone();
                            let email = email.clone();
                            let password = password.clone();
                            rect()
                                .padding((8., 14., 8., 14.))
                                .corner_radius(6.)
                                .background(crate::grain::hover())
                                .cursor(if *gmail_authenticating.read() {
                                    freya::winit::window::CursorIcon::Default
                                } else {
                                    freya::winit::window::CursorIcon::Pointer
                                })
                                .on_press(move |_| {
                                    if *gmail_authenticating.peek() {
                                        return;
                                    }
                                    gmail_authenticating.set(true);
                                    gmail_auth_status.set("Waiting for Google sign-in…".to_string());
                                    let receiver = crate::mail::google_oauth::sign_in_in_background();
                                    let mut gmail_authenticating = gmail_authenticating.clone();
                                    let mut gmail_auth_status = gmail_auth_status.clone();
                                    let mut email = email.clone();
                                    let mut password = password.clone();
                                    freya::prelude::spawn(async move {
                                        let result = receiver.recv().await.unwrap_or_else(|_| {
                                            Err("Google sign-in stopped unexpectedly.".to_string())
                                        });
                                        match result {
                                            Ok(account) => {
                                                email.set(account.email.clone());
                                                password.set(format!(
                                                    "oauth2-refresh:{}",
                                                    account.refresh_token
                                                ));
                                                gmail_auth_status.set(format!(
                                                    "Signed in as {}.",
                                                    account.email
                                                ));
                                            }
                                            Err(error) => gmail_auth_status.set(error),
                                        }
                                        gmail_authenticating.set(false);
                                    });
                                })
                                .child(label().text(if *gmail_authenticating.read() {
                                    "Signing in…"
                                } else {
                                    "Sign in with Google"
                                }).color(DARK.text).font_size(13.))
                        })
                        .child(label().text(gmail_auth_status.read().clone()).color(
                            if password.read().starts_with("oauth2-refresh:") {
                                DARK.success
                            } else {
                                DARK.muted
                            },
                        ).font_size(11.));
                    } else {
                        fields = fields
                        .child(AddAccountFormField {
                            label: "Email address",
                            placeholder: "you@example.com",
                            value: email.clone(),
                            secret: false,
                        })
                        .child(AddAccountFormField {
                            label: "Password or OAuth access token",
                            placeholder: "Password, app password, or OAuth token",
                            value: password.clone(),
                            secret: true,
                        })
                        .child(label().text("Prefix OAuth access tokens with oauth2: — Photon Mail does not refresh these tokens.").color(DARK.muted).font_size(11.));
                    }
                    fields
                }
                SetupStep::Server => {
                    rect().spacing(12.)
                    .child(label().text("Configure your IMAP server settings.").color(DARK.muted).font_size(13.))
                    .child(AddAccountFormField {
                        label: "IMAP server",
                        placeholder: "imap.example.com",
                        value: host.clone(),
                        secret: false,
                    })
                    .child(AddAccountFormField {
                        label: "IMAP port (993 TLS, 143 STARTTLS)",
                        placeholder: "993",
                        value: port.clone(),
                        secret: false,
                    })
                    .child(label().text("Configure your SMTP (sending) server settings.").color(DARK.muted).font_size(13.))
                    .child(AddAccountFormField {
                        label: "SMTP server",
                        placeholder: "smtp.example.com",
                        value: smtp_host.clone(),
                        secret: false,
                    })
                    .child(AddAccountFormField {
                        label: "SMTP port (465 = SSL/TLS, 587 = STARTTLS)",
                        placeholder: "587",
                        value: smtp_port.clone(),
                        secret: false,
                    })
                    .child({
                        // Auto-detect button
                        let mut detect = discover.clone();
                        let is_detecting = detecting.clone();
                        rect()
                            .padding((6., 14., 6., 14.))
                            .corner_radius(6.)
                            .background(crate::grain::hover())
                            .cursor(if *is_detecting.read() {
                                freya::winit::window::CursorIcon::Default
                            } else {
                                freya::winit::window::CursorIcon::Pointer
                            })
                            .opacity(if *is_detecting.read() { 0.6 } else { 1.0 })
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .on_press(move |_| {
                                if !*is_detecting.read() {
                                    detect(true);
                                }
                            })
                            .child(icon(SEARCH, 14., DARK.muted))
                            .child(label().text(if *is_detecting.read() { "Detecting…" } else { "Auto-detect server settings" }).color(DARK.muted).font_size(12.))
                    })
                    .child({
                        // Status message for auto-detect
                        let status = detect_status.clone();
                        let is_detecting = detecting.clone();
                        rect()
                            .padding(10.)
                            .corner_radius(8.)
                            .background(DARK.panel)
                            .child(
                                rect()
                                    .horizontal()
                                    .cross_align(Alignment::Center)
                                    .spacing(8.)
                                    .child(icon(SETTINGS, 14., DARK.muted))
                                    .child(label().text({
                                        let s = status.read().clone();
                                        if *is_detecting.read() {
                                            "Looking up DNS records and provider settings…".to_string()
                                        } else if s.is_empty() {
                                            "Click to look up server settings for your email provider.".to_string()
                                        } else {
                                            s
                                        }
                                    }).color(DARK.muted).font_size(12.))
                            )
                    })
                }
                SetupStep::Verify => {
                    let addr = email.read().clone();
                    let acct_nm = account_name.read().clone();
                    let sndr_nm = sender_name.read().clone();
                    let srv = host.read().clone();
                    let p = port.read().clone();
                    let smtp_srv =
                        format!("{}:{}", smtp_host.read().trim(), smtp_port.read().trim());
                    let acct_display = if acct_nm.is_empty() {
                        addr.split('@').next().unwrap_or(&addr).to_string()
                    } else {
                        acct_nm
                    };
                    let sndr_display = if sndr_nm.is_empty() {
                        acct_display.clone()
                    } else {
                        sndr_nm
                    };

                    rect()
                        .spacing(12.)
                        .child(
                            label()
                                .text("Review your settings before connecting.")
                                .color(DARK.muted)
                                .font_size(13.),
                        )
                        .child(
                            rect()
                                .padding(14.)
                                .corner_radius(8.)
                                .background(DARK.panel)
                                .spacing(10.)
                                .child(VerifyRow {
                                    label: "Account",
                                    value: addr.clone(),
                                })
                                .child(VerifyRow {
                                    label: "Account name",
                                    value: acct_display,
                                })
                                .child(VerifyRow {
                                    label: "Sender name",
                                    value: sndr_display,
                                })
                                .child(VerifyRow {
                                    label: "IMAP server",
                                    value: format!("{}:{}", srv, p),
                                })
                                .child(VerifyRow {
                                    label: "SMTP server",
                                    value: smtp_srv,
                                })
                                .child(VerifyRow {
                                    label: "Auth",
                                    value: if *gmail_selected.read() {
                                        "Google OAuth 2.0".to_string()
                                    } else {
                                        "Password / OAuth token".to_string()
                                    },
                                }),
                        )
                }
            }
        };
        if *show_provider_picker.read() {
            col = col.child(step_content);
        } else {
            col = col.child(
                ScrollView::new()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .child(step_content),
            );
        }

        // Error / working status
        if let Some(e) = error {
            col = col.child(
                rect()
                    .padding(10.)
                    .corner_radius(8.)
                    .background(Fill::from(DARK.danger_bg))
                    .child(label().text(e).color(DARK.danger).font_size(13.)),
            );
        }
        if *working.read() {
            col = col.child(
                label()
                    .text("Connecting and checking your login…")
                    .color(DARK.muted)
                    .font_size(13.),
            );
        }

        // Navigation buttons
        let can_next = match *current_step.read() {
            SetupStep::Account => {
                email.read().contains('@')
                    && if *gmail_selected.read() {
                        password.read().starts_with("oauth2-refresh:")
                    } else {
                        !password.read().is_empty()
                    }
            }
            SetupStep::Server => host.read().trim().len() > 0 && smtp_host.read().trim().len() > 0,
            SetupStep::Verify => true,
        };

        // Action button (Next or Connect)
        let action_button = if *current_step.read() == SetupStep::Verify {
            let (email, password, host, port) =
                (email.clone(), password.clone(), host.clone(), port.clone());
            let (smtp_host, smtp_port) = (smtp_host.clone(), smtp_port.clone());
            let account_name = account_name.clone();
            let sender_name = sender_name.clone();
            let btn_handler = {
                let mut state = state;
                EventHandler::new(move |_| {
                    if *working.peek() {
                        return;
                    }
                    let addr = email.read().trim().to_string();
                    let server = host.read().trim().to_string();
                    let port_num: u16 = port.read().trim().parse().unwrap_or(993);
                    let pw = password.read().clone();
                    let acct_nm = account_name.read().trim().to_string();
                    let sndr_nm = sender_name.read().trim().to_string();
                    if !addr.contains('@') || server.is_empty() || pw.is_empty() {
                        state.store.write().add_error =
                            Some("Enter your email, password and IMAP server.".to_string());
                        return;
                    }
                    state.store.write().add_error = None;
                    working.set(true);
                    let smtp_host = smtp_host.read().trim().to_string();
                    let smtp_port: u16 = smtp_port.read().trim().parse().unwrap_or(587);
                    let name_val = if acct_nm.is_empty() {
                        addr.split('@').next().unwrap_or(&addr).to_string()
                    } else {
                        acct_nm
                    };
                    let sender_name_val = if sndr_nm.is_empty() {
                        name_val.clone()
                    } else {
                        sndr_nm
                    };
                    let cfg = crate::mail::AccountConfig {
                        id: crate::mail::config::new_id(),
                        name: name_val,
                        color: None,
                        sender_name: sender_name_val,
                        email: addr.clone(),
                        username: addr,
                        host: server,
                        port: port_num,
                        smtp_host,
                        smtp_port,
                    };
                    state
                        .engine
                        .peek()
                        .send(crate::mail::Command::AddAccount { cfg, password: pw });
                })
            };
            rect()
                .padding((8., 18., 8., 18.))
                .corner_radius(6.)
                .background(Fill::from(DARK.accent))
                .cursor(freya::winit::window::CursorIcon::Pointer)
                .on_press(btn_handler)
                .child(
                    label()
                        .text(if *working.read() {
                            "Connecting…"
                        } else {
                            "Connect"
                        })
                        .color(DARK.accent_text)
                        .font_size(13.)
                        .font_weight(FontWeight::BOLD),
                )
        } else {
            let mut step = current_step;
            let mut start_discovery = discover.clone();
            rect()
                .padding((8., 18., 8., 18.))
                .corner_radius(6.)
                .background(if can_next {
                    Fill::from(DARK.accent)
                } else {
                    Fill::from(DARK.hover_strong)
                })
                .opacity(if can_next { 1. } else { 0.5 })
                .cursor(freya::winit::window::CursorIcon::Pointer)
                .on_press(move |_| {
                    if can_next {
                        if *step.read() == SetupStep::Account {
                            start_discovery(false);
                        }
                        let next = std::cmp::min(step.read().index() + 1, 2);
                        step.set(SetupStep::all()[next]);
                    }
                })
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(6.)
                .child(
                    label()
                        .text("Next")
                        .color(if can_next {
                            DARK.accent_text
                        } else {
                            DARK.text
                        })
                        .font_size(13.)
                        .font_weight(FontWeight::BOLD),
                )
                .child(icon(
                    CHEVRON_RIGHT,
                    14.,
                    if can_next {
                        DARK.accent_text
                    } else {
                        DARK.text
                    },
                ))
        };

        let nav_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .spacing(8.)
            .child({
                if *current_step.read() != SetupStep::Account {
                    let mut step = current_step;
                    rect()
                        .padding((8., 14., 8., 14.))
                        .corner_radius(6.)
                        .background(crate::grain::hover())
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(6.)
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |_| {
                            let prev = step.read().index().saturating_sub(1);
                            step.set(SetupStep::all()[prev]);
                        })
                        .child(icon(CHEVRON_LEFT, 14., DARK.muted))
                        .child(label().text("Back").color(DARK.muted).font_size(13.))
                } else {
                    rect()
                }
            })
            .child(rect().width(Size::flex(1.)))
            .child({
                let s_back = state;
                rect()
                    .padding((8., 14., 8., 14.))
                    .corner_radius(6.)
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(move |_| s_back.set_view(crate::state::View::Mail))
                    .child(label().text("Cancel").color(DARK.muted).font_size(13.))
            })
            .child(action_button);

        if !*show_provider_picker.read() {
            col = col.child(nav_row);
        }
        col
    }
}

/// Edit an existing account's name, email, IMAP server, and SMTP settings.
#[derive(PartialEq)]
pub struct EditAccountView {
    pub state: AppState,
    pub index: usize,
}

impl Component for EditAccountView {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state;
        let index = self.index;

        // Pre-fill from the persisted config (has SMTP fields)
        let persisted_cfg = {
            let configs = crate::mail::config::load_accounts();
            configs.into_iter().nth(index)
        };
        let Some(cfg) = persisted_cfg else {
            return rect().child(label().text("Account not found").color(DARK.danger));
        };

        let email = use_state(|| cfg.email.clone());
        let account_name = use_state(|| cfg.name.clone());
        let sender_name = use_state(|| cfg.sender_name.clone());
        let host = use_state(|| cfg.host.clone());
        let port = use_state(|| cfg.port.to_string());
        let smtp_host = use_state(|| cfg.smtp_host.clone());
        let smtp_port = use_state(|| cfg.smtp_port.to_string());

        // Auto-detect status for Edit Account
        let edit_detecting = use_state(|| false);
        let edit_detect_status = use_state(|| String::new());
        let gmail_authenticating = use_state(|| false);
        let gmail_auth_status = use_state(String::new);

        let mut col = rect()
            .width(Size::fill())
            .height(Size::fill())
            .padding(16.)
            .spacing(16.)
            .child(
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(12.)
                    .child(icon(MAIL, 24., DARK.accent))
                    .child(
                        label()
                            .text("Edit Account")
                            .color(DARK.text)
                            .font_size(20.)
                            .font_weight(FontWeight::BOLD),
                    ),
            );

        // Form fields (scrollable so buttons aren't cut off on small windows)
        col = col.child(
            ScrollView::new()
                .width(Size::fill())
                .height(Size::flex(1.))
                .child(
                    rect()
                        .width(Size::fill())
                        .spacing(12.)
                        .child(label().text("Account details").color(DARK.muted).font_size(13.))
                        .child(AddAccountFormField {
                            label: "Email address",
                            placeholder: "you@example.com",
                            value: email.clone(),
                            secret: false,
                        })
                        .child(AddAccountFormField {
                            label: "Account name",
                            placeholder: "Work Email (shown in sidebar)",
                            value: account_name.clone(),
                            secret: false,
                        })
                        .child(AddAccountFormField {
                            label: "Sender name",
                            placeholder: "John Doe (shown to recipients)",
                            value: sender_name.clone(),
                            secret: false,
                        })
                        .child(label().text("IMAP server settings").color(DARK.muted).font_size(13.))
                        .child(AddAccountFormField {
                            label: "IMAP server",
                            placeholder: "imap.example.com",
                            value: host.clone(),
                            secret: false,
                        })
                        .child(
                            rect()
                                .horizontal()
                                .spacing(8.)
                                .child(
                                    rect().width(Size::flex(1.)).child(AddAccountFormField {
                                        label: "IMAP port (993 TLS, 143 STARTTLS)",
                                        placeholder: "993",
                                        value: port.clone(),
                                        secret: false,
                                    })
                                )
                        )
                        .child(label().text("SMTP (sending) server settings").color(DARK.muted).font_size(13.))
                        .child(AddAccountFormField {
                            label: "SMTP server",
                            placeholder: "smtp.example.com",
                            value: smtp_host.clone(),
                            secret: false,
                        })
                        .child(AddAccountFormField {
                            label: "SMTP port (465 = SSL/TLS, 587 = STARTTLS)",
                            placeholder: "587",
                            value: smtp_port.clone(),
                            secret: false,
                        })
                        .child({
                            // Auto-detect button for Edit Account — supports async DNS discovery
                            let email = email.clone();
                            let host = host.clone();
                            let port = port.clone();
                            let smtp_host = smtp_host.clone();
                            let smtp_port = smtp_port.clone();
                            let mut is_detecting = edit_detecting.clone();
                            let mut detect_status = edit_detect_status.clone();
                            rect()
                                .padding((6., 14., 6., 14.))
                                .corner_radius(6.)
                                .background(crate::grain::hover())
                                .cursor(if *is_detecting.read() {
                                    freya::winit::window::CursorIcon::Default
                                } else {
                                    freya::winit::window::CursorIcon::Pointer
                                })
                                .opacity(if *is_detecting.read() { 0.6 } else { 1.0 })
                                .horizontal()
                                .cross_align(Alignment::Center)
                                .spacing(6.)
                                .on_press(move |_| {
                                    if *is_detecting.read() {
                                        return;
                                    }
                                    let addr = email.read().trim().to_string();
                                    is_detecting.set(true);
                                    detect_status.set("Looking up server settings…".to_string());
                                    let result = crate::mail::discovery::discover_in_background(addr);
                                    let mut h = host.clone();
                                    let mut p = port.clone();
                                    let mut sh = smtp_host.clone();
                                    let mut sp = smtp_port.clone();
                                    let mut det = is_detecting.clone();
                                    let mut ds = detect_status.clone();
                                    freya::prelude::spawn(async move {
                                        let result = result.recv().await.unwrap_or_else(|_| {
                                            crate::mail::discovery::DiscoveryResult::Failed(
                                                "Server discovery could not be started.".to_string(),
                                            )
                                        });
                                        match result {
                                            crate::mail::discovery::DiscoveryResult::Found(s) => {
                                                h.set(s.imap_host);
                                                p.set(s.imap_port.to_string());
                                                sh.set(s.smtp_host);
                                                sp.set(s.smtp_port.to_string());
                                                ds.set("✓ Server settings detected automatically".to_string());
                                            }
                                            crate::mail::discovery::DiscoveryResult::Failed(msg) => {
                                                ds.set(format!("✗ {msg}"));
                                            }
                                        }
                                        det.set(false);
                                    });
                                })
                                .child(icon(SEARCH, 14., DARK.muted))
                                .child(label().text(if *is_detecting.read() { "Detecting…" } else { "Auto-detect server settings" }).color(DARK.muted).font_size(12.))
                        })
                        .child({
                            // Status message for Edit Account auto-detect
                            let status = edit_detect_status.clone();
                            let is_detecting = edit_detecting.clone();
                            rect()
                                .padding(10.)
                                .corner_radius(8.)
                                .background(DARK.panel)
                                .child(
                                    rect()
                                        .horizontal()
                                        .cross_align(Alignment::Center)
                                        .spacing(8.)
                                        .child(icon(SETTINGS, 14., DARK.muted))
                                        .child(label().text({
                                            let s = status.read().clone();
                                            if *is_detecting.read() {
                                                "Looking up DNS records and provider settings…".to_string()
                                            } else if s.is_empty() {
                                                "Click to look up server settings for your email provider.".to_string()
                                            } else {
                                                s
                                            }
                                        }).color(DARK.muted).font_size(12.))
                                )
                        })
                )
        );

        if cfg.host.eq_ignore_ascii_case("imap.gmail.com") {
            col = col.child(
                rect()
                    .spacing(8.)
                    .child(label().text("Google sign-in").color(DARK.muted).font_size(13.))
                    .child(
                        label()
                            .text("Reconnect with Google to replace the saved refresh token. Restart Photon Mail afterward.")
                            .color(DARK.muted)
                            .font_size(12.),
                    )
                    .child({
                        let mut gmail_authenticating = gmail_authenticating.clone();
                        let mut gmail_auth_status = gmail_auth_status.clone();
                        let expected_email = cfg.email.clone();
                        let account_id = cfg.id.clone();
                        rect()
                            .padding((8., 14., 8., 14.))
                            .corner_radius(6.)
                            .background(crate::grain::hover())
                            .cursor(if *gmail_authenticating.read() {
                                freya::winit::window::CursorIcon::Default
                            } else {
                                freya::winit::window::CursorIcon::Pointer
                            })
                            .on_press(move |_| {
                                if *gmail_authenticating.peek() {
                                    return;
                                }
                                gmail_authenticating.set(true);
                                gmail_auth_status.set("Waiting for Google sign-in…".to_string());
                                let receiver = crate::mail::google_oauth::sign_in_in_background();
                                let mut gmail_authenticating = gmail_authenticating.clone();
                                let mut gmail_auth_status = gmail_auth_status.clone();
                                let expected_email = expected_email.clone();
                                let account_id = account_id.clone();
                                freya::prelude::spawn(async move {
                                    let result = receiver.recv().await.unwrap_or_else(|_| {
                                        Err("Google sign-in stopped unexpectedly.".to_string())
                                    });
                                    match result {
                                        Ok(account)
                                            if !account
                                                .email
                                                .eq_ignore_ascii_case(&expected_email) =>
                                        {
                                            gmail_auth_status.set(format!(
                                                "Google returned {}, but this account is {}. No token was changed.",
                                                account.email, expected_email
                                            ));
                                        }
                                        Ok(account) => {
                                            let secret =
                                                format!("oauth2-refresh:{}", account.refresh_token);
                                            let (save_tx, save_rx) = async_channel::bounded(1);
                                            let save_thread = std::thread::Builder::new()
                                                .name("photon-mail-google-keyring".into())
                                                .spawn(move || {
                                                    let result = crate::mail::config::save_password(
                                                        &account_id,
                                                        &secret,
                                                    );
                                                    let _ = save_tx.send_blocking(result);
                                                });
                                            let saved = match save_thread {
                                                Ok(_) => save_rx.recv().await.unwrap_or_else(|_| {
                                                    Err("Keyring update stopped unexpectedly.".to_string())
                                                }),
                                                Err(error) => Err(format!(
                                                    "Couldn't start the keyring update: {error}"
                                                )),
                                            };
                                            gmail_auth_status.set(match saved {
                                                Ok(()) => "Google sign-in refreshed. Restart Photon Mail to reconnect with the new token.".to_string(),
                                                Err(error) => format!("Couldn't save the Google token to the system keyring: {error}"),
                                            });
                                        }
                                        Err(error) => gmail_auth_status.set(error),
                                    }
                                    gmail_authenticating.set(false);
                                });
                            })
                            .child(
                                label()
                                    .text(if *gmail_authenticating.read() {
                                        "Signing in…"
                                    } else {
                                        "Reconnect Google"
                                    })
                                    .color(DARK.text)
                                    .font_size(13.),
                            )
                    })
                    .child(
                        label()
                            .text(gmail_auth_status.read().clone())
                            .color(if gmail_auth_status.read().starts_with("Google sign-in refreshed") {
                                DARK.success
                            } else {
                                DARK.muted
                            })
                            .font_size(11.),
                    ),
            );
        }

        // Navigation buttons
        let nav_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .spacing(8.)
            .child({
                let s_back = state;
                rect()
                    .padding((8., 14., 8., 14.))
                    .corner_radius(6.)
                    .background(crate::grain::hover())
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(move |_| s_back.set_view(crate::state::View::Settings(index)))
                    .child(label().text("Cancel").color(DARK.muted).font_size(13.))
            })
            .child(rect().width(Size::flex(1.)))
            .child({
                let save_handler = {
                    EventHandler::new(move |_| {
                        let addr = email.read().trim().to_string();
                        let acct_nm = account_name.read().trim().to_string();
                        let sndr_nm = sender_name.read().trim().to_string();
                        let server = host.read().trim().to_string();
                        let port_num: u16 = port.read().trim().parse().unwrap_or(993);
                        let smtp_srv = smtp_host.read().trim().to_string();
                        let smtp_port_num: u16 = smtp_port.read().trim().parse().unwrap_or(587);

                        if !addr.contains('@') || server.is_empty() || smtp_srv.is_empty() {
                            return;
                        }

                        let name_val = if acct_nm.is_empty() {
                            addr.split('@').next().unwrap_or(&addr).to_string()
                        } else {
                            acct_nm
                        };
                        let sender_name_val = if sndr_nm.is_empty() {
                            name_val.clone()
                        } else {
                            sndr_nm
                        };

                        // Update the in-memory UI model
                        let mut store = state.store.write();
                        if let Some(acc) = store.accounts.get_mut(index) {
                            acc.name = name_val.clone();
                            acc.sender_name = sender_name_val.clone();
                            acc.addr = addr.clone();
                            acc.host = server.clone();
                        }
                        drop(store);

                        // Persist the full config to disk
                        let updated_cfg = crate::mail::AccountConfig {
                            id: cfg.id.clone(),
                            name: name_val,
                            color: cfg.color,
                            sender_name: sender_name_val,
                            email: addr,
                            username: cfg.username.clone(),
                            host: server,
                            port: port_num,
                            smtp_host: smtp_srv,
                            smtp_port: smtp_port_num,
                        };
                        state.update_account_config(index, updated_cfg);
                        state.set_view(crate::state::View::Settings(index));
                    })
                };
                rect()
                    .padding((8., 18., 8., 18.))
                    .corner_radius(6.)
                    .background(Fill::from(DARK.accent))
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(save_handler)
                    .child(
                        label()
                            .text("Save")
                            .color(DARK.accent_text)
                            .font_size(13.)
                            .font_weight(FontWeight::BOLD),
                    )
            });

        col = col.child(nav_row);
        col
    }
}

/// Step progress indicator bar (Account → Server → Verify)
#[derive(PartialEq, Clone)]
struct StepIndicator {
    current: SetupStep,
}

impl Component for StepIndicator {
    fn render(&self) -> impl IntoElement {
        let current = self.current;
        let current_idx = current.index();

        let mut items = rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(4.);

        for (i, step) in SetupStep::all().into_iter().enumerate() {
            let is_active = i == current_idx;
            let is_completed = i < current_idx;

            if i > 0 {
                // Connector line
                items = items.child(rect().width(Size::px(20.)).height(Size::px(2.)).background(
                    if i <= current_idx {
                        Fill::from(DARK.accent)
                    } else {
                        Fill::from(DARK.divider)
                    },
                ));
            }

            let bg = if is_active {
                Fill::from(DARK.selected)
            } else if is_completed {
                Fill::from(DARK.hover)
            } else {
                crate::grain::hover()
            };
            let text_color = if is_active || is_completed {
                DARK.text
            } else {
                DARK.muted
            };

            items = items.child(
                rect()
                    .padding((4., 10., 4., 10.))
                    .corner_radius(12.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .background(bg)
                    .child(icon(step.icon_bytes(), 12., text_color))
                    .child(
                        label()
                            .text(step.label())
                            .color(text_color)
                            .font_size(12.)
                            .font_weight(if is_active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            }),
                    ),
            );
        }

        items
    }
}

/// A labeled form field for the add-account wizard with visible dark input boxes.
#[derive(PartialEq, Clone)]
struct AddAccountFormField {
    label: &'static str,
    placeholder: &'static str,
    value: State<String>,
    secret: bool,
}

impl Component for AddAccountFormField {
    fn render(&self) -> impl IntoElement {
        let mut input = Input::new(self.value.clone())
            .placeholder(self.placeholder.to_string())
            .compact()
            .width(Size::fill())
            .theme_colors(
                InputColorsThemePartial::new()
                    .color(DARK.text)
                    .placeholder_color(DARK.muted)
                    .background(Color::from(DARK.panel))
                    .focus_background(Color::from(DARK.panel))
                    .border_fill(Color::from(DARK.divider))
                    .focus_border_fill(Color::from(DARK.accent)),
            );
        if self.secret {
            input = input.mode(InputMode::new_password());
        }
        rect()
            .width(Size::fill())
            .spacing(4.)
            .child(
                label()
                    .text(self.label)
                    .color(DARK.muted)
                    .font_size(12.)
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(input)
    }
}

/// A single row in the verify/review step.
#[derive(PartialEq, Clone)]
struct VerifyRow {
    label: &'static str,
    value: String,
}

impl Component for VerifyRow {
    fn render(&self) -> impl IntoElement {
        rect()
            .width(Size::fill())
            .horizontal()
            .spacing(12.)
            .child(
                label()
                    .text(self.label)
                    .color(DARK.muted)
                    .font_size(13.)
                    .width(Size::px(90.)),
            )
            .child(
                label()
                    .text(self.value.clone())
                    .color(DARK.text)
                    .font_size(13.),
            )
    }
}

#[derive(PartialEq)]
pub struct AccountSettings {
    pub state: AppState,
    pub index: usize,
}

impl Component for AccountSettings {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let index = self.index;
        let Some(a) = state.store.read().accounts.get(index).cloned() else {
            return rect();
        };

        rect()
            .spacing(16.)
            .child(
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(14.)
                    .child(ProfileAvatar {
                        name: a.name.clone(),
                        email: a.addr.clone(),
                        color: a.color,
                        size: 56.,
                        state,
                    })
                    .child(
                        rect()
                            .spacing(2.)
                            .child(
                                label()
                                    .text(a.name.clone())
                                    .font_size(24.)
                                    .font_weight(FontWeight::BOLD)
                                    .color(DARK.text),
                            )
                            .child(label().text(a.addr.clone()).color(DARK.muted)),
                    ),
            )
            .child(divider())
            .child(
                label()
                    .text("Account color")
                    .color(DARK.muted)
                    .font_size(13.),
            )
            .child(
                rect()
                    .horizontal()
                    .width(Size::fill())
                    .spacing(10.)
                    .content(Content::wrap_spacing(10.))
                    .children(crate::theme::ACCOUNT_COLORS.iter().map(|&color| {
                        let mut s_color = state;
                        rect()
                            .width(Size::px(24.))
                            .height(Size::px(24.))
                            .corner_radius(12.)
                            .background(Fill::from(color))
                            .border(
                                Border::new()
                                    .width(if a.color == color { 2. } else { 1. })
                                    .fill(if a.color == color {
                                        DARK.text
                                    } else {
                                        DARK.divider
                                    }),
                            )
                            .cursor(freya::winit::window::CursorIcon::Pointer)
                            .on_press(move |_| s_color.set_account_color(index, color))
                    })),
            )
            .child(field(
                "Sync status",
                &format!("● {}", a.sync_label()),
                if a.is_error() {
                    DARK.danger
                } else {
                    DARK.success
                },
            ))
            .child(field("Server", &a.host, DARK.text))
            .child(field("Folders", &FOLDERS.len().to_string(), DARK.text))
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(AccountButton {
                        text: "Sync now".to_string(),
                        bg: crate::grain::hover(),
                        hover_bg: Fill::from(DARK.hover_strong),
                        on_press: EventHandler::new(move |_| state.sync_account(index)),
                    })
                    .child({
                        let s_edit = state;
                        AccountButton {
                            text: "Edit".to_string(),
                            bg: crate::grain::hover(),
                            hover_bg: Fill::from(DARK.hover_strong),
                            on_press: EventHandler::new(move |_| {
                                s_edit.set_view(crate::state::View::EditAccount(index))
                            }),
                        }
                    })
                    .child(AccountButton {
                        text: "Remove account".to_string(),
                        bg: Fill::from(DARK.danger),
                        hover_bg: Fill::from(DARK.close_hover),
                        on_press: EventHandler::new(move |_| state.remove_account(index)),
                    }),
            )
    }
}

fn field(name: &str, value: &str, color: (u8, u8, u8)) -> Rect {
    rect()
        .horizontal()
        .spacing(12.)
        .child(label().text(format!("{name}:")).color(DARK.muted))
        .child(label().text(value.to_string()).color(color))
}

fn provider_choice(
    title: &'static str,
    logo: &'static [u8],
    enabled: bool,
    on_press: Option<EventHandler<Event<PressEventData>>>,
) -> Rect {
    let choice = rect()
        .width(Size::fill())
        .height(Size::px(68.))
        .padding((12., 16., 12., 16.))
        .corner_radius(10.)
        .background(crate::grain::panel())
        .border(Border::new().fill(DARK.divider).width(1.))
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .spacing(12.)
        .child(
            rect()
                .width(Size::px(30.))
                .height(Size::px(30.))
                .center()
                .child(
                    SvgViewer::new(logo)
                        .key(title)
                        .width(Size::px(26.))
                        .height(Size::px(26.))
                        .show_loader(false),
                ),
        )
        .child(
            rect()
                .width(Size::flex(1.))
                .spacing(2.)
                .child(
                    label()
                        .text(title)
                        .color(if enabled { DARK.text } else { DARK.muted })
                        .font_size(14.),
                )
                .child(
                    label()
                        .text(if enabled { "" } else { "Coming soon" })
                        .color(DARK.muted)
                        .font_size(10.),
                ),
        );

    if enabled {
        if let Some(handler) = on_press {
            choice
                .cursor(freya::winit::window::CursorIcon::Pointer)
                .on_press(handler)
        } else {
            choice
        }
    } else {
        choice.cursor(freya::winit::window::CursorIcon::Default)
    }
}

#[derive(PartialEq, Clone)]
struct AccountButton {
    text: String,
    bg: Fill,
    hover_bg: Fill,
    on_press: EventHandler<Event<PressEventData>>,
}

impl Component for AccountButton {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let text = self.text.clone();
        let bg_fill = self.bg.clone();
        let hover_fill = self.hover_bg.clone();
        let on_press = self.on_press.clone();
        rect()
            .padding((8., 14., 8., 14.))
            .corner_radius(6.)
            .background(if hovering() { hover_fill } else { bg_fill })
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(label().text(text).color(DARK.text).font_size(13.))
    }
}
