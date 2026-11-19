use freya::prelude::*;

use crate::{
    data::{ACCOUNTS, FOLDERS},
    theme::DARK,
    widgets::{avatar, divider},
};

const PROVIDERS: [&str; 5] = ["Proton Mail", "Gmail", "Outlook", "iCloud", "Other (IMAP)"];

#[derive(PartialEq)]
pub struct AddAccountView;

impl Component for AddAccountView {
    fn render(&self) -> impl IntoElement {
        let mut col = rect()
            .spacing(10.)
            .child(label().text("Add account").font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text))
            .child(label().text("Choose a provider").color(DARK.muted))
            .child(rect().height(Size::px(8.)));

        for (n, p) in PROVIDERS.iter().enumerate() {
            col = col.child(ProviderCard { index: n, name: *p });
        }
        col.child(label().text("Sign-in forms come next.").color(DARK.muted).font_size(12.))
    }
}

#[derive(PartialEq)]
struct ProviderCard {
    index: usize,
    name: &'static str,
}

impl Component for ProviderCard {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let name = self.name;
        rect()
            .key(self.index)
            .width(Size::px(360.))
            .padding(12.)
            .corner_radius(8.)
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(12.)
            .background(if hovering() {
                Fill::from((45, 45, 62))
            } else {
                crate::grain::hover()
            })
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(|_| {}) // TODO: sign-in form
            .child(avatar(name, DARK.accent, 32.))
            .child(label().text(name).color(DARK.text))
    }
}

#[derive(PartialEq)]
pub struct AccountSettings {
    pub index: usize,
}

impl Component for AccountSettings {
    fn render(&self) -> impl IntoElement {
        let a = &ACCOUNTS[self.index];

        rect()
            .spacing(16.)
            .child(
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(14.)
                    .child(avatar(a.name, a.color, 56.))
                    .child(
                        rect()
                            .spacing(2.)
                            .child(label().text(a.name).font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text))
                            .child(label().text(a.addr).color(DARK.muted)),
                    ),
            )
            .child(divider())
            .child(field("Sync status", &format!("● Synced {}", a.sync), DARK.success))
            .child(field("Folders", &FOLDERS.len().to_string(), DARK.text))
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(AccountButton { text: "Sync now".to_string(), bg: crate::grain::hover(), hover_bg: Fill::from((45, 45, 62)), on_press: EventHandler::new(|_| {}) })
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
