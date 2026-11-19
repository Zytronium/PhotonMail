use freya::prelude::*;
use freya_components::input::InputColorsThemePartial;

use crate::{
    data::{ACCOUNTS, MAILS},
    icons::*,
    state::AppState,
    theme::DARK,
    widgets::{avatar, Chip, divider, icon, IconButton},
};

#[derive(PartialEq)]
pub struct MessageList {
    pub state: AppState,
}

impl Component for MessageList {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let visible = state.visible();
        let unread_only = *state.unread_only.read();
        let mut s_filter = state;
        let search_query = use_state(|| String::new());

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
            rect().padding(24.).child(label().text("No messages").color(DARK.muted))
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

#[derive(PartialEq)]
struct MessageRow {
    state: AppState,
    i: usize,
}

impl Component for MessageRow {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let state = self.state;
        let i = self.i;
        let m = &MAILS[i];
        let account = &ACCOUNTS[m.account];
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
                .child(label().text(m.date).color(DARK.muted).font_size(12.))
        };

        let mut name_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(label().text(m.from).color(DARK.text).font_size(15.).font_weight(weight));
        if unified {
            name_row = name_row.child(label().text(account.name).color(account.color).font_size(11.));
        }
        let name_row = name_row.child(rect().width(Size::flex(1.))).child(trailing);

        let email_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(label().text(m.from_email).color(DARK.muted).font_size(12.))
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
            .child(avatar(m.from, account.color, 36.))
            .child(
                rect()
                    .width(Size::flex(1.))
                    .spacing(2.)
                    .child(top)
                    .child(label().text(m.subject).color(DARK.text).font_weight(weight))
                    .child(label().text(m.preview).color(DARK.muted).font_size(12.)),
            )
    }
}
