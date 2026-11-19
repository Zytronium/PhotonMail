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
            col = col.child(
                rect()
                    .key(n)
                    .width(Size::px(360.))
                    .padding(12.)
                    .corner_radius(8.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(12.)
                    .background(crate::grain::hover())
                    .on_press(|_| {}) // TODO: sign-in form
                    .child(avatar(p, DARK.accent, 32.))
                    .child(label().text(*p).color(DARK.text)),
            );
        }
        col.child(label().text("Sign-in forms come next.").color(DARK.muted).font_size(12.))
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
                    .child(button("Sync now", crate::grain::hover()))
                    .child(button("Edit", crate::grain::hover()))
                    .child(button("Remove account", DARK.danger)),
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

fn button(text: &str, bg: impl Into<Fill>) -> Rect {
    rect()
        .padding((8., 14., 8., 14.))
        .corner_radius(6.)
        .background(bg)
        .on_press(|_| {}) // TODO
        .child(label().text(text.to_string()).color(DARK.text).font_size(13.))
}
