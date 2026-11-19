mod account_view;
mod data;
mod message_list;
mod pane_divider;
mod reader;
mod sidebar;
mod state;
mod theme;
mod titlebar;
mod icons;
mod widgets;
mod keyboard;

use freya::{borderless::BorderlessPlugin, prelude::*};

use crate::{
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
    let mut focus_state = *state.focus_state.read();

    rect()
        .expanded()
        .vertical()
        .background(DARK.bg)
        .on_global_key_down(move |e: Event<KeyboardEventData>| {
            let consumed = handle_key(&e, &mut state, &mut focus_state);
            if consumed {
                e.stop_propagation();
                state.focus_state.set(focus_state);
            }
        })
        .child(Titlebar)
        .child(
            rect()
                .horizontal()
                .content(Content::Flex)
                .width(Size::fill())
                .height(Size::flex(1.))
                .child(Sidebar { state })
                .child(PaneDivider { state, target: DividerTarget::Sidebar })
                .child(MessageList { state })
                .child(PaneDivider { state, target: DividerTarget::MessageList })
                .child(Reader { state }),
        )
}
