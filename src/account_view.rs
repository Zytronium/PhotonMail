use freya::prelude::*;

use freya_components::input::InputColorsThemePartial;

use crate::{
    data::FOLDERS,
    state::AppState,
    theme::DARK,
    widgets::{avatar, divider},
};

/// (label, IMAP host). Empty host = user types it. Implicit-TLS port 993 for all.
const PROVIDERS: [(&str, &str); 4] = [
    ("Gmail", "imap.gmail.com"),
    ("Outlook", "outlook.office365.com"),
    ("iCloud", "imap.mail.me.com"),
    ("Other (IMAP)", ""),
];

#[derive(PartialEq)]
pub struct AddAccountView {
    pub state: AppState,
}

impl Component for AddAccountView {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state;
        let demo = state.store.read().demo;
        let error = state.store.read().add_error.clone();

        let email = use_state(String::new);
        let password = use_state(String::new);
        let host = use_state(String::new);
        let port = use_state(|| "993".to_string());
        let mut working = use_state(|| false);

        // A failure from the worker re-enables the button.
        if error.is_some() && *working.peek() {
            working.set(false);
        }

        let mut col = rect()
            .spacing(10.)
            .child(label().text("Add account").font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text));

        if demo {
            return col.child(label().text("Demo mode: accounts are disabled. Run without --demo to add one.").color(DARK.muted));
        }

        col = col.child(label().text("Select a provider to prefill the server, then sign in.").color(DARK.muted));
        let mut row = rect().horizontal().spacing(8.);
        for (n, (name, h)) in PROVIDERS.iter().enumerate() {
            // Only "Other (IMAP)" (last entry) is enabled; the rest are disabled.
            let enabled = n == PROVIDERS.len() - 1;
            row = row.child(ProviderCard { index: n, name: *name, host: *h, host_state: host, enabled });
        }
        col = col.child(row).child(rect().height(Size::px(6.)));

        col = col
            .child(form_field("Email", email, false))
            .child(form_field("Password", password, true))
            .child(form_field("IMAP server", host, false))
            .child(form_field("Port (993 = TLS)", port, false));

        if let Some(e) = error {
            col = col.child(label().text(e).color(DARK.danger).font_size(13.));
        }
        if *working.read() {
            col = col.child(label().text("Connecting and checking your login…").color(DARK.muted).font_size(13.));
        }

        let submit = {
            let (email, password, host, port) = (email, password, host, port);
            EventHandler::new(move |_| {
                if *working.peek() {
                    return;
                }
                let addr = email.read().trim().to_string();
                let server = host.read().trim().to_string();
                let port: u16 = port.read().trim().parse().unwrap_or(993);
                let pw = password.read().clone();
                if !addr.contains('@') || server.is_empty() || pw.is_empty() {
                    state.store.write().add_error =
                        Some("Enter your email, password and IMAP server.".to_string());
                    return;
                }
                state.store.write().add_error = None;
                working.set(true);
                let cfg = crate::mail::AccountConfig {
                    id: crate::mail::config::new_id(),
                    name: addr.split('@').next().unwrap_or(&addr).to_string(),
                    email: addr.clone(),
                    username: addr,
                    host: server,
                    port,
                };
                state
                    .engine
                    .peek()
                    .send(crate::mail::Command::AddAccount { cfg, password: pw });
            })
        };
        col.child(AccountButton {
            text: (if *working.read() { "Connecting…" } else { "Connect" }).to_string(),
            bg: Fill::from(DARK.accent),
            hover_bg: Fill::from((120, 90, 255)),
            on_press: submit,
        })
    }
}

fn form_field(title: &str, value: State<String>, secret: bool) -> Rect {
    let mut input = Input::new(value)
        .placeholder(title.to_string())
        .compact()
        .width(Size::px(360.))
        .theme_colors(
            InputColorsThemePartial::new()
                .color(DARK.text)
                .placeholder_color(DARK.muted)
                .background(Color::from((30, 30, 46)))
                .focus_background(Color::from((30, 30, 46)))
                .border_fill(Color::from((50, 50, 70)))
                .focus_border_fill(Color::from(DARK.accent)),
        );
    if secret {
        // If this doesn't compile on your Freya version, delete this line (password shows).
        input = input.mode(InputMode::new_password());
    }
    rect()
        .spacing(3.)
        .child(label().text(title.to_string()).color(DARK.muted).font_size(12.))
        .child(input)
}

#[derive(PartialEq)]
struct ProviderCard {
    index: usize,
    name: &'static str,
    host: &'static str,
    host_state: State<String>,
    enabled: bool,
}

impl Component for ProviderCard {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let name = self.name;
        let host = self.host;
        let mut host_state = self.host_state;
        let enabled = self.enabled;
        let mut r = rect()
            .key(self.index)
            .padding(10.)
            .corner_radius(8.)
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(8.)
            .background(Fill::from((20, 20, 28)))
            .child(avatar(name, if enabled { DARK.accent } else { (80, 80, 100) }, 24.))
            .child(label().text(name).color(if enabled { DARK.text } else { DARK.muted }).font_size(13.).opacity(if enabled { 1. } else { 0.4 }));
        if enabled {
            r = r
                .background(if hovering() {
                    Fill::from((45, 45, 62))
                } else {
                    crate::grain::hover()
                })
                .on_pointer_enter(move |_| hovering.set(true))
                .on_pointer_leave(move |_| hovering.set(false))
                .on_press(move |_| host_state.set(host.to_string()));
        }
        r
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
                    .child(AccountButton { text: "Edit".to_string(), bg: crate::grain::hover(), hover_bg: Fill::from((45, 45, 62)), on_press: EventHandler::new(|_| {}) })
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
