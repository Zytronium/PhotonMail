use freya::prelude::*;
use freya::winit::window::CursorIcon;

use crate::{
    state::AppState,
    theme::{Rgb, DARK, HOVER_STRONG},
};

/// Patriotic tooltips for Freagle Mail
pub fn patriotic_tooltip(text: &str) -> String {
    match text {
        "Reply" => "Send a message back to your fellow patriot".to_string(),
        "Reply all" => "Broadcast your reply to all your fellow patriots".to_string(),
        "Forward" => "Pass this along to another freedom lover".to_string(),
        "Archive" => "Store this in the archives of liberty".to_string(),
        "Trash" => "Banish this to the dustbin of tyranny".to_string(),
        "Mark as read" | "Mark as unread" => "Toggle the read status of this missive".to_string(),
        "New message" => "Compose a new message to your fellow citizens".to_string(),
        "Send" => "Launch this message into the great American sky".to_string(),
        "Save" => "Preserve this draft for posterity".to_string(),
        "Back" => "Return to the inbox of freedom".to_string(),
        "Search messages…" => "Search the archives of liberty".to_string(),
        "Unread" => "Show only unread missives".to_string(),
        _ => text.to_string(),
    }
}

pub fn icon(bytes: &'static [u8], size: f32, color: Rgb) -> SvgViewer {
    SvgViewer::new(bytes)
        .width(Size::px(size))
        .height(Size::px(size))
        .stroke(color)
}

#[derive(PartialEq, Clone)]
pub struct IconButton {
    pub bytes: &'static [u8],
    pub color: Rgb,
    pub on_press: EventHandler<Event<PressEventData>>,
}

impl Component for IconButton {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let bytes = self.bytes;
        let color = self.color;
        let on_press = self.on_press.clone();
        let mut r = rect()
            .width(Size::px(28.))
            .height(Size::px(28.))
            .corner_radius(6.)
            .center()
            .cursor(CursorIcon::Pointer)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press);
        if hovering() {
            r = r.background(crate::grain::hover());
        }
        r.child(icon(bytes, 16., color))
    }
}

pub fn initials(name: &str) -> String {
    name.trim_start_matches("To: ")
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

pub fn avatar(name: &str, color: Rgb, size: f32) -> Rect {
    rect()
        .width(Size::px(size))
        .height(Size::px(size))
        .corner_radius(size / 2.)
        .background(color)
        .center()
        .child(
            label()
                .text(initials(name))
                .color(DARK.text)
                .font_size(size * 0.42)
                .font_weight(FontWeight::BOLD),
        )
}

#[derive(PartialEq, Clone)]
pub struct Chip {
    pub text: String,
    pub active: bool,
    pub on_press: EventHandler<Event<PressEventData>>,
}

impl Component for Chip {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let text = self.text.clone();
        let active = self.active;
        let on_press = self.on_press.clone();
        let bg = if active {
            Fill::from(DARK.accent)
        } else if hovering() {
            Fill::from((40, 40, 85))
        } else {
            crate::grain::hover()
        };
        rect()
            .padding((4., 10., 4., 10.))
            .corner_radius(12.)
            .cursor(CursorIcon::Pointer)
            .background(bg)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(label().text(text).color(DARK.text).font_size(12.))
    }
}

pub fn divider() -> Rect {
    rect().width(Size::fill()).height(Size::px(1.)).background(DARK.divider)
}

/// Icon + text pill, used for reader actions. Supports hover state with FREEDOM GLOW.
#[derive(PartialEq, Clone)]
pub struct Action {
    pub bytes: &'static [u8],
    pub text: String,
    pub color: Rgb,
    pub on_press: EventHandler<Event<PressEventData>>,
}

impl Component for Action {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let mut glow_frame = use_state(|| 0u32);
        let bytes = self.bytes;
        let text = self.text.clone();
        let color = self.color;
        let on_press = self.on_press.clone();

        // Animate the FREEDOM GLOW
        use_hook(|| {
            let mut gf = glow_frame.clone();
            freya::prelude::spawn(async move {
                loop {
                    futures_timer::Delay::new(std::time::Duration::from_millis(100)).await;
                    let f = *gf.read();
                    gf.set(f.wrapping_add(1));
                }
            });
        });

        let frame = *glow_frame.read();
        let glow_color = if hovering() {
            // Animated red-white-blue glow
            let phase = (frame % 30) as f32 / 30.0;
            let colors = [(200, 40, 60), (240, 240, 255), (60, 60, 200)];
            let idx = (phase * 3.0) as usize % 3;
            colors[idx]
        } else {
            (0, 0, 0)
        };
        let has_glow = hovering() && glow_color != (0, 0, 0);

        let mut r = rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding((6., 10., 6., 10.))
            .corner_radius(6.)
            .cursor(CursorIcon::Pointer)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press);
        if hovering() {
            r = r.background(Fill::from((45, 45, 95)));
            if has_glow {
                r = r.border(freya::prelude::Border::new()
                    .width(2.)
                    .fill(Color::from_rgb(glow_color.0, glow_color.1, glow_color.2)));
            }
        } else {
            r = r.background(crate::grain::hover());
        }
        r.child(icon(bytes, 14., color))
            .child(label().text(text).color(DARK.text).font_size(13.))
    }
}

/// The "..." button that shows/hides the quoted part of a reply. Styled like the other
/// secondary buttons (`Action`): grain `hover` surface, stronger colour on hover.
#[derive(PartialEq)]
pub struct QuoteToggle {
    pub state: AppState,
    pub mail_idx: usize,
}

impl Component for QuoteToggle {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let mut state = self.state;
        let mail_idx = self.mail_idx;
        let bg = if hovering() {
            Fill::from(HOVER_STRONG)
        } else {
            crate::grain::hover()
        };

        rect()
            .padding((4., 14., 4., 14.))
            .corner_radius(6.)
            .background(bg)
            .center()
            .cursor(CursorIcon::Pointer)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(EventHandler::new(move |_| {
                let mut expanded = state.quoted_expanded.read().clone();
                if !expanded.remove(&mail_idx) {
                    expanded.insert(mail_idx);
                }
                state.quoted_expanded.set(expanded);
            }))
            .child(icon(crate::icons::ELLIPSIS, 16., DARK.text))
    }
}
