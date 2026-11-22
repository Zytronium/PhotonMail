use freya::prelude::*;
use freya_components::input::InputColorsThemePartial;

use crate::{
    icons::*,
    state::AppState,
    theme::DARK,
    widgets::{avatar, Chip, divider, icon, IconButton},
};

pub struct MessageList {
    pub state: AppState,
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
        let mut s_filter = state;
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
            .padding((6., 10., 6., 10.))
            .corner_radius(8.)
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
        }).into();

        let toolbar = rect()
            .width(Size::fill())
            .vertical()
            .spacing(6.)
            .padding((4., 12., 8., 12.))
            .child(search_bar)
            .child(
                rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(Chip { text: "Unread".to_string(), active: unread_only, on_press: chip_on_press }),
            );

        let body = if visible.is_empty() {
            if query_text.trim().is_empty() {
                rect().padding(24.).child(label().text("No messages").color(DARK.muted))
            } else {
                rect().padding(24.).child(label().text("No matching messages").color(DARK.muted))
            }
        } else {
            rect().child(
                ScrollView::new()
                    .height(Size::fill())
                    .children(visible.iter().map(|&i| rect().key(i).child(MessageRow { state, i }))),
            )
        };

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::MessageList;

        rect()
            .width(Size::px(*state.message_list_width.read()))
            .height(Size::fill())
            .background(crate::grain::surface())
            .padding(2.)
            .border(freya::prelude::Border::new().width(if has_focus { 2. } else { 0. }).fill(DARK.accent))
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
        let selected = *state.selected.read() == i;
        let unread = state.is_unread(i);
        let unified = state.account.read().is_none();
        let weight = if unread { FontWeight::BOLD } else { FontWeight::NORMAL };
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

        // Date, or quick actions while hovering. Same fixed height in both states.
        let trailing = if hovering() {
            rect()
                .horizontal()
                .height(Size::px(28.))
                .child(IconButton { bytes: ARCHIVE, color: DARK.muted, on_press: (move |e: Event<PressEventData>| {
                    e.stop_propagation();
                    s_archive.archive(i);
                }).into() })
                .child(IconButton { bytes: TRASH, color: DARK.muted, on_press: (move |e: Event<PressEventData>| {
                    e.stop_propagation();
                    s_trash.trash(i);
                }).into() })
                .child(IconButton { bytes: MAIL, color: DARK.muted, on_press: (move |e: Event<PressEventData>| {
                    e.stop_propagation();
                    s_read.toggle_read(i);
                }).into() })
        } else {
            rect()
                .height(Size::px(28.))
                .center()
                .child(label().text(m.date.clone()).color(DARK.muted).font_size(12.))
        };

        // For Sent (folder 2) and Drafts (folder 1), show the recipient instead of the sender
        let (display_name, display_email) = if m.folder == 1 || m.folder == 2 {
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
            .child(label().text(display_name).color(DARK.text).font_size(15.).font_weight(weight));
        if unified {
            name_row = name_row.child(label().text(account.name.clone()).color(account.color).font_size(11.));
        }
        let name_row = name_row.child(rect().width(Size::flex(1.))).child(trailing);

        let email_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(label().text(display_email).color(DARK.muted).font_size(12.))
            .child(rect().width(Size::flex(1.)));

        let top = rect()
            .width(Size::fill())
            .spacing(1.)
            .child(name_row)
            .child(email_row);

        let row = rect()
            .width(Size::fill())
            .padding(12.)
            .horizontal()
            .content(Content::Flex)
            .spacing(10.);
        let row = match bg {
            Some(c) => row.background(c),
            None => row,
        };
        row
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(move |_| s_select.select(i))
            .child(avatar(&m.from, account.color, 36.))
            .child(
                rect()
                    .width(Size::flex(1.))
                    .spacing(2.)
                    .child(top)
                    .child(label().text(m.subject.clone()).color(DARK.text).font_weight(weight))
                    .child(label().text(m.preview.clone()).color(DARK.muted).font_size(12.)),
            )
    }
}
