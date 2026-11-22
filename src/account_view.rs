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
        let display_name = use_state(String::new);
        let password = use_state(String::new);
        let host = use_state(String::new);
        let port = use_state(|| "993".to_string());
        let security = use_state(|| "SSL/TLS".to_string());
        let mut working = use_state(|| false);
        let current_step = use_state(|| SetupStep::Account);

        // A failure from the worker re-enables the button.
        if error.is_some() && *working.peek() {
            working.set(false);
        }

        // Auto-discover IMAP host from email domain when advancing from Account step
        let discover_host = {
            let email = email.clone();
            let mut host = host.clone();
            move || {
                if host.read().is_empty() {
                    let addr = email.read().trim().to_string();
                    if let Some(domain) = addr.split('@').nth(1) {
                        let discovered = match domain {
                            d if d.ends_with("gmail.com") || d.ends_with("googlemail.com") => "imap.gmail.com",
                            d if d.ends_with("outlook.com") || d.ends_with("hotmail.com") || d.ends_with("live.com") => "outlook.office365.com",
                            d if d.ends_with("icloud.com") => "imap.mail.me.com",
                            d if d.ends_with("yahoo.com") => "imap.mail.yahoo.com",
                            _ => "",
                        };
                        if !discovered.is_empty() {
                            host.set(discovered.to_string());
                        }
                    }
                }
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
                        label: "Display name",
                        placeholder: "Your Name (optional)",
                        value: display_name.clone(),
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
                    .child(
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
                                    .child(label().text("Common providers are auto-detected from your email address.").color(DARK.muted).font_size(12.))
                            )
                    )
            }
            SetupStep::Verify => {
                let addr = email.read().clone();
                let name = display_name.read().clone();
                let srv = host.read().clone();
                let p = port.read().clone();
                let name_display = if name.is_empty() { addr.split('@').next().unwrap_or(&addr).to_string() } else { name };

                rect().spacing(12.)
                    .child(label().text("Review your settings before connecting.").color(DARK.muted).font_size(13.))
                    .child(
                        rect()
                            .padding(14.)
                            .corner_radius(8.)
                            .background(Fill::from((30, 30, 46)))
                            .spacing(10.)
                            .child(VerifyRow { label: "Account", value: addr.clone() })
                            .child(VerifyRow { label: "Name", value: name_display })
                            .child(VerifyRow { label: "IMAP server", value: format!("{}:{}", srv, p) })
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
            SetupStep::Server => host.read().trim().len() > 0,
            SetupStep::Verify => true,
        };

        // Action button (Next or Connect)
        let action_button = if *current_step.read() == SetupStep::Verify {
            let (email, password, host, port) = (email.clone(), password.clone(), host.clone(), port.clone());
            let display_name = display_name.clone();
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
                    let name = display_name.read().trim().to_string();
                    if !addr.contains('@') || server.is_empty() || pw.is_empty() {
                        state.store.write().add_error =
                            Some("Enter your email, password and IMAP server.".to_string());
                        return;
                    }
                    state.store.write().add_error = None;
                    working.set(true);
                    let cfg = crate::mail::AccountConfig {
                        id: crate::mail::config::new_id(),
                        name: if name.is_empty() { addr.split('@').next().unwrap_or(&addr).to_string() } else { name },
                        email: addr.clone(),
                        username: addr,
                        host: server,
                        port: port_num,
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

/// Edit an existing account's name, email, and IMAP server settings.
#[derive(PartialEq)]
pub struct EditAccountView {
    pub state: AppState,
    pub index: usize,
}

impl Component for EditAccountView {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state;
        let index = self.index;

        // Pre-fill from the existing account
        let existing = {
            let store = state.store.read();
            store.accounts.get(index).cloned()
        };
        let Some(account) = existing else {
            return rect().child(label().text("Account not found").color(DARK.danger));
        };

        let email = use_state(|| account.addr.clone());
        let display_name = use_state(|| account.name.clone());
        let host = use_state(|| account.host.clone());

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

        // Form fields
        col = col.child(
            rect()
                .width(Size::fill())
                .spacing(12.)
                .child(AddAccountFormField {
                    label: "Email address",
                    placeholder: "you@example.com",
                    value: email.clone(),
                    secret: false,
                })
                .child(AddAccountFormField {
                    label: "Display name",
                    placeholder: "Your name",
                    value: display_name.clone(),
                    secret: false,
                })
                .child(AddAccountFormField {
                    label: "IMAP server",
                    placeholder: "imap.example.com",
                    value: host.clone(),
                    secret: false,
                })
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
                        let name = display_name.read().trim().to_string();
                        let server = host.read().trim().to_string();

                        if !addr.contains('@') || server.is_empty() {
                            return;
                        }

                        let mut store = state.store.write();
                        if let Some(acc) = store.accounts.get_mut(index) {
                            acc.name = if name.is_empty() {
                                addr.split('@').next().unwrap_or(&addr).to_string()
                            } else {
                                name
                            };
                            acc.addr = addr;
                            acc.host = server;
                        }
                        drop(store);
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
