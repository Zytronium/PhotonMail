use freya::prelude::*;

use crate::theme::{Rgb, DARK};

pub fn icon(bytes: &'static [u8], size: f32, color: Rgb) -> SvgViewer {
    SvgViewer::new(bytes)
        .width(Size::px(size))
        .height(Size::px(size))
        .stroke(color)
}

pub fn icon_button(bytes: &'static [u8], color: Rgb) -> Rect {
    rect()
        .width(Size::px(28.))
        .height(Size::px(28.))
        .corner_radius(6.)
        .center()
        .child(icon(bytes, 16., color))
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

pub fn chip(text: &str, active: bool) -> Rect {
    rect()
        .padding((4., 10., 4., 10.))
        .corner_radius(12.)
        .background(if active { Fill::from(DARK.accent) } else { crate::grain::hover() })
        .child(label().text(text.to_string()).color(DARK.text).font_size(12.))
}

pub fn divider() -> Rect {
    rect().width(Size::fill()).height(Size::px(1.)).background(DARK.divider)
}

/// Icon + text pill, used for reader actions.
pub fn action(bytes: &'static [u8], text: &str, color: Rgb) -> Rect {
    rect()
        .horizontal()
        .cross_align(Alignment::Center)
        .spacing(6.)
        .padding((6., 10., 6., 10.))
        .corner_radius(6.)
        .background(crate::grain::hover())
        .child(icon(bytes, 14., color))
        .child(label().text(text.to_string()).color(DARK.text).font_size(13.))
}
