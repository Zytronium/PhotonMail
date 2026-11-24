mod account_view;
mod compose;
mod data;
mod grain;
mod html_viewer;
mod keyboard;
mod link_prefs;
mod mail;
mod message_list;
mod model;
mod pane_divider;
mod reader;
mod sidebar;
mod sounds;
mod state;
mod theme;
mod titlebar;
mod icons;
mod widgets;

use freya::{borderless::BorderlessPlugin, prelude::*};

use crate::{
    html_viewer::LinkConfirmDialog,
    keyboard::handle_key,
    message_list::MessageList, pane_divider::{PaneDivider, DividerTarget},
    reader::{AttachmentPreview, Reader},
    sidebar::Sidebar, state::use_app_state, theme::DARK, titlebar::Titlebar,
};

pub const APP_NAME: &str = "Freagle Mail";
pub const WINDOW_TITLE: &str = "Freagle Mail | Protecting Your Inbox and Your God-Given Right to Email";

/// Global sound effects handle — initialized in main() before launch().
pub static SOUNDS: std::sync::LazyLock<std::sync::Mutex<Option<sounds::SoundEffects>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// `photon-mail --demo` shows the built-in placeholder accounts and emails instead of
/// connecting to real mail servers.
pub fn is_demo() -> bool {
    static DEMO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEMO.get_or_init(|| std::env::args().skip(1).any(|a| a == "--demo"))
}

const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");
const FLAG_JPG: &[u8] = include_bytes!("../assets/flag.jpg");
const AMERICA_JPG: &[u8] = include_bytes!("../assets/america.jpg");

fn main() {
    // Initialize sound effects
    let sounds = sounds::SoundEffects::new();
    *SOUNDS.lock().unwrap() = Some(sounds);

    launch(
        LaunchConfig::new()
            .with_plugin(BorderlessPlugin::new().with_corner_radius(12.))
            .with_window(
                WindowConfig::new(app)
                    .with_title(WINDOW_TITLE)
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

    // Launch splash screen as a separate window on startup
    use_hook(|| {
        freya::prelude::spawn(async move {
            // Small delay to let the splash window render first
            futures_timer::Delay::new(std::time::Duration::from_millis(100)).await;
            let window_id = Platform::get()
                .launch_window(
                    WindowConfig::new(splash_app)
                        .with_title("Freagle Mail")
                        .with_size(640., 430.)
                        .with_decorations(false)
                        .with_transparency(true)
                        .with_background(Color::TRANSPARENT),
                )
                .await;
            // Close splash after 3 seconds
            futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
            Platform::get().close_window(window_id);
        });
    });

    // Play Star-Spangled Banner on startup (once)
    use_hook(|| {
        if let Ok(s) = SOUNDS.lock() {
            if let Some(ref sounds) = *s {
                sounds.play_startup();
            }
        }
    });

    // July fireworks timer — random fireworks during July (or year-round via setting)
    let mut july_fireworks = use_state(|| false);
    use_hook(|| {
        let mut jf = july_fireworks.clone();
        freya::prelude::spawn(async move {
            loop {
                futures_timer::Delay::new(std::time::Duration::from_secs(30)).await;
                let now = chrono::Utc::now();
                if now.format("%m").to_string() == "07" && fastrand::u32(0..100) < 30 {
                    if let Ok(s) = SOUNDS.lock() {
                        if let Some(ref sounds) = *s {
                            sounds.play_firework();
                        }
                    }
                    jf.set(!jf());
                }
            }
        });
    });

    // Fireworks at cursor position on click — Vec of (x, y, unique_id) to support multiple simultaneous fireworks
    let mut fireworks_list = use_state(|| Vec::new());
    let mut fireworks_timer = use_state(|| 0u32);

    // Subscribe to search_query so the app re-renders when it changes, allowing MessageList
    // to re-filter messages. Without this, the parent never re-renders on search query changes
    // because AppState contains State<T> fields which compare equal by Arc pointer identity.
    let _ = state.search_query.read();
    let _ = *july_fireworks.read();
    let fw_list = fireworks_list.read().clone();
    let _ = *fireworks_timer.read();
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
    let dismiss_send_error = {
        let mut store = state.store.clone();
        EventHandler::new(move |_| {
            store.write().send_error = None;
        })
    };

    rect()
        .expanded()
        .background(DARK.bg)
        // Background texture — flag.jpg stretched to fill the window, behind all UI
        .child(
            rect()
                .position(Position::new_global().top(0.).left(0.))
                .width(Size::window_percent(100.))
                .height(Size::window_percent(100.))
                .child(
                    ImageViewer::new(("flag_bg", FLAG_JPG))
                        .width(Size::fill())
                        .height(Size::fill())
                        .opacity(0.5),
                ),
        )
        .child(
            rect()
                .expanded()
                .vertical()
                .on_global_key_down(move |e: Event<KeyboardEventData>| {
                    // Read fresh each time so we never act on a stale copy.
                    let mut fs = *state.focus_state.read();
                    if handle_key(&e, &mut state, &mut fs) {
                        e.stop_propagation();
                        state.focus_state.set(fs);
                    }
                })
                .on_global_pointer_down(move |e: Event<freya::prelude::PointerEventData>| {
                    let mut fs = *state.focus_state.read();
                    fs.keyboard_active = false;
                    state.focus_state.set(fs);

                    // Fireworks at cursor position + play sound
                    if let Ok(s) = SOUNDS.lock() {
                        if let Some(ref sounds) = *s {
                            sounds.play_firework();
                        }
                    }
                    let pos = e.global_location();
                    let pos = (pos.x as f32, pos.y as f32);
                    let id = {
                        let mut ft = fireworks_timer.write();
                        *ft += 1;
                        *ft
                    };
                    {
                        let mut fl = fireworks_list.write();
                        fl.push((pos.0, pos.1, id));
                    }
                    // Remove after animation
                    let mut fl2 = fireworks_list.clone();
                    freya::prelude::spawn(async move {
                        futures_timer::Delay::new(std::time::Duration::from_millis(1500)).await;
                        let mut fl = fl2.write();
                        fl.retain(|(_, _, iid)| *iid != id);
                    });
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
                                .child(label().text("Dismiss").color((255, 255, 255)).font_size(13.)),
                        )
                }))
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
                ),
            )
        // Fireworks particle overlay — supports multiple simultaneous fireworks
        // Each firework is its own overlay rect so the full window is not covered by a transparent rect
        .children(
            fw_list.iter().enumerate().map(|(idx, (x, y, _id))| {
                rect()
                    .key(idx)
                    .layer(Layer::OverlayLevel(3))
                    .interactive(false)
                    .position(Position::new_global().top((y - 150.).max(0.)).left((x - 150.).max(0.)))
                    .width(Size::px(300.))
                    .height(Size::px(300.))
                    .child(FireworksParticles)
            })
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
                                EventHandler::new(move |e: Event<freya::prelude::PressEventData>| {
                                    e.stop_propagation();
                                })
                            })
                            .child(AttachmentPreview { state }),
                    )
            })
        })
}

/// Splash screen component — shows america.jpg fitted to its aspect ratio (2048×1377)
fn splash_app() -> impl IntoElement {
    rect()
        .width(Size::fill())
        .height(Size::fill())
        .background((20, 20, 20))
        .center()
        .child(
            // Window size 640×430 matches america.jpg aspect ratio (2048:1377 ≈ 1.487)
            ImageViewer::new(("splash_americas", AMERICA_JPG))
                .width(Size::fill())
                .height(Size::fill()),
        )
}

/// Realistic fireworks particle animation component with gravity, flashing, and irregular spread.
#[derive(PartialEq)]
struct FireworksParticles;

/// Deterministic "random" seed per particle index — gives each particle unique behavior without true randomness
fn particle_seed(i: usize) -> u32 {
    // Simple hash for deterministic per-particle variation
    let x = i as u32;
    let mut h = x.wrapping_mul(2654435761);
    h ^= h.wrapping_shr(16);
    h.wrapping_mul(2246822519) >> 13
}

impl Component for FireworksParticles {
    fn render(&self) -> impl IntoElement {
        let mut frame = use_state(|| 0u32);
        use_hook(|| {
            let mut f = frame.clone();
            freya::prelude::spawn(async move {
                for i in 0..30u32 {
                    // Vary frame timing slightly for organic feel
                    let delay = if i < 5 { 30 } else { 40 };
                    futures_timer::Delay::new(std::time::Duration::from_millis(delay)).await;
                    f.set(i);
                }
            });
        });

        let f = *frame.read();
        if f >= 30 {
            return rect();
        }

        // Generate particles based on frame
        let colors = [(255, 40, 40), (255, 255, 255), (60, 60, 200), (255, 200, 50), (255, 100, 100)]; // Red, White, Blue, Gold, Light Red
        let total_particles = 40;
        let mut particles = rect()
            .width(Size::fill())
            .height(Size::fill());

        // Burst phase: particles appear over first few frames, then expand and fade
        let burst_frames = 6u32;
        let appeared = ((f * 7).min(total_particles as u32)) as usize;

        for i in 0..appeared {
            let seed = particle_seed(i);

            // Irregular angle: spread in multiple rings, not just one circle
            let ring = ((seed >> 8) % 3) as f32; // 0, 1, or 2 ring
            let base_angle = (i as f32) * std::f32::consts::PI * 2.0 / (total_particles as f32);
            let angle_jitter = (((seed >> 4) % 20) as i32 - 10) as f32 * 0.05; // ±0.5 rad jitter
            let angle = base_angle + angle_jitter + ring * 0.3;

            // Irregular speed: each particle expands at different rate
            let speed_factor = 1.0 + ((seed >> 1) % 15) as f32 * 0.07; // 1.0 to 2.05
            let dist = (f.saturating_sub(ring as u32 * 2)) as f32 * 3.5 * speed_factor;

            // Gravity: particles fall downward over time (quadratic)
            let gravity_pull = (f as f32).powi(2) * 0.15;

            // Center of 300x300 container
            let cx = 150.;
            let cy = 150.;

            let px = angle.cos() * dist + cx;
            let py = angle.sin() * dist + cy + gravity_pull;

            let color = colors[i % colors.len()];

            // Per-particle fade rate: some fade faster, some linger
            let fade_start = 15 + ((seed >> 6) % 10) as u32; // Start fading between frame 15-25
            let fade_duration = 8 + ((seed >> 12) % 8); // Fade over 8-16 frames
            let fade_progress = if f < fade_start {
                0u32
            } else {
                ((f - fade_start).min(fade_duration))
            };
            let base_alpha = (((fade_duration as u32 - fade_progress) * 255 / fade_duration) as u8).max(20);

            // Flashing/twinkling: particles flicker in brightness
            let flash_cycle = (seed >> 3) % 5 + 2; // Flash every 2-7 frames
            let flash_phase = f % flash_cycle;
            let flash_mult = match flash_phase {
                0 => 255, // Full brightness flash
                1 => 180, // Dim
                _ => 220, // Medium
            };
            let alpha = ((base_alpha as u32 * flash_mult) / 255) as u8;

            // Per-particle size variation
            let size_base = 2 + ((seed >> 2) % 4); // 2-6 px
            // Particles shrink slightly as they fade
            let shrink = if f > fade_start { (f - fade_start) as f32 * 0.1 } else { 0.0 };
            let size = (size_base as f32 - shrink).max(1.0);

            particles = particles.child(
                rect()
                    .position(Position::new_absolute().top(py).left(px))
                    .width(Size::px(size))
                    .height(Size::px(size))
                    .corner_radius(size / 2.0)
                    .background(Color::from_argb(alpha, color.0, color.1, color.2)),
            );
        }

        // Central flash: bright flash at the burst point for first few frames
        if f < 4 {
            let flash_alpha = (((4 - f) * 200 / 4) as u8).max(50);
            particles = particles.child(
                rect()
                    .position(Position::new_absolute().top(146.).left(146.))
                    .width(Size::px(8.))
                    .height(Size::px(8.))
                    .corner_radius(4.)
                    .background(Color::from_argb(flash_alpha, 255, 255, 200)),
            );
        }

        particles
    }
}
