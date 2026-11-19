use freya::prelude::*;

use crate::{
    data::{ACCOUNTS, FOLDERS, LABELS},
    icons::*,
    state::{AppState, View},
    theme::{Rgb, DARK},
    widgets::{divider, icon, IconButton},
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
        rows.push(rect().child(Item {
            lead: Lead::Icon(INBOX),
            text: "All inboxes".to_string(),
            active: in_mail && acc.is_none() && fol == 0,
            count: state.inbox_unread(None),
            indent: 0.,
            on_press: (move |_| s.go(None, 0)).into(),
        }));

        // Accounts section header
        rows.push(rect().child(SectionHeader { title: "ACCOUNTS", show_add: true, state: state.clone() }));

        for (i, a) in ACCOUNTS.iter().enumerate() {
            let open = expanded[i];
            let mut s_toggle = state;
            let mut s_gear = state;
            let gear_on_press: EventHandler<Event<PressEventData>> = (move |e: Event<PressEventData>| {
                e.stop_propagation();
                s_gear.view.set(View::Settings(i));
            }).into();
            rows.push(rect().child(GroupHeader {
                name: a.name.to_string(),
                color: a.color,
                open,
                active: view == View::Settings(i),
                count: state.inbox_unread(Some(i)),
                show_gear: true,
                gear_on_press: gear_on_press.clone(),
                on_press: (move |_| s_toggle.toggle_expanded(i)).into(),
            }));
            if open {
                for (f, name) in FOLDERS.iter().enumerate() {
                    let mut s_go = state;
                    rows.push(rect().child(Item {
                        lead: Lead::Icon(FOLDER_ICONS[f]),
                        text: name.to_string(),
                        active: in_mail && acc == Some(i) && fol == f,
                        count: state.account_folder_unread(i, f),
                        indent: 20.,
                        on_press: (move |_| s_go.go(Some(i), f)).into(),
                    }));
                }
            }
        }

        // Labels (static for now)
        rows.push(rect().child(SectionHeader { title: "LABELS", show_add: false, state: state.clone() }));
        for (name, color) in LABELS.iter() {
            rows.push(rect().child(Item {
                lead: Lead::Tinted(TAG, *color),
                text: name.to_string(),
                active: false,
                count: 0,
                indent: 0.,
                on_press: EventHandler::new(|_| {}),
            }));
        }

        let mut compose_hover = use_state(|| false);
        let mut s_compose = state;
        let compose = rect()
            .width(Size::fill())
            .padding(10.)
            .corner_radius(8.)
            .horizontal()
            .center()
            .spacing(8.)
            .background(if compose_hover() {
                Fill::from((120, 90, 255))
            } else {
                Fill::from(DARK.accent)
            })
            .on_pointer_enter(move |_| compose_hover.set(true))
            .on_pointer_leave(move |_| compose_hover.set(false))
            .on_press(move |_| s_compose.view.set(View::Compose))
            .child(icon(PENCIL, 16., DARK.text))
            .child(label().text("New message").color(DARK.text).font_weight(FontWeight::BOLD));

        let footer = rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(8.)
            .padding((8., 4., 0., 4.))
            .child(label().text("●").color(DARK.success).font_size(10.))
            .child(label().text("All accounts synced").color(DARK.muted).font_size(12.));

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::Sidebar;

        rect()
            .width(Size::px(*state.sidebar_width.read()))
            .height(Size::fill())
            .background(crate::grain::panel())
            .padding(14.)
            .spacing(8.)
            .content(Content::Flex)
            .border(freya::prelude::Border::new().width(if has_focus { 2. } else { 0. }).fill(DARK.accent))
            .child(compose)
            .child(
                // Negative right margin pulls the ScrollView (and its scrollbar) past the outer padding to the edge
                rect()
                    .margin((0., -14., 0., 0.))
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .child(
                        ScrollView::new()
                            .height(Size::fill())
                            .children(rows.into_iter().enumerate().map(|(n, r)| r.key(n))),
                    ),
            )
            .child(divider())
            .child(footer)
    }
}

#[derive(PartialEq, Clone)]
enum Lead {
    Dot(Rgb),
    Icon(&'static [u8]),
    Tinted(&'static [u8], Rgb),
}

#[derive(PartialEq, Clone)]
struct SectionHeader {
    title: &'static str,
    show_add: bool,
    state: AppState,
}

impl Component for SectionHeader {
    fn render(&self) -> impl IntoElement {
        let show_add = self.show_add;
        let title = self.title;
        let state = self.state.clone();
        let mut r = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .padding((12., 14., 4., 8.))
            .child(label().text(title.to_string()).color(DARK.muted).font_size(11.).font_weight(FontWeight::BOLD))
            .child(rect().width(Size::flex(1.)));
        if show_add {
            let mut s_add = state;
            r = r.child(IconButton {
                bytes: PLUS,
                color: DARK.muted,
                on_press: (move |_| s_add.view.set(View::AddAccount)).into(),
            });
        }
        r
    }
}

#[derive(PartialEq, Clone)]
struct GroupHeader {
    name: String,
    color: Rgb,
    open: bool,
    active: bool,
    count: usize,
    show_gear: bool,
    gear_on_press: EventHandler<Event<PressEventData>>,
    on_press: EventHandler<Event<PressEventData>>,
}

impl Component for GroupHeader {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let name = self.name.clone();
        let color = self.color;
        let open = self.open;
        let active = self.active;
        let count = self.count;
        let show_gear = self.show_gear;
        let gear_on_press = self.gear_on_press.clone();
        let on_press = self.on_press.clone();
        let mut r = rect()
            .width(Size::fill())
            .padding((4., 14., 4., 8.))
            .corner_radius(8.)
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(icon(if open { CHEVRON_DOWN } else { CHEVRON_RIGHT }, 14., DARK.muted))
            .child(label().text("●").color(color))
            .child(label().text(name).color(DARK.text).font_weight(FontWeight::BOLD))
            .child(rect().width(Size::flex(1.)));
        if count > 0 {
            r = r.child(label().text(count.to_string()).color(DARK.text).font_size(12.).font_weight(FontWeight::BOLD));
        }
        if active {
            r = r.background(crate::grain::selected());
        } else if hovering() {
            r = r.background(crate::grain::hover());
        }
        if show_gear {
            r = r.child(IconButton {
                bytes: SETTINGS,
                color: DARK.muted,
                on_press: gear_on_press,
            });
        }
        r
    }
}

#[derive(PartialEq, Clone)]
struct Item {
    lead: Lead,
    text: String,
    active: bool,
    count: usize,
    indent: f32,
    on_press: EventHandler<Event<PressEventData>>,
}

impl Component for Item {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let lead = self.lead.clone();
        let text = self.text.clone();
        let active = self.active;
        let count = self.count;
        let indent = self.indent;
        let on_press = self.on_press.clone();
        let bg = if active {
            Some(crate::grain::selected())
        } else if hovering() {
            Some(crate::grain::hover())
        } else {
            None
        };

        let mut row = rect()
            .width(Size::fill())
            .padding((8., 14., 8., 8. + indent))
            .corner_radius(8.)
            .horizontal()
            .cross_align(Alignment::Center)
            .content(Content::Flex)
            .spacing(8.)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press);
        if let Some(c) = bg {
            row = row.background(c);
        }

        let mut row = match lead {
            Lead::Dot(color) => row.child(label().text("●").color(color)),
            Lead::Icon(bytes) => row.child(icon(bytes, 16., if active { DARK.text } else { DARK.muted })),
            Lead::Tinted(bytes, color) => row.child(icon(bytes, 16., color)),
        }
            .child(label().text(text).color(DARK.text))
            .child(rect().width(Size::flex(1.)));

        if count > 0 {
            row = row.child(label().text(count.to_string()).color(DARK.text).font_size(12.).font_weight(FontWeight::BOLD));
        }
        row
    }
}
