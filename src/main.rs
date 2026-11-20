mod account_view;
mod compose;
mod data;
mod grain;
mod html_viewer;
mod keyboard;
mod link_prefs;
mod message_list;
mod pane_divider;
mod reader;
mod sidebar;
mod state;
mod theme;
mod titlebar;
mod icons;
mod widgets;

use freya::{borderless::BorderlessPlugin, prelude::*};

use crate::{
    html_viewer::LinkConfirmDialog,
    keyboard::handle_key,
    message_list::MessageList, pane_divider::{PaneDivider, DividerTarget}, reader::Reader,
    sidebar::Sidebar, state::use_app_state, theme::DARK, titlebar::Titlebar,
};

pub const APP_NAME: &str = "Photon Mail";
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

fn main() {
    launch(
        LaunchConfig::new()
            .with_plugin(BorderlessPlugin::new().with_corner_radius(12.))
            .with_window(
                WindowConfig::new(app)
                    .with_title(APP_NAME)
                    .with_icon(LaunchConfig::window_icon(LOGO_PNG))
                    .with_size(1200., 750.)
                    .with_decorations(false)
                    .with_transparency(true)
                    .with_background(Color::TRANSPARENT),
            ),
    )
}

fn app() -> impl IntoElement {
    let mut state = use_app_state();

    // Link confirmation dialog — uses Freya's Popup component for proper z-order and centering.
    let show_link_dialog = *state.link_dialog_open.read();
    let close_link_dialog = {
        let mut dialog_open = state.link_dialog_open.clone();
        let mut dialog_url = state.link_dialog_url.clone();
        let mut dialog_sender = state.link_dialog_sender.clone();
        EventHandler::new(move |_| {
            dialog_open.set(false);
            dialog_url.set(String::new());
            dialog_sender.set(String::new());
        })
    };

    rect()
        .expanded()
        .background(DARK.bg)
        .vertical()
        .on_global_key_down(move |e: Event<KeyboardEventData>| {
            // Read fresh each time so we never act on a stale copy.
            let mut fs = *state.focus_state.read();
            if handle_key(&e, &mut state, &mut fs) {
                e.stop_propagation();
                state.focus_state.set(fs);
            }
        })
        .on_global_pointer_down(move |_| {
            let mut fs = *state.focus_state.read();
            fs.keyboard_active = false;
            state.focus_state.set(fs);
        })
        .child(Titlebar)
        .child(
            rect()
                .horizontal()
                .content(Content::Flex)
                .width(Size::fill())
                .height(Size::fill())
                .child(Sidebar { state })
                .child(PaneDivider { state, target: DividerTarget::Sidebar })
                .child(MessageList { state })
                .child(PaneDivider { state, target: DividerTarget::MessageList })
                .child(Reader { state }),
        )
        .maybe_child(show_link_dialog.then(|| {
            // Manual overlay using the same approach as freya-components' PopupBackground
            // but with our own dark theme styling.
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_global().top(0.).left(0.))
                .width(Size::window_percent(100.))
                .height(Size::window_percent(100.))
                .background(Color::from_argb(180, 0, 0, 0))
                .on_press(close_link_dialog)
                .child(
                    rect()
                        .position(Position::new_global().top(0.).left(0.))
                        .width(Size::window_percent(100.))
                        .height(Size::window_percent(100.))
                        .center()
                        .on_press({
                            EventHandler::new(move |e: Event<freya::prelude::PressEventData>| {
                                e.stop_propagation();
                            })
                        })
                        .child(LinkConfirmDialog { state }),
                )
        }))
}
