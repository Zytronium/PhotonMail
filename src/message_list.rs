use freya::prelude::*;
use freya_components::input::InputColorsThemePartial;

use crate::{
    data::FOLDERS,
    icons::*,
    mail::{DRAFTS, SENT},
    state::AppState,
    theme::DARK,
    widgets::{Chip, ProfileAvatar, divider, dot, icon},
};

pub struct MessageList {
    pub state: AppState,
}

fn first_email_address(email_list: &str) -> &str {
    email_list.split(',').next().unwrap_or_default().trim()
}

impl PartialEq for MessageList {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

impl Component for MessageList {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state;
        let unread_only = *state.unread_only.read();
        let starred_only = *state.starred_only.read();
        let mut s_filter = state;
        let mut s_all = state;
        let search_query = state.search_query.clone();

        // Track whether the search input has focus so the keyboard handler can skip navigation
        let search_a11y_id = use_a11y();
        let search_focus = use_focus(search_a11y_id);
        {
            let mut fs = state.focus_state.write();
            fs.search_input_focused = search_focus.read().is_focused();
        }

        // Filter visible messages by search query
        let query_text = search_query.peek().clone();
        let visible = state.search_visible(&query_text);

        // Search bar
        let search_bar = rect()
            .width(Size::fill())
            .padding((6., 12., 6., 12.))
            .corner_radius(10.)
            .border(freya::prelude::Border::new().width(1.).fill(DARK.divider))
            .background(Color::from_rgb(DARK.bg.0, DARK.bg.1, DARK.bg.2).with_a(128))
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(icon(SEARCH, 14., DARK.muted))
            .child(
                Input::new(search_query.clone())
                    .a11y_id(search_a11y_id)
                    .placeholder("Search messages…")
                    .flat()
                    .compact()
                    .width(Size::fill())
                    .theme_colors(
                        InputColorsThemePartial::new()
                            .focus_background(Color::TRANSPARENT)
                            .focus_border_fill(Color::TRANSPARENT)
                            .border_fill(Color::TRANSPARENT)
                            .color(DARK.text)
                            .placeholder_color(DARK.muted),
                    ),
            );

        let chip_on_press: EventHandler<Event<PressEventData>> = (move |_| {
            let v = !*s_filter.unread_only.read();
            s_filter.unread_only.set(v);
        })
        .into();

        let all_on_press: EventHandler<Event<PressEventData>> = (move |_| {
            s_all.unread_only.set(false);
        })
        .into();

        let acc = *state.account.read();
        let fol = *state.folder.read();
        let title = if starred_only {
            "Starred".to_string()
        } else if acc.is_none() {
            "All inboxes".to_string()
        } else {
            FOLDERS.get(fol).copied().unwrap_or("Inbox").to_string()
        };
        let shown = visible.len();

        let heading = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .padding((6., 4., 2., 4.))
            .child(
                label()
                    .text(title)
                    .color(DARK.text)
                    .font_size(18.)
                    .font_weight(FontWeight::BOLD),
            )
            .child(rect().width(Size::flex(1.)))
            .child(
                label()
                    .text(format!("{} shown", shown))
                    .color(DARK.muted)
                    .font_size(12.),
            );

        let toolbar = rect()
            .width(Size::fill())
            .vertical()
            .spacing(8.)
            .padding((6., 12., 10., 12.))
            .child(heading)
            .child(search_bar)
            .child(
                rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(Chip {
                        text: "All".to_string(),
                        active: !unread_only,
                        on_press: all_on_press,
                    })
                    .child(Chip {
                        text: "Unread".to_string(),
                        active: unread_only,
                        on_press: chip_on_press,
                    }),
            );

        let body = if visible.is_empty() {
            if query_text.trim().is_empty() {
                rect()
                    .padding(24.)
                    .child(label().text("No messages").color(DARK.muted))
            } else {
                rect()
                    .padding(24.)
                    .child(label().text("No matching messages").color(DARK.muted))
            }
        } else {
            rect().child(
                ScrollView::new()
                    .height(Size::fill())
                    .children(visible.iter().map(|&i| {
                        rect()
                            .key(i)
                            .padding((1., 6., 1., 6.))
                            .child(MessageRow { state, i })
                    })),
            )
        };

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::MessageList;

        rect()
            .width(Size::px(*state.message_list_width.read()))
            .height(Size::fill())
            .background(crate::grain::surface())
            .padding(2.)
            .border(
                freya::prelude::Border::new()
                    .width(if has_focus { 2. } else { 0. })
                    .fill(DARK.accent),
            )
            .child(toolbar)
            .child(divider())
            .child(body)
    }
}

struct MessageRow {
    state: AppState,
    i: usize,
}

impl PartialEq for MessageRow {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

impl Component for MessageRow {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let state = self.state;
        let i = self.i;
        let (m, account) = {
            let store = state.store.read();
            let Some(m) = store.mails.get(i).cloned() else {
                return rect();
            };
            let account = store.accounts[m.account].clone();
            (m, account)
        };
        let selected = *state.selected.read() == Some(i);
        let unread = state.is_unread(i);
        let unified = state.account.read().is_none();
        let weight = if unread {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let bg = if selected {
            Some(crate::grain::selected())
        } else if hovering() {
            Some(crate::grain::hover())
        } else {
            None
        };

        let mut s_select = state;
        let mut s_archive = state;
        let mut s_trash = state;
        let mut s_read = state;
        let mut s_star = state;

        // Date, or quick actions while hovering. Same fixed height in both states.
        // Use inline buttons without pointer handlers to avoid flicker — IconButton's
        // on_pointer_enter/leave conflicts with the row's hover detection.
        let date = rect().height(Size::px(28.)).center().child(
            label()
                .text(m.date.clone())
                .color(if unread { DARK.text } else { DARK.muted })
                .font_size(12.),
        );
        let trailing = if hovering() {
            rect()
                .horizontal()
                .content(Content::Flex)
                .spacing(4.)
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_star.toggle_star(i);
                        })
                        .child(icon(
                            STAR,
                            16.,
                            if m.starred { DARK.accent } else { DARK.muted },
                        )),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_archive.archive(i);
                        })
                        .child(icon(ARCHIVE, 16., DARK.muted)),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_trash.trash(i);
                        })
                        .child(icon(TRASH, 16., DARK.muted)),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_read.toggle_read(i);
                        })
                        .child(icon(MAIL, 16., DARK.muted)),
                )
        } else {
            if m.starred {
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(icon(STAR, 14., DARK.accent))
                    .child(date)
            } else {
                date
            }
        };

        // For Sent (folder 2) and Drafts (folder 1), show the recipient instead of the sender
        let (display_name, display_email) = if m.folder == DRAFTS || m.folder == SENT {
            (m.to.clone(), m.to_email.clone())
        } else {
            (m.from.clone(), m.from_email.clone())
        };
        let mut name_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(
                rect().width(Size::flex(1.)).child(
                    label()
                        .text(display_name.clone())
                        .color(DARK.text)
                        .font_size(15.)
                        .font_weight(weight)
                        .max_lines(1)
                        .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                ),
            );
        if unified {
            name_row = name_row.child(
                label()
                    .text(account.name.clone())
                    .color(account.color)
                    .font_size(11.),
            );
        }
        let name_row = name_row.child(trailing);

        let email_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(
                label()
                    .text(display_email.clone())
                    .color(DARK.muted)
                    .font_size(12.)
                    .max_lines(1)
                    .text_overflow(freya::prelude::TextOverflow::Ellipsis),
            )
            .child(rect().width(Size::flex(1.)));

        let top = rect()
            .width(Size::fill())
            .spacing(1.)
            .child(name_row)
            .child(email_row);

        let row = rect()
            .width(Size::fill())
            .padding((10., 10., 10., 8.))
            .corner_radius(10.)
            .horizontal()
            .content(Content::Flex)
            .spacing(10.);
        let row = match bg {
            Some(c) => row.background(c),
            None => row,
        };
        row.on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(move |_e: Event<PressEventData>| {
                if m.folder == DRAFTS {
                    let mut s_compose = state.clone();
                    s_compose.draft_mail.set(Some((m.account, m.folder, m.uid)));
                    s_compose.draft_pin.set(None);
                    s_compose.set_view(crate::state::View::Compose);
                } else {
                    s_select.select(i);
                }
            })
            .child(
                // Fixed-width gutter so rows line up whether read or not.
                rect()
                    .width(Size::px(8.))
                    .margin((14., 0., 0., 0.))
                    .child(if unread {
                        dot(8., DARK.accent)
                    } else {
                        rect().width(Size::px(8.)).height(Size::px(8.))
                    }),
            )
            .child(ProfileAvatar {
                name: display_name,
                email: first_email_address(&display_email).to_string(),
                color: account.color,
                size: 36.,
                state,
            })
            .child(
                rect()
                    .width(Size::flex(1.))
                    .spacing(2.)
                    .child(top)
                    .child(
                        label()
                            .text(m.subject.clone())
                            .color(DARK.text)
                            .font_weight(weight)
                            .max_lines(1)
                            .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                    )
                    .child(
                        label()
                            .text(m.preview.clone())
                            .color(DARK.muted)
                            .font_size(12.)
                            .max_lines(1)
                            .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::first_email_address;

    #[test]
    fn avatar_uses_only_the_first_recipient_email() {
        assert_eq!(
            first_email_address("alice@example.test, bob@example.test"),
            "alice@example.test"
        );
        assert_eq!(
            first_email_address(" alice@example.test "),
            "alice@example.test"
        );
        assert_eq!(first_email_address(""), "");
    }
}
