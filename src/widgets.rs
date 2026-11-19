use freya::prelude::*;
use freya::winit::window::CursorIcon;

use crate::theme::{Rgb, DARK};

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
            Fill::from((45, 45, 62))
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

/// Icon + text pill, used for reader actions. Supports hover state.
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
        let bytes = self.bytes;
        let text = self.text.clone();
        let color = self.color;
        let on_press = self.on_press.clone();
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
            r = r.background(Fill::from((50, 50, 68)));
        } else {
            r = r.background(crate::grain::hover());
        }
        r.child(icon(bytes, 14., color))
         .child(label().text(text).color(DARK.text).font_size(13.))
    }
}
