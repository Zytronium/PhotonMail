//! HTML email rendering with security sandboxing and external link handling.
//!
//! Wraps Freya's `HtmlViewer` (Blitz-powered) to render email HTML safely:
//! - Strips `<script>` tags and `on*` event attributes
//! - Detects and blocks tracking pixels (1x1 images, invisible iframes)
//! - Sanitizes `javascript:` URLs
//! - Intercepts link clicks and shows a confirmation dialog before opening externally
//! - Remembers "do not show again" preferences per sender and globally

use std::sync::LazyLock;

use freya::prelude::*;
use freya::html::*;
use freya::winit::window::CursorIcon;
use regex::Regex;

use crate::grain;
use crate::state::AppState;
use crate::theme::DARK;
use crate::widgets::QuoteToggle;

/// Precompiled regex patterns for HTML sanitization.
static RE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
static RE_EVENT_DQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\s+on\w+\s*=\s*"[^"]*""#).unwrap());
static RE_EVENT_SQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+on\w+\s*=\s*'[^']*'").unwrap());
static RE_JS_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)javascript:").unwrap());
static RE_TRACKING_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<img[^>]*(?:width=["']?1["']?\s+height=["']?1["']?|height=["']?1["']?\s+width=["']?1["']?|src=["'][^"']*(?:track|pixel|beacon|analytics|collect|__utm)[^"']*["][^>]*|class=["'][^"']*(?:track|pixel|beacon|analytics)[^"']*["][^>]*|id=["'][^"']*(?:track|pixel|beacon|analytics)[^"']*["][^>]*).*?>\s*"#
    ).unwrap()
});
static RE_TRACKING_FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<iframe[^>]*(?:style=["'][^"']*(?:display:\s*none|visibility:\s*hidden|width:\s*0|height:\s*0|opacity:\s*0)[^"']*["'][^>]*|width=["']?0["']?\s+height=["']?0["']?|src=["'][^"']*(?:track|pixel|beacon|analytics|collect|__utm)[^"']*["'][^>]*).*?</iframe>"#
    ).unwrap()
});


static RE_BG_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:background(?:-color)?\s*:\s*|bgcolor\s*=\s*["']?)(#[0-9a-f]{3,8}\b|rgba?\([^)]*\)|[a-z]+)"#).unwrap()
});
static RE_COLOR_SCHEME_DARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:color-scheme\s*:\s*[^;"}]*\bdark\b|<meta[^>]*name=["']color-scheme["'][^>]*dark|supported-color-schemes[^>]*dark)"#).unwrap()
});

/// Parse a CSS/HTML color into (r, g, b). Returns None for unknown / transparent values.
fn parse_color(v: &str) -> Option<(u8, u8, u8)> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        let h: Vec<u8> = hex.chars().filter_map(|c| c.to_digit(16).map(|d| d as u8)).collect();
        return match h.len() {
            3 | 4 => Some((h[0] * 17, h[1] * 17, h[2] * 17)),
            6 | 8 => Some((h[0] * 16 + h[1], h[2] * 16 + h[3], h[4] * 16 + h[5])),
            _ => None,
        };
    }
    if v.starts_with("rgb") {
        let inner = v.split('(').nth(1)?.split(')').next()?;
        let nums: Vec<f32> = inner
            .split(|c: char| c == ',' || c == ' ' || c == '/')
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.trim_end_matches('%').parse::<f32>().ok())
            .collect();
        if nums.len() >= 3 {
            if nums.len() >= 4 && nums[3] == 0.0 {
                return None;
            }
            return Some((nums[0] as u8, nums[1] as u8, nums[2] as u8));
        }
        return None;
    }
    match v.as_str() {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        "navy" => Some((0, 0, 128)),
        "darkblue" => Some((0, 0, 139)),
        "midnightblue" => Some((25, 25, 112)),
        "darkslategray" | "darkslategrey" => Some((47, 79, 79)),
        "gray" | "grey" => Some((128, 128, 128)),
        _ => None,
    }
}

fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
}

/// Heuristic: does this email already ship with a dark design?
/// Looks for a declared dark color scheme, or a dark page background on
/// html/body/the outermost table (the first background declarations in the document).
fn is_already_dark(html: &str) -> bool {
    if RE_COLOR_SCHEME_DARK.is_match(html) {
        return true;
    }
    // Only consider the first few background declarations: those belong to the
    // outermost wrappers (html/body/main table), not to small inner accents.
    let mut seen = 0;
    let mut dark_votes = 0;
    let mut light_votes = 0;
    for cap in RE_BG_DECL.captures_iter(html) {
        if let Some(c) = parse_color(&cap[1]) {
            if luminance(c) < 0.35 {
                dark_votes += 1;
            } else {
                light_votes += 1;
            }
            seen += 1;
            if seen >= 4 {
                break;
            }
        }
    }
    dark_votes > 0 && dark_votes >= light_votes
}

/// Analyze an HTML body and return a sanitized version plus tracking-pixel warnings.
#[derive(Clone, Default, PartialEq)]
pub struct HtmlAnalysis {
    /// The sanitized HTML ready for rendering.
    pub sanitized: String,
    /// Number of suspected tracking pixels that were blocked.
    pub tracking_pixels_blocked: usize,
    /// Number of script/event handlers that were stripped.
    pub scripts_stripped: usize,
}

/// Sanitize HTML content for safe rendering in an email client.
///
/// This is a conservative sanitizer that removes known attack vectors
/// while preserving legitimate email formatting. It's designed to be
/// extended with more sophisticated sanitization later.
pub fn sanitize_html(html: &str) -> HtmlAnalysis {
    let mut analysis = HtmlAnalysis::default();
    let mut result = html.to_string();

    // 1. Strip <script>...</script> blocks
    let count = RE_SCRIPT.find_iter(&result).count();
    analysis.scripts_stripped += count;
    result = RE_SCRIPT.replace_all(&result, "").to_string();

    // 2. Strip on* event handlers (onclick, onload, onerror, etc.)
    for re in &[&*RE_EVENT_DQ, &*RE_EVENT_SQ] {
        let count = re.find_iter(&result).count();
        analysis.scripts_stripped += count;
        result = re.replace_all(&result, "").to_string();
    }

    // 3. Block javascript: URLs
    let count = RE_JS_URL.find_iter(&result).count();
    analysis.scripts_stripped += count;
    result = RE_JS_URL.replace_all(&result, "blocked:").to_string();

    // 4. Detect and block tracking pixels (1x1 images, invisible images, tracking URLs)
    let count = RE_TRACKING_IMG.find_iter(&result).count();
    analysis.tracking_pixels_blocked += count;
    result = RE_TRACKING_IMG.replace_all(&result, "").to_string();

    // 5. Block invisible iframes (another common tracking method)
    let count = RE_TRACKING_FRAME.find_iter(&result).count();
    analysis.tracking_pixels_blocked += count;
    result = RE_TRACKING_FRAME.replace_all(&result, "").to_string();

    // 6. Inject CSS for dark mode rendering and pointer cursor on links.
    //    Uses the standard email-client approach: invert all colors via CSS filter
    //    so dark text becomes readable on our dark background, then re-invert images
    //    so photographs don't look like negatives. This is how Gmail iOS and many
    //    other clients handle dark mode for emails without explicit dark-mode styles.
    //    Emails that are already dark (detected heuristically) are left untouched,
    //    otherwise the inversion would turn them light.
    let style = if is_already_dark(&result) {
        "<style>a{cursor:pointer!important}</style>".to_string()
    } else {
        r#"<style>
html{filter:invert(1) hue-rotate(180deg)!important}
img,video,iframe{filter:invert(1) hue-rotate(180deg)!important}
a{cursor:pointer!important}
</style>"#.to_string()
    };
    if let Some(body_start) = result.find("<body") {
        let insert_pos = if let Some(gt) = result[body_start..].find('>') {
            body_start + gt + 1
        } else {
            body_start + 6
        };
        result.insert_str(insert_pos, &style);
    } else if let Some(html_start) = result.find("<html") {
        let insert_pos = if let Some(gt) = result[html_start..].find('>') {
            html_start + gt + 1
        } else {
            html_start + 6
        };
        result.insert_str(insert_pos, &style);
    } else {
        // No <body> or <html> tag — prepend the style
        result.insert_str(0, &style);
    }

    analysis.sanitized = result;
    analysis
}

/// A secure HTML viewer component for rendering email bodies.
/// Wraps `HtmlViewer` with pre-sanitization, security warnings, link click interception,
/// and pointer cursor on hover over links.
#[derive(PartialEq)]
pub struct SecureHtmlViewer {
    pub html: String,
    /// Sender email address, used for "do not show again" preferences.
    pub sender_email: String,
    /// Shared app state for link dialog and preferences.
    pub state: AppState,
    /// Index of the message being shown when its HTML contains the quote-button spacer
    /// (see `mail::quoting::prepare_html_quotes`). The native "..." button is drawn over the
    /// spacer and flips this message in `AppState::quoted_expanded`.
    pub quote_toggle_idx: Option<usize>,
}

impl Component for SecureHtmlViewer {
    fn render(&self) -> impl IntoElement {
        let html = self.html.clone();
        let sender_email = self.sender_email.clone();
        let state = self.state;
        let quote_toggle_idx = self.quote_toggle_idx;

        // Sanitize the HTML content
        let analysis = use_memo(move || sanitize_html(&html));
        let sanitized_ref = analysis.read();
        let sanitized = sanitized_ref.sanitized.clone();
        let tracking_blocked = sanitized_ref.tracking_pixels_blocked;
        let scripts_stripped = sanitized_ref.scripts_stripped;

        let has_warnings = tracking_blocked > 0 || scripts_stripped > 0;

        // Initialize the HtmlHandle with the sanitized HTML.
        // When the email changes, the parent keys this component by the mail index,
        // forcing a full remount so use_hook runs again with the new HTML.
        let handle = use_html(|| HtmlSource::html(sanitized.clone()));

        // Watch for link clicks: when current_url() becomes Some, a link was clicked.
        // We immediately navigate back to the original HTML and show the dialog via AppState.
        // Use create_sync so the first run is immediate and establishes reactive deps right away.
        use_hook(|| {
            let mut handle = handle;
            let sanitized = sanitized.clone();
            let sender_email = sender_email.clone();
            let mut dialog_url = state.link_dialog_url.clone();
            let mut dialog_sender = state.link_dialog_sender.clone();
            let mut dialog_open = state.link_dialog_open.clone();
            let link_prefs = state.link_prefs.clone();

            Effect::create_sync(move || {
                let url = match handle.current_url() {
                    Some(u) => u,
                    None => return,
                };

                // Navigate back to the original HTML immediately
                handle.replace_html(sanitized.clone());

                // Check if we should skip the dialog
                let prefs = link_prefs.read();
                if prefs.should_skip(&sender_email) {
                    let _ = open::that(&url);
                    return;
                }

                // Show the dialog via app state
                dialog_url.set(url.clone());
                dialog_sender.set(sender_email.clone());
                dialog_open.set(true);
            });
        });

        // Forward mouse moves to Blitz so it updates hover state, then read
        // the cursor from Blitz's document. Blitz correctly detects links via
        // CSS cursor rules and is_link() checks, even for table-based layouts
        // where elements_at() hit testing fails.
        let cursor_icon = use_state(|| CursorIcon::Default);

        let mut viewer = HtmlViewer::new(handle).expanded();
        if quote_toggle_idx.is_some() {
            // Report where the spacer is so the native button can be drawn over it
            viewer = viewer.track(crate::mail::quoting::ANCHOR_ID);
        }

        let mut col = rect()
            .height(Size::fill())
            .width(Size::fill())
            .vertical()
            .cursor(*cursor_icon.read())
            .on_mouse_move({
                let h = handle;
                let mut ci = cursor_icon.clone();
                move |e: Event<MouseEventData>| {
                    let x = e.element_location.x as f32;
                    let y = e.element_location.y as f32;
                    // Forward to Blitz so it updates hover state
                    h.mouse_move(x, y);
                    // Read the cursor from Blitz's document
                    let cursor = h.get_cursor().unwrap_or(CursorIcon::Default);
                    if *ci.peek() != cursor {
                        ci.set(cursor);
                    }
                }
            })
            .child(viewer);

        // Native "..." button, drawn over the spacer at the end of the new content.
        if let (Some(idx), Some(anchor)) = (quote_toggle_idx, handle.anchor_rect()) {
            col = col.child(
                rect()
                    .position(Position::new_absolute().top(anchor.y).left(anchor.x))
                    .width(Size::px(anchor.width))
                    .height(Size::px(anchor.height))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .child(QuoteToggle { state, mail_idx: idx }),
            );
        }

        if has_warnings {
            col = col.child(
                rect()
                    .width(Size::fill())
                    .padding((4., 8., 4., 8.))
                    .horizontal()
                    .spacing(6.)
                    .cross_align(Alignment::Center)
                    .child(
                        label()
                            .text(format!(
                                "Security: {} tracking pixel(s) blocked, {} script(s) stripped",
                                tracking_blocked, scripts_stripped
                            ))
                            .color((240, 140, 40))
                            .font_size(11.),
                    ),
            );
        }

        col
    }
}

/// Confirmation dialog shown when a link in an HTML email is clicked.
/// Renders inside Freya's Popup component, which handles overlay, centering, and z-order.
#[derive(PartialEq)]
pub struct LinkConfirmDialog {
    pub state: AppState,
}

impl Component for LinkConfirmDialog {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let url = state.link_dialog_url.read().clone();
        let sender = state.link_dialog_sender.read().clone();

        let trust_sender = use_state(|| false);
        let always_open = use_state(|| false);

        let close_dialog = {
            let mut dialog_open = state.link_dialog_open.clone();
            let mut dialog_url = state.link_dialog_url.clone();
            let mut dialog_sender = state.link_dialog_sender.clone();
            EventHandler::new(move |_| {
                dialog_open.set(false);
                dialog_url.set(String::new());
                dialog_sender.set(String::new());
            })
        };

        let open_link = {
            let url = url.clone();
            let sender = sender.clone();
            let mut link_prefs = state.link_prefs.clone();
            let mut dialog_open = state.link_dialog_open.clone();
            let mut dialog_url = state.link_dialog_url.clone();
            let mut dialog_sender = state.link_dialog_sender.clone();
            let trust_sender = trust_sender.clone();
            let always_open = always_open.clone();
            EventHandler::new(move |_| {
                let ts = *trust_sender.read();
                let ao = *always_open.read();
                if ts {
                    link_prefs.write().trust_sender(&sender);
                }
                if ao {
                    link_prefs.write().set_always_open();
                }
                link_prefs.write().save_to_disk();
                dialog_open.set(false);
                dialog_url.set(String::new());
                dialog_sender.set(String::new());
                let _ = open::that(&url);
            })
        };

        // Dialog card with grainy texture matching the rest of the app
        rect()
            .width(Size::px(480.))
            .corner_radius(12.)
            .background(grain::dialog())
            .padding(20.)
            .vertical()
            .spacing(14.)
            .child(
                // Title
                label()
                    .text("Open External Link?")
                    .font_size(16.)
                    .font_weight(FontWeight::BOLD)
                    .color(DARK.text),
            )
            .child(
                // Warning message
                label()
                    .text(format!(
                        "This email from {} contains a link to an external website:",
                        sender
                    ))
                    .font_size(13.)
                    .color(DARK.muted),
            )
            .child(
                // Full URL display
                rect()
                    .width(Size::fill())
                    .padding(10.)
                    .corner_radius(6.)
                    .background((35, 35, 70))
                    .child(
                        label()
                            .text(url.clone())
                            .font_size(12.)
                            .color((100, 150, 255)),
                    ),
            )
            .child(
                // "Do not show again for this sender" checkbox
                rect()
                    .horizontal()
                    .spacing(8.)
                    .cross_align(Alignment::Center)
                    .cursor(CursorIcon::Pointer)
                    .on_press({
                        let mut ts = trust_sender.clone();
                        move |_e: Event<freya::prelude::PressEventData>| {
                            ts.set(!ts());
                        }
                    })
                    .child(
                        rect()
                            .width(Size::px(18.))
                            .height(Size::px(18.))
                            .corner_radius(4.)
                            .border(freya::prelude::Border::new().width(1.5).fill(DARK.muted))
                            .background(if trust_sender() { DARK.accent } else { (25, 25, 55) })
                            .center()
                            .child(
                                if trust_sender() {
                                    label().text("✓").color((255, 255, 255)).font_size(12.).font_weight(FontWeight::BOLD)
                                } else {
                                    label().text("")
                                },
                            ),
                    )
                    .child(
                        label()
                            .text("Do not show again for this sender")
                            .font_size(12.)
                            .color(DARK.muted),
                    ),
            )
            .child(
                // "Do not show again" checkbox
                rect()
                    .horizontal()
                    .spacing(8.)
                    .cross_align(Alignment::Center)
                    .cursor(CursorIcon::Pointer)
                    .on_press({
                        let mut ao = always_open.clone();
                        move |_e: Event<freya::prelude::PressEventData>| {
                            ao.set(!ao());
                        }
                    })
                    .child(
                        rect()
                            .width(Size::px(18.))
                            .height(Size::px(18.))
                            .corner_radius(4.)
                            .border(freya::prelude::Border::new().width(1.5).fill(DARK.muted))
                            .background(if always_open() { DARK.accent } else { (25, 25, 55) })
                            .center()
                            .child(
                                if always_open() {
                                    label().text("✓").color((255, 255, 255)).font_size(12.).font_weight(FontWeight::BOLD)
                                } else {
                                    label().text("")
                                },
                            ),
                    )
                    .child(
                        label()
                            .text("Do not show again")
                            .font_size(12.)
                            .color(DARK.muted),
                    ),
            )
            .child(
                // Divider
                rect()
                    .width(Size::fill())
                    .height(Size::px(1.))
                    .background(DARK.divider),
            )
            .child(
                // Buttons
                rect()
                    .horizontal()
                    .spacing(10.)
                    .content(Content::Flex)
                    .child(
                        // Cancel button
                        rect()
                            .padding((8., 18., 8., 18.))
                            .corner_radius(6.)
                            .background((35, 35, 70))
                            .cursor(CursorIcon::Pointer)
                            .on_press(close_dialog.clone())
                            .child(
                                label()
                                    .text("Cancel")
                                    .font_size(13.)
                                    .color(DARK.text),
                            ),
                    )
                    .child(
                        // Open button
                        rect()
                            .padding((8., 18., 8., 18.))
                            .corner_radius(6.)
                            .background(DARK.accent)
                            .cursor(CursorIcon::Pointer)
                            .on_press(open_link)
                            .child(
                                label()
                                    .text("Open Link")
                                    .font_size(13.)
                                    .font_weight(FontWeight::BOLD)
                                    .color((255, 255, 255)),
                            ),
                    ),
            )
    }
}
