mod account_view;
mod data;
mod message_list;
mod reader;
mod sidebar;
mod state;
mod theme;
mod titlebar;
mod icons;
mod widgets;

use freya::{borderless::BorderlessPlugin, prelude::*};

use crate::{
    message_list::MessageList, reader::Reader, sidebar::Sidebar, state::use_app_state,
    theme::DARK, titlebar::Titlebar,
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
    let state = use_app_state();

    rect()
        .expanded()
        .vertical()
        .background(DARK.bg)
        .child(Titlebar)
        .child(
            rect()
                .horizontal()
                .content(Content::Flex)
                .width(Size::fill())
                .height(Size::flex(1.))
                .child(Sidebar { state })
                .child(MessageList { state })
                .child(Reader { state }),
        )
}
