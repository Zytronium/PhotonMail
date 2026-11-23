use freya::prelude::*;

use freya_components::input::InputColorsThemePartial;

use crate::{
    data::FOLDERS,
    icons::*,
    state::AppState,
    theme::DARK,
    widgets::{avatar, divider, icon},
};

/// Multi-step account setup flow, inspired by SubspaceMail's wizard-style onboarding.
///
/// Steps:
/// 1. **Account** — email address, display name, and password
/// 2. **Server** — IMAP host, port, and security settings
/// 3. **Verify** — review settings and connect
///
/// Provider selection (Gmail, Outlook, iCloud) is reserved for future OAuth support;
/// only IMAP/SMTP is functional now.

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
        let host = use_state(String::new);
        let port = use_state(|| "993".to_string());
        let security = use_state(|| "SSL/TLS".to_string());
        let smtp_host = use_state(String::new);
        let smtp_port = use_state(|| "587".to_string());
        let mut working = use_state(|| false);
        let current_step = use_state(|| SetupStep::Account);

        // Auto-detect status: tracks the async DNS/autoconfig discovery
        let detecting = use_state(|| false);
        let detect_status = use_state(|| String::new());

        // A failure from the worker re-enables the button.
        if error.is_some() && *working.peek() {
            working.set(false);
        }

        // Auto-discover server settings from email domain when advancing from Account step.
        // Uses the built-in provider database (discover_settings) for known providers,
        // and falls back to heuristic guessing for unknown domains.
        let discover_host = {
            let email = email.clone();
            let mut host = host.clone();
            let mut port = port.clone();
            let mut smtp_host = smtp_host.clone();
            let mut smtp_port = smtp_port.clone();
            move || {
                if host.read().is_empty() || smtp_host.read().is_empty() {
                    let addr = email.read().trim().to_string();
                    // Try the built-in provider database first
                    if let Some(settings) = crate::mail::config::discover_settings(&addr) {
                        if host.read().is_empty() {
                            host.set(settings.imap.host.to_string());
                        }
                        if port.read().as_str() == "993" {
                            port.set(settings.imap.port.to_string());
                        }
                        if smtp_host.read().is_empty() {
                            smtp_host.set(settings.smtp.host.to_string());
                        }
                        if smtp_port.read().as_str() == "587" {
                            smtp_port.set(settings.smtp.port.to_string());
                        }
                        return;
                    }
                    // Fallback: heuristic guessing for unknown domains
                    if host.read().is_empty() {
                        if let Some(domain) = addr.split('@').nth(1) {
                            host.set(format!("imap.{domain}"));
                        }
                    }
                    if smtp_host.read().is_empty() {
                        let h = host.read().trim().to_lowercase();
                        if !h.is_empty() {
                            let suggested = if let Some(rest) = ["imap-mail.", "imap.", "imaps."]
                                .iter()
                                .find_map(|p| h.strip_prefix(p))
                            {
                                format!("smtp.{rest}")
                            } else if h.starts_with("smtp.") || h.starts_with("mail.") {
                                h.clone()
                            } else {
                                format!("smtp.{}", h.trim_start_matches("imap.").trim_start_matches("imap-").trim_start_matches("imaps."))
                            };
                            smtp_host.set(suggested);
                        }
                    }
                }
            }
        };

        // Auto-detect: triggered by the "Auto-detect" button.
        // First tries the built-in provider database (instant), then falls back to
        // async DNS/autoconfig discovery for unknown domains.
        // Uses a channel to send results back to the UI thread (Freya State is !Send).
        let auto_detect = {
            let email = email.clone();
            let mut host = host.clone();
            let mut port = port.clone();
            let mut smtp_host = smtp_host.clone();
            let mut smtp_port = smtp_port.clone();
            let mut detecting = detecting.clone();
            let mut detect_status = detect_status.clone();
            move || {
                let addr = email.read().trim().to_string();

                // Try built-in database first (instant, offline)
                if let Some(settings) = crate::mail::config::discover_settings(&addr) {
                    host.set(settings.imap.host.to_string());
                    port.set(settings.imap.port.to_string());
                    smtp_host.set(settings.smtp.host.to_string());
                    smtp_port.set(settings.smtp.port.to_string());
                    detect_status.set("✓ Settings found from provider database".to_string());
                    return;
                }

                // Not in database — run DNS/autoconfig discovery on a background thread
                // with its own Tokio runtime (hickory-resolver requires Tokio).
                detecting.set(true);
                detect_status.set("Looking up server settings…".to_string());

                let mut h = host.clone();
                let mut p = port.clone();
                let mut sh = smtp_host.clone();
                let mut sp = smtp_port.clone();
                let mut det = detecting.clone();
                let mut ds = detect_status.clone();

                freya::prelude::spawn(async move {
                    let result = crate::mail::discovery::discover_sync(&addr);
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
                    .child(label().text("Add account").font_size(20.).font_weight(FontWeight::BOLD).color(DARK.text))
            );

        if demo {
            return col.child(label().text("Demo mode: accounts are disabled. Run without --demo to add one.").color(DARK.muted));
        }

        // Step indicator
        col = col.child(StepIndicator { current: *current_step.read() });

        // Step content (scrollable so buttons aren't cut off)
        let step_content = match *current_step.read() {
            SetupStep::Account => {
                rect().spacing(12.)
                    .child(label().text("Enter your email details to get started.").color(DARK.muted).font_size(13.))
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
                    .child(AddAccountFormField {
                        label: "Password",
                        placeholder: "Enter your email password or app password",
                        value: password.clone(),
                        secret: true,
                    })
                    .child(label().text("If your provider requires it, use an app-specific password.").color(DARK.muted).font_size(11.))
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
                    .child(
                        rect()
                            .horizontal()
                            .spacing(8.)
                            .child(
                                rect().width(Size::flex(1.)).child(AddAccountFormField {
                                    label: "Port",
                                    placeholder: "993",
                                    value: port.clone(),
                                    secret: false,
                                })
                            )
                            .child(
                                rect().width(Size::flex(1.)).child(AddAccountFormField {
                                    label: "Security",
                                    placeholder: "SSL/TLS",
                                    value: security.clone(),
                                    secret: false,
                                })
                            )
                    )
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
                        let mut detect = auto_detect.clone();
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
                                    detect();
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
                            .background(Fill::from((30, 30, 46)))
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
                let smtp_srv = format!("{}:{}", smtp_host.read().trim(), smtp_port.read().trim());
                let acct_display = if acct_nm.is_empty() { addr.split('@').next().unwrap_or(&addr).to_string() } else { acct_nm };
                let sndr_display = if sndr_nm.is_empty() { acct_display.clone() } else { sndr_nm };

                rect().spacing(12.)
                    .child(label().text("Review your settings before connecting.").color(DARK.muted).font_size(13.))
                    .child(
                        rect()
                            .padding(14.)
                            .corner_radius(8.)
                            .background(Fill::from((30, 30, 46)))
                            .spacing(10.)
                            .child(VerifyRow { label: "Account", value: addr.clone() })
                            .child(VerifyRow { label: "Account name", value: acct_display })
                            .child(VerifyRow { label: "Sender name", value: sndr_display })
                            .child(VerifyRow { label: "IMAP server", value: format!("{}:{}", srv, p) })
                            .child(VerifyRow { label: "SMTP server", value: smtp_srv })
                            .child(VerifyRow { label: "Auth", value: "Password".to_string() })
                    )
            }
        };
        col = col.child(
            ScrollView::new()
                .width(Size::fill())
                .height(Size::flex(1.))
                .child(step_content)
        );

        // Error / working status
        if let Some(e) = error {
            col = col.child(
                rect()
                    .padding(10.)
                    .corner_radius(8.)
                    .background(Fill::from((60, 20, 30)))
                    .child(label().text(e).color(DARK.danger).font_size(13.))
            );
        }
        if *working.read() {
            col = col.child(label().text("Connecting and checking your login…").color(DARK.muted).font_size(13.));
        }

        // Navigation buttons
        let can_next = match *current_step.read() {
            SetupStep::Account => email.read().contains('@') && password.read().len() > 0,
            SetupStep::Server => host.read().trim().len() > 0 && smtp_host.read().trim().len() > 0,
            SetupStep::Verify => true,
        };

        // Action button (Next or Connect)
        let action_button = if *current_step.read() == SetupStep::Verify {
            let (email, password, host, port) = (email.clone(), password.clone(), host.clone(), port.clone());
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
                    let name_val = if acct_nm.is_empty() { addr.split('@').next().unwrap_or(&addr).to_string() } else { acct_nm };
                    let sender_name_val = if sndr_nm.is_empty() { name_val.clone() } else { sndr_nm };
                    let cfg = crate::mail::AccountConfig {
                        id: crate::mail::config::new_id(),
                        name: name_val,
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
                .child(label().text(if *working.read() { "Connecting…" } else { "Connect" }).color(DARK.text).font_size(13.).font_weight(FontWeight::BOLD))
        } else {
            let mut step = current_step;
            let mut discover = discover_host.clone();
            rect()
                .padding((8., 18., 8., 18.))
                .corner_radius(6.)
                .background(if can_next { Fill::from(DARK.accent) } else { Fill::from((50, 50, 68)) })
                .opacity(if can_next { 1. } else { 0.5 })
                .cursor(freya::winit::window::CursorIcon::Pointer)
                .on_press(move |_| {
                    if can_next {
                        discover();
                        let next = std::cmp::min(step.read().index() + 1, 2);
                        step.set(SetupStep::all()[next]);
                    }
                })
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(6.)
                .child(label().text("Next").color(DARK.text).font_size(13.).font_weight(FontWeight::BOLD))
                .child(icon(CHEVRON_RIGHT, 14., DARK.text))
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
                let mut s_back = state;
                rect()
                    .padding((8., 14., 8., 14.))
                    .corner_radius(6.)
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(move |_| s_back.view.set(crate::state::View::Mail))
                    .child(label().text("Cancel").color(DARK.muted).font_size(13.))
            })
            .child(action_button);

        col = col.child(nav_row);
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
                    .child(label().text("Edit Account").color(DARK.text).font_size(20.).font_weight(FontWeight::BOLD))
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
                                        label: "Port",
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
                            let mut host = host.clone();
                            let mut port = port.clone();
                            let mut smtp_host = smtp_host.clone();
                            let mut smtp_port = smtp_port.clone();
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
                                    // Try built-in database first
                                    if let Some(settings) = crate::mail::config::discover_settings(&addr) {
                                        host.set(settings.imap.host.to_string());
                                        port.set(settings.imap.port.to_string());
                                        smtp_host.set(settings.smtp.host.to_string());
                                        smtp_port.set(settings.smtp.port.to_string());
                                        detect_status.set("✓ Settings found from provider database".to_string());
                                        return;
                                    }
                                    // DNS/autoconfig discovery — runs on a background thread with its own
                                    // Tokio runtime (hickory-resolver requires Tokio).
                                    is_detecting.set(true);
                                    detect_status.set("Looking up server settings…".to_string());
                                    let mut h = host.clone();
                                    let mut p = port.clone();
                                    let mut sh = smtp_host.clone();
                                    let mut sp = smtp_port.clone();
                                    let mut det = is_detecting.clone();
                                    let mut ds = detect_status.clone();
                                    freya::prelude::spawn(async move {
                                        let result = crate::mail::discovery::discover_sync(&addr);
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
                                .background(Fill::from((30, 30, 46)))
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

        // Navigation buttons
        let nav_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .spacing(8.)
            .child({
                let mut s_back = state;
                rect()
                    .padding((8., 14., 8., 14.))
                    .corner_radius(6.)
                    .background(crate::grain::hover())
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(move |_| s_back.view.set(crate::state::View::Settings(index)))
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
                            sender_name: sender_name_val,
                            email: addr,
                            username: cfg.username.clone(),
                            host: server,
                            port: port_num,
                            smtp_host: smtp_srv,
                            smtp_port: smtp_port_num,
                        };
                        state.update_account_config(index, updated_cfg);
                        state.view.set(crate::state::View::Settings(index));
                    })
                };
                rect()
                    .padding((8., 18., 8., 18.))
                    .corner_radius(6.)
                    .background(Fill::from(DARK.accent))
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(save_handler)
                    .child(label().text("Save").color(DARK.text).font_size(13.).font_weight(FontWeight::BOLD))
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

        let mut items = rect().horizontal().spacing(4.);

        for (i, step) in SetupStep::all().into_iter().enumerate() {
            let is_active = i == current_idx;
            let is_completed = i < current_idx;

            if i > 0 {
                // Connector line
                items = items.child(
                    rect()
                        .width(Size::px(20.))
                        .height(Size::px(2.))
                        .background(if i <= current_idx { Fill::from(DARK.accent) } else { Fill::from(DARK.divider) })
                );
            }

            let bg = if is_active {
                Fill::from((60, 45, 130))
            } else if is_completed {
                Fill::from((40, 40, 60))
            } else {
                crate::grain::hover()
            };
            let text_color = if is_active || is_completed { DARK.text } else { DARK.muted };

            items = items.child(
                rect()
                    .padding((4., 10., 4., 10.))
                    .corner_radius(12.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .background(bg)
                    .child(icon(step.icon_bytes(), 12., text_color))
                    .child(label().text(step.label()).color(text_color).font_size(12.).font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL }))
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
                    .background(Color::from((30, 30, 46)))
                    .focus_background(Color::from((30, 30, 46)))
                    .border_fill(Color::from((50, 50, 70)))
                    .focus_border_fill(Color::from(DARK.accent)),
            );
        if self.secret {
            input = input.mode(InputMode::new_password());
        }
        rect()
            .width(Size::fill())
            .spacing(4.)
            .child(label().text(self.label).color(DARK.muted).font_size(12.).font_weight(FontWeight::MEDIUM))
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
            .child(label().text(self.label).color(DARK.muted).font_size(13.).width(Size::px(90.)))
            .child(label().text(self.value.clone()).color(DARK.text).font_size(13.))
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
                    .child(avatar(&a.name, a.color, 56.))
                    .child(
                        rect()
                            .spacing(2.)
                            .child(label().text(a.name.clone()).font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text))
                            .child(label().text(a.addr.clone()).color(DARK.muted)),
                    ),
            )
            .child(divider())
            .child(field(
                "Sync status",
                &format!("● {}", a.sync_label()),
                if a.is_error() { DARK.danger } else { DARK.success },
            ))
            .child(field("Server", &a.host, DARK.text))
            .child(field("Folders", &FOLDERS.len().to_string(), DARK.text))
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(AccountButton { text: "Sync now".to_string(), bg: crate::grain::hover(), hover_bg: Fill::from((45, 45, 62)), on_press: EventHandler::new(move |_| state.sync_account(index)) })
                    .child({
                        let mut s_edit = state;
                        AccountButton { text: "Edit".to_string(), bg: crate::grain::hover(), hover_bg: Fill::from((45, 45, 62)), on_press: EventHandler::new(move |_| s_edit.view.set(crate::state::View::EditAccount(index))) }
                    })
                    .child(AccountButton { text: "Remove account".to_string(), bg: Fill::from(DARK.danger), hover_bg: Fill::from(DARK.close_hover), on_press: EventHandler::new(|_| {}) }),
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
            .background(if hovering() {
                hover_fill
            } else {
                bg_fill
            })
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(label().text(text).color(DARK.text).font_size(13.))
    }
}
