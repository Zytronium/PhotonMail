use freya::prelude::*;

use crate::{APP_NAME, theme::DARK};

const LOGO: &[u8] = include_bytes!("../assets/logo.svg");
const ICON_MIN: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M1 5h8" stroke="#ebebf5" stroke-width="1" fill="none"/></svg>"##;
const ICON_MAX: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="1.5" y="1.5" width="7" height="7" stroke="#ebebf5" stroke-width="1" fill="none"/></svg>"##;
const ICON_RESTORE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M3 3V1.5h5.5V7H7" stroke="#ebebf5" stroke-width="1" fill="none"/><rect x="1.5" y="3" width="5.5" height="5.5" stroke="#ebebf5" stroke-width="1" fill="none"/></svg>"##;
const ICON_CLOSE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="#ebebf5" stroke-width="1" fill="none"/></svg>"##;

#[derive(PartialEq)]
pub struct Titlebar;

impl Component for Titlebar {
    fn render(&self) -> impl IntoElement {
        rect()
            .horizontal()
            .content(Content::Flex)
            .width(Size::fill())
            .height(Size::px(36.))
            .background(crate::grain::titlebar())
            .cross_align(Alignment::Center)
            .padding((0., 0., 0., 12.))
            .spacing(8.)
            .child(
                SvgViewer::new(LOGO)
                    .width(Size::px(18.))
                    .height(Size::px(18.)),
            )
            .child(label().text(APP_NAME).color(DARK.text).font_size(13.))
            .child(
                rect()
                    .window_drag()
                    .width(Size::flex(1.))
                    .height(Size::fill()),
            )
            .child(WinButton(WinAction::Minimize))
            .child(WinButton(WinAction::Maximize))
            .child(WinButton(WinAction::Close))
    }
}

#[derive(PartialEq, Clone, Copy)]
enum WinAction {
    Minimize,
    Maximize,
    Close,
}

#[derive(PartialEq)]
struct WinButton(WinAction);

impl Component for WinButton {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let mut maximized = use_state(|| false);
        let action = self.0;

        let bg = match (hovering(), action == WinAction::Close) {
            (true, true) => Some(crate::grain::close_hover()),
            (true, false) => Some(crate::grain::hover()),
            _ => None,
        };
        let icon: &'static [u8] = match action {
            WinAction::Minimize => ICON_MIN,
            WinAction::Maximize if maximized() => ICON_RESTORE,
            WinAction::Maximize => ICON_MAX,
            WinAction::Close => ICON_CLOSE,
        };

        let button = rect().width(Size::px(46.)).height(Size::px(36.));
        let button = match bg {
            Some(c) => button.background(c),
            None => button,
        };
        button
            .center()
            .on_pointer_enter(move |_| {
                hovering.set(true);
                Platform::get().with_window(Platform::window_id(), move |window| {
                    let now = window.is_maximized() || window.fullscreen().is_some();
                    if maximized() != now {
                        maximized.set(now);
                    }
                });
            })
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(move |_| match action {
                WinAction::Minimize => {
                    Platform::get().with_window(Platform::window_id(), |w| w.set_minimized(true));
                }
                WinAction::Maximize => {
                    Platform::get().with_window(Platform::window_id(), |w| {
                        let is_max = w.is_maximized();
                        w.set_maximized(!is_max);
                    });
                    maximized.set(!maximized());
                }
                WinAction::Close => {
                    Platform::get().with_window(Platform::window_id(), |window| {
                        window.set_visible(false);
                    });
                }
            })
            .child(
                SvgViewer::new(icon)
                    .width(Size::px(12.))
                    .height(Size::px(12.)),
            )
    }
}
