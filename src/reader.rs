use freya::prelude::*;

use crate::{
    account_view::{AccountSettings, AddAccountView},
    compose::ComposePane,
    data::{MailContentType, ACCOUNTS, MAILS},
    html_viewer::SecureHtmlViewer,
    icons::{CHEVRON_RIGHT, *},
    state::{AppState, View},
    theme::DARK,
    widgets::{Action, avatar, divider, icon, IconButton},
};

#[derive(PartialEq)]
pub struct Reader {
    pub state: AppState,
}

impl Component for Reader {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let view = *state.view.read();
        let show_back = matches!(view, View::AddAccount | View::Settings(_));

        let mut col = rect().spacing(16.).padding(10.);

        // Back button when in a sub-view (not for Compose, which has its own close)
        if show_back {
            let mut s_back = state;
            col = col.child(
                rect()
                    .width(Size::px(120.))
                    .padding((6., 12., 6., 12.))
                    .corner_radius(6.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .background(crate::grain::hover())
                    .on_press(move |_| s_back.view.set(View::Mail))
                    .child(icon(CHEVRON_RIGHT, 14., DARK.text))
                    .child(label().text("Back").color(DARK.text).font_size(13.)),
            );
        }

        let body = match view {
            View::Mail => rect().child(MailView { state }),
            View::Compose => rect().child(ComposePane { state }),
            View::AddAccount => rect().child(AddAccountView),
            View::Settings(index) => rect().child(AccountSettings { index }),
        };

        col = col.child(body.width(Size::fill()).height(Size::fill()));

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::Reader;

        rect()
            .width(Size::flex(1.))
            .height(Size::fill())
            .background(crate::grain::panel())
            .padding(26.)
            .border(freya::prelude::Border::new().width(if has_focus { 2. } else { 0. }).fill(DARK.accent))
            .child(col.width(Size::fill()).height(Size::fill()))
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
                    .child(Action { bytes: REPLY, text: "Reply".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) })
                    .child(Action { bytes: REPLY_ALL, text: "Reply all".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) })
                    .child(Action { bytes: FORWARD, text: "Forward".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) })
                    .child(rect().width(Size::flex(1.)))
                    .child(
                        IconButton { bytes: ARCHIVE, color: DARK.muted, on_press: (move |_| s_archive.archive(i)).into() }
                    )
                    .child(
                        IconButton { bytes: TRASH, color: DARK.muted, on_press: (move |_| s_trash.trash(i)).into() }
                    )
                    .child(
                        IconButton { bytes: MAIL, color: DARK.muted, on_press: (move |_| s_unread.toggle_read(i)).into() }
                    );

                let body_content = if m.content_type == MailContentType::Html {
                    rect()
                        .height(Size::fill())
                        .width(Size::fill())
                        .key(i)
                        .child(SecureHtmlViewer {
                            html: m.body.to_string(),
                            sender_email: m.from_email.to_string(),
                            state,
                        })
                } else {
                    rect()
                        .height(Size::fill())
                        .width(Size::fill())
                        .child(
                            ScrollView::new()
                                .height(Size::fill())
                                .child(
                                    paragraph()
                                        .span(m.body)
                                        .color(DARK.text),
                                ),
                        )
                };

                rect()
                    .height(Size::fill())
                    .spacing(16.)
                    .child(header)
                    .child(actions)
                    .child(divider())
                    .child(body_content)
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
