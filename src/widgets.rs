use freya::prelude::*;
use freya::winit::window::CursorIcon;
use sha2::{Digest, Sha256};

use crate::{
    state::AppState,
    theme::{DARK, Rgb},
};

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
            .corner_radius(8.)
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

#[derive(PartialEq)]
pub struct ProfileAvatar {
    pub name: String,
    pub email: String,
    pub color: Rgb,
    pub size: f32,
    pub state: AppState,
}

impl Component for ProfileAvatar {
    fn render(&self) -> impl IntoElement {
        let name = self.name.clone();
        let email = self.email.clone();
        let color = self.color;
        let size = self.size;
        let show_gravatar = self.state.settings.read().gravatar_enabled();
        let container = rect()
            .width(Size::px(size))
            .height(Size::px(size))
            .corner_radius(size / 2.)
            .background(color);
        let Some(url) = show_gravatar
            .then(|| gravatar_url(&email, size))
            .flatten()
            .and_then(|url| url::Url::parse(&url).ok())
        else {
            return container.center().child(
                label()
                    .text(initials(&name))
                    .color(DARK.text)
                    .font_size(size * 0.42)
                    .font_weight(FontWeight::BOLD),
            );
        };

        container.child(
            ImageViewer::new(url)
                .width(Size::px(size))
                .height(Size::px(size))
                .corner_radius(size / 2.)
                .loading_placeholder(avatar(&name, color, size))
                .error_renderer(move |_| avatar(&name, color, size).into()),
        )
    }
}

fn gravatar_url(email: &str, size: f32) -> Option<String> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') {
        return None;
    }
    let digest = Sha256::digest(email.as_bytes());
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!(
        "https://www.gravatar.com/avatar/{hash}?s={}&d=404",
        size.round() as u32
    ))
}

#[cfg(test)]
mod avatar_tests {
    use super::*;

    #[test]
    fn gravatar_uses_normalized_sha256_email_and_rejects_empty_addresses() {
        let normalized = gravatar_url(" Alice@Example.com ", 40.).unwrap();
        assert_eq!(normalized, gravatar_url("alice@example.com", 40.).unwrap());
        assert!(normalized.contains("/avatar/"));
        assert!(normalized.ends_with("?s=40&d=404"));
        assert!(gravatar_url("not-an-address", 40.).is_none());
    }

    #[test]
    fn gravatar_uses_sha256_digest() {
        let url = gravatar_url("alice@example.com", 36.).unwrap();
        assert!(url.contains("ff8d9819fc0e12bf0d24892e45987e249a28dce836a85cad60e28eaaa8c6d976"));
    }
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
            Fill::from(DARK.hover_strong)
        } else {
            crate::grain::hover()
        };
        rect()
            .padding((4., 12., 4., 12.))
            .corner_radius(14.)
            .border(freya::prelude::Border::new().width(1.).fill(if active {
                DARK.accent
            } else {
                DARK.divider
            }))
            .cursor(CursorIcon::Pointer)
            .background(bg)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(
                label()
                    .text(text)
                    .color(if active { DARK.accent_text } else { DARK.text })
                    .font_size(12.)
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    }),
            )
    }
}

pub fn divider() -> Rect {
    rect()
        .width(Size::fill())
        .height(Size::px(1.))
        .background(DARK.divider)
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
            .padding((6., 12., 6., 12.))
            .corner_radius(8.)
            .border(freya::prelude::Border::new().width(1.).fill(DARK.divider))
            .cursor(CursorIcon::Pointer)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press);
        if hovering() {
            r = r.background(Fill::from(DARK.hover_strong));
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
            Fill::from(DARK.hover_strong)
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

/// Small filled circle (unread marker, account colour, status light).
pub fn dot(size: f32, color: Rgb) -> Rect {
    rect()
        .width(Size::px(size))
        .height(Size::px(size))
        .corner_radius(size / 2.)
        .background(color)
}

/// Rounded count pill used in the sidebar. `on_accent` is for the selected row.
pub fn badge(count: usize, on_accent: bool) -> Rect {
    let fill = if on_accent { DARK.accent } else { DARK.badge };
    let text = if count > 999 {
        "999+".to_string()
    } else {
        count.to_string()
    };
    rect()
        .padding((1., 7., 1., 7.))
        .corner_radius(10.)
        .background(fill)
        .center()
        .child(
            label()
                .text(text)
                .color(DARK.text)
                .font_size(11.)
                .font_weight(FontWeight::BOLD),
        )
}
