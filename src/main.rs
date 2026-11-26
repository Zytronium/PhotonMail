mod account_view;
mod compose;
mod data;
mod grain;
mod html_viewer;
mod icons;
mod keyboard;
mod link_prefs;
mod mail;
mod message_list;
mod model;
mod pane_divider;
mod reader;
mod settings_view;
mod sidebar;
mod state;
mod theme;
mod titlebar;
mod widgets;

use freya::tray::{
    TrayEvent, TrayIconBuilder,
    menu::{Menu, MenuItem, Submenu},
};
use freya::{borderless::BorderlessPlugin, prelude::*};

use crate::{
    html_viewer::LinkConfirmDialog,
    keyboard::handle_key,
    message_list::MessageList,
    pane_divider::{DividerTarget, PaneDivider},
    reader::{AttachmentPreview, Reader},
    sidebar::Sidebar,
    state::{NotificationChannels, use_app_state},
    theme::DARK,
    titlebar::Titlebar,
};

pub const APP_NAME: &str = "Photon Mail";

/// `photon-mail --demo` shows the built-in placeholder accounts and emails instead of
/// connecting to real mail servers.
pub fn is_demo() -> bool {
    static DEMO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEMO.get_or_init(|| std::env::args().skip(1).any(|a| a == "--demo"))
}

const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

fn main() {
    let (notification_open_tx, notification_open_rx) = async_channel::unbounded::<Option<String>>();
    let (notification_target_tx, notification_target_rx) = async_channel::unbounded();
    let notification_channels = NotificationChannels {
        open_rx: notification_open_rx,
        open_tx: notification_open_tx,
        target_rx: notification_target_rx,
        target_tx: notification_target_tx,
    };
    launch(
        LaunchConfig::new()
            .with_plugin(BorderlessPlugin::new().with_corner_radius(12.))
            .with_global_context(notification_channels.clone())
            .with_task({
                let notification_open_rx = notification_channels.open_rx.clone();
                move |proxy| async move {
                    while let Ok(activation_token) = notification_open_rx.recv().await {
                        let _ = proxy
                            .post_callback(move |renderer| {
                                activate_app_windows_with_token(renderer, activation_token)
                            })
                            .await;
                    }
                }
            })
            .with_window(
                WindowConfig::new(app)
                    .with_title(APP_NAME)
                    .with_icon(LaunchConfig::window_icon(LOGO_PNG))
                    .with_size(1200., 750.)
                    .with_decorations(false)
                    .with_transparency(true)
                    .with_background(Color::TRANSPARENT)
                    .with_on_close(|renderer, window_id| {
                        if let Some(window) = renderer.windows.get_mut(&window_id) {
                            window.window_mut().set_minimized(true);
                            window.window_mut().set_visible(false);
                        }
                        CloseDecision::KeepOpen
                    }),
            )
            .with_exit_on_close(false)
            .with_tray(
                || {
                    let open = MenuItem::with_id("open", "Open Photon Mail", true, None);
                    let quit = MenuItem::with_id("quit", "Quit Photon Mail", true, None);
                    let submenu = Submenu::with_items("Photon Mail", true, &[&open, &quit])
                        .expect("Failed to create tray menu items");
                    let menu = Menu::with_items(&[&submenu]).expect("Failed to create tray menu");

                    TrayIconBuilder::new()
                        .with_icon(LaunchConfig::tray_icon(LOGO_PNG))
                        .with_tooltip(APP_NAME)
                        .with_menu(Box::new(menu))
                        .build()
                        .expect("Failed to create system tray icon")
                },
                |event, mut renderer| {
                    if let TrayEvent::Menu(event) = event {
                        match event.id.as_ref() {
                            "open" => {
                                activate_app_windows(&mut renderer);
                            }
                            "quit" => renderer.exit(),
                            _ => {}
                        }
                    }
                },
            ),
    )
}

fn activate_app_windows(renderer: &mut RendererContext<'_>) {
    activate_app_windows_with_token(renderer, None);
}

fn activate_app_windows_with_token(
    renderer: &mut RendererContext<'_>,
    activation_token: Option<String>,
) {
    for window in renderer.windows.values_mut() {
        let window = window.window_mut();
        window.set_minimized(false);
        window.set_visible(true);
        window.request_redraw();
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(token) = activation_token.as_ref() {
            use freya::winit::platform::startup_notify::WindowExtStartupNotify;
            let token = freya::winit::window::ActivationToken::from_raw(token.clone());
            let _ = window.activate_with_token(token);
        } else {
            window.focus_window();
        }

        #[cfg(not(all(unix, not(target_os = "macos"))))]
        window.focus_window();
    }
}

fn app() -> impl IntoElement {
    let mut state = use_app_state();

    // Subscribe to search_query so the app re-renders when it changes, allowing MessageList
    // to re-filter messages. Without this, the parent never re-renders on search query changes
    // because AppState contains State<T> fields which compare equal by Arc pointer identity.
    let _ = state.search_query.read();

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

    let send_error = state.store.read().send_error.clone();
    let pending_sends = state.pending_sends.read().clone();
    let dismiss_send_error = {
        let mut store = state.store.clone();
        EventHandler::new(move |_| {
            store.write().send_error = None;
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
        .maybe_child(send_error.map(|msg| {
            rect()
                .horizontal()
                .width(Size::fill())
                .padding(8.)
                .spacing(12.)
                .background(DARK.danger)
                .child(label().text(msg).color((255, 255, 255)).font_size(13.))
                .child(
                    rect()
                        .padding((2., 10., 2., 10.))
                        .corner_radius(4.)
                        .on_press(dismiss_send_error)
                        .child(
                            label()
                                .text("Dismiss")
                                .color((255, 255, 255))
                                .font_size(13.),
                        ),
                )
        }))
        .maybe_child((!pending_sends.is_empty()).then(|| {
            rect()
                .vertical()
                .spacing(4.)
                .children(pending_sends.iter().map(|pending| {
                    let id = pending.id;
                    let remaining_seconds = pending.remaining_seconds;
                    let mut state = state;
                    rect()
                        .horizontal()
                        .width(Size::fill())
                        .padding(8.)
                        .spacing(12.)
                        .background(DARK.accent)
                        .child(
                            label()
                                .text(format!("Message queued — {}s to undo", remaining_seconds))
                                .color((255, 255, 255))
                                .font_size(13.),
                        )
                        .child(
                            rect()
                                .padding((2., 10., 2., 10.))
                                .corner_radius(4.)
                                .cursor(freya::winit::window::CursorIcon::Pointer)
                                .on_press(move |_| state.undo_pending_send(id))
                                .child(
                                    label()
                                        .text("Undo send")
                                        .color((255, 255, 255))
                                        .font_size(13.),
                                ),
                        )
                }))
        }))
        .child(
            rect()
                .horizontal()
                .content(Content::Flex)
                .width(Size::fill())
                .height(Size::fill())
                .child(Sidebar { state })
                .child(PaneDivider {
                    state,
                    target: DividerTarget::Sidebar,
                })
                .child(MessageList { state })
                .child(PaneDivider {
                    state,
                    target: DividerTarget::MessageList,
                })
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
        .maybe_child({
            // Attachment preview overlay
            let show_preview = *state.attachment_preview_open.read();
            show_preview.then(|| {
                let close_preview = {
                    let mut preview_open = state.attachment_preview_open.clone();
                    let mut preview_fs = state.attachment_preview_fullscreen.clone();
                    EventHandler::new(move |_| {
                        preview_open.set(false);
                        preview_fs.set(false);
                    })
                };
                rect()
                    .layer(Layer::Overlay)
                    .position(Position::new_global().top(0.).left(0.))
                    .width(Size::window_percent(100.))
                    .height(Size::window_percent(100.))
                    .background(Color::from_argb(180, 0, 0, 0))
                    .on_press(close_preview.clone())
                    .child(
                        rect()
                            .position(Position::new_global().top(0.).left(0.))
                            .width(Size::window_percent(100.))
                            .height(Size::window_percent(100.))
                            .center()
                            .on_press({
                                EventHandler::new(
                                    move |e: Event<freya::prelude::PressEventData>| {
                                        e.stop_propagation();
                                    },
                                )
                            })
                            .child(AttachmentPreview { state }),
                    )
            })
        })
}
