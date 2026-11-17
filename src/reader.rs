use freya::prelude::*;

use crate::{
    account_view::{AccountSettings, AddAccountView},
    data::{ACCOUNTS, MAILS},
    icons::*,
    state::{AppState, View},
    theme::DARK,
    widgets::{action, avatar, divider, icon, icon_button},
};

#[derive(PartialEq)]
pub struct Reader {
    pub state: AppState,
}

impl Component for Reader {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let body = match *state.view.read() {
            View::Mail => rect().child(MailView { state }),
            View::AddAccount => rect().child(AddAccountView),
            View::Settings(index) => rect().child(AccountSettings { index }),
        };

        rect()
            .width(Size::flex(1.))
            .height(Size::fill())
            .background(DARK.panel)
            .padding(24.)
            .child(body.width(Size::fill()).height(Size::fill()))
    }
}

#[derive(PartialEq)]
struct MailView {
    state: AppState,
}

impl Component for MailView {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        match state.open() {
            Some(i) => {
                let m = &MAILS[i];
                let account = &ACCOUNTS[m.account];
                let mut s_archive = state;
                let mut s_trash = state;
                let mut s_unread = state;

                let header = rect()
                    .horizontal()
                    .spacing(12.)
                    .child(avatar(m.from, account.color, 44.))
                    .child(
                        rect()
                            .spacing(2.)
                            .child(label().text(m.subject).font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text))
                            .child(label().text(format!("{} → {}", m.from, account.addr)).color(DARK.muted))
                            .child(label().text(m.date).color(DARK.muted).font_size(12.)),
                    );

                let actions = rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(action(REPLY, "Reply", DARK.text).on_press(|_| {}))
                    .child(action(REPLY_ALL, "Reply all", DARK.text).on_press(|_| {}))
                    .child(action(FORWARD, "Forward", DARK.text).on_press(|_| {}))
                    .child(rect().width(Size::flex(1.)))
                    .child(
                        icon_button(ARCHIVE, DARK.muted)
                            .background(DARK.hover)
                            .on_press(move |_| s_archive.archive(i)),
                    )
                    .child(
                        icon_button(TRASH, DARK.muted)
                            .background(DARK.hover)
                            .on_press(move |_| s_trash.trash(i)),
                    )
                    .child(
                        icon_button(MAIL, DARK.muted)
                            .background(DARK.hover)
                            .on_press(move |_| s_unread.toggle_read(i)),
                    );

                rect()
                    .spacing(16.)
                    .child(header)
                    .child(actions)
                    .child(divider())
                    .child(label().text(m.preview).color(DARK.text))
            }
            None => rect()
                .expanded()
                .center()
                .spacing(12.)
                .child(icon(INBOX, 48., DARK.muted))
                .child(label().text("Select a message to read").color(DARK.muted)),
        }
    }
}
