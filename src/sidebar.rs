use freya::prelude::*;

use crate::{
    data::{ACCOUNTS, FOLDERS, LABELS},
    icons::*,
    state::{AppState, View},
    theme::{Rgb, DARK},
    widgets::{divider, icon, icon_button},
};

#[derive(PartialEq)]
pub struct Sidebar {
    pub state: AppState,
}

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let acc = *state.account.read();
        let fol = *state.folder.read();
        let view = *state.view.read();
        let in_mail = view == View::Mail;
        let expanded = state.expanded.read().clone();

        let mut rows: Vec<Rect> = Vec::new();

        // Unified inbox
        let mut s = state;
        rows.push(
            item(Lead::Icon(INBOX), "All inboxes", in_mail && acc.is_none() && fol == 0, state.inbox_unread(None), 0.)
                .on_press(move |_| s.go(None, 0)),
        );

        // Accounts
        let mut s = state;
        let add = icon_button(PLUS, DARK.muted).on_press(move |_| s.view.set(View::AddAccount));
        rows.push(section("ACCOUNTS", Some(add)));
        for (i, a) in ACCOUNTS.iter().enumerate() {
            let open = expanded[i];
            let mut s_toggle = state;
            let mut s_gear = state;
            let gear = icon_button(SETTINGS, DARK.muted).on_press(move |e: Event<PressEventData>| {
                e.stop_propagation();
                s_gear.view.set(View::Settings(i));
            });
            rows.push(
                group_header(a.name, a.color, open, view == View::Settings(i), state.inbox_unread(Some(i)), gear)
                    .on_press(move |_| s_toggle.toggle_expanded(i)),
            );
            if open {
                for (f, name) in FOLDERS.iter().enumerate() {
                    let mut s_go = state;
                    rows.push(
                        item(
                            Lead::Icon(FOLDER_ICONS[f]),
                            name,
                            in_mail && acc == Some(i) && fol == f,
                            state.account_folder_unread(i, f),
                            20.,
                        )
                            .on_press(move |_| s_go.go(Some(i), f)),
                    );
                }
            }
        }

        // Labels (static for now)
        rows.push(section("LABELS", None));
        for (name, color) in LABELS.iter() {
            rows.push(item(Lead::Tinted(TAG, *color), name, false, 0, 0.));
        }

        let compose = rect()
            .width(Size::fill())
            .padding(10.)
            .corner_radius(8.)
            .horizontal()
            .center()
            .spacing(8.)
            .background(DARK.accent)
            .on_press(|_| {}) // TODO: compose
            .child(icon(PENCIL, 16., DARK.text))
            .child(label().text("New message").color(DARK.text).font_weight(FontWeight::BOLD));

        let footer = rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(8.)
            .padding((8., 4., 0., 4.))
            .child(label().text("●").color(DARK.success).font_size(10.))
            .child(label().text("All accounts synced").color(DARK.muted).font_size(12.));

        rect()
            .width(Size::px(*state.sidebar_width.read()))
            .height(Size::fill())
            .background(DARK.panel)
            .padding(12.)
            .spacing(8.)
            .content(Content::Flex)
            .child(compose)
            .child(
                ScrollView::new()
                    .height(Size::flex(1.))
                    .children(rows.into_iter().enumerate().map(|(n, r)| r.key(n))),
            )
            .child(divider())
            .child(footer)
    }
}

enum Lead {
    Dot(Rgb),
    Icon(&'static [u8]),
    Tinted(&'static [u8], Rgb),
}

fn section(title: &str, trailing: Option<Rect>) -> Rect {
    let mut r = rect()
        .width(Size::fill())
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .padding((12., 0., 4., 8.))
        .child(label().text(title.to_string()).color(DARK.muted).font_size(11.).font_weight(FontWeight::BOLD))
        .child(rect().width(Size::flex(1.)));
    if let Some(t) = trailing {
        r = r.child(t);
    }
    r
}

fn group_header(name: &str, color: Rgb, open: bool, active: bool, count: usize, gear: Rect) -> Rect {
    let mut r = rect()
        .width(Size::fill())
        .padding((4., 4., 4., 8.))
        .corner_radius(8.)
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .spacing(6.)
        .background(if active { DARK.selected } else { DARK.panel })
        .child(icon(if open { CHEVRON_DOWN } else { CHEVRON_RIGHT }, 14., DARK.muted))
        .child(label().text("●").color(color))
        .child(label().text(name.to_string()).color(DARK.text).font_weight(FontWeight::BOLD))
        .child(rect().width(Size::flex(1.)));
    if count > 0 {
        r = r.child(label().text(count.to_string()).color(DARK.text).font_size(12.).font_weight(FontWeight::BOLD));
    }
    r.child(gear)
}

fn item(lead: Lead, text: &str, active: bool, count: usize, indent: f32) -> Rect {
    let row = rect()
        .width(Size::fill())
        .padding((8., 8., 8., 8. + indent))
        .corner_radius(8.)
        .horizontal()
        .cross_align(Alignment::Center)
        .content(Content::Flex)
        .spacing(8.)
        .background(if active { DARK.selected } else { DARK.panel });

    let mut row = match lead {
        Lead::Dot(color) => row.child(label().text("●").color(color)),
        Lead::Icon(bytes) => row.child(icon(bytes, 16., if active { DARK.text } else { DARK.muted })),
        Lead::Tinted(bytes, color) => row.child(icon(bytes, 16., color)),
    }
        .child(label().text(text.to_string()).color(DARK.text))
        .child(rect().width(Size::flex(1.)));

    if count > 0 {
        row = row.child(label().text(count.to_string()).color(DARK.text).font_size(12.).font_weight(FontWeight::BOLD));
    }
    row
}
