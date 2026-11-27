//! HTML email rendering with security sandboxing and external link handling.
//!
//! Wraps Freya's `HtmlViewer` (Blitz-powered) to render email HTML safely:
//! - Sanitizes untrusted HTML with an HTML5 allow-list parser
//! - Blocks remote content until explicitly allowed for a message
//! - Intercepts link clicks and shows a confirmation dialog before opening externally
//! - Persists the user's global link-confirmation preference

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use ammonia::Builder as HtmlSanitizer;
use freya::html::*;
use freya::prelude::*;
use freya::winit::window::CursorIcon;
use regex::Regex;
use url::Url;

use crate::grain;
use crate::icons::INFO;
use crate::state::AppState;
use crate::theme::{DARK, ThemeMode};
use crate::widgets::{QuoteToggle, icon};

static RE_BG_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:background(?:-(?:color|image))?\s*:\s*|bgcolor\s*=\s*["']?)([^;}"']+)"#)
        .unwrap()
});
static RE_CSS_COLOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)#[0-9a-f]{3,8}\b|rgba?\([^)]*\)|\b(?:black|white|navy|darkblue|midnightblue|darkslategray|darkslategrey|gray|grey)\b").unwrap()
});
static RE_CSS_TEXT_COLOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[^\w-])(color\s*:\s*)([^;{}]+)").unwrap());
static RE_COLOR_SCHEME_DARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:color-scheme\s*:\s*[^;"}]*\bdark\b|<meta[^>]*name=["']color-scheme["'][^>]*dark|supported-color-schemes[^>]*dark)"#).unwrap()
});
static RE_STYLE_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style\b[^>]*>(.*?)</style\s*>").unwrap());
static RE_BODY_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<body\b((?:[^>"']|"[^"]*"|'[^']*')*)>"#).unwrap());
static RE_STYLE_ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bstyle="([^"]*)""#).unwrap());
static RE_RGBA_FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\brgba\(\s*[+-]?(?:\d+(?:\.\d*)?|\.\d+)%?\s*,\s*[+-]?(?:\d+(?:\.\d*)?|\.\d+)%?\s*,\s*[+-]?(?:\d+(?:\.\d*)?|\.\d+)%?\s*,\s*[+-]?(?:\d+(?:\.\d*)?|\.\d+)%?\s*\)").unwrap()
});
static RE_IMG_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<img\b[^>]*>").unwrap());
static RE_IMG_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bsrc="([^"]*)""#).unwrap());
static RE_IMG_ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\balt="([^"]*)""#).unwrap());
static RE_HTML_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:^|\s)([a-z_:][a-z0-9_:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
        .unwrap()
});

/// Parse a CSS/HTML color into (r, g, b). Returns None for unknown / transparent values.
fn parse_color(v: &str) -> Option<(u8, u8, u8)> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        let h: Vec<u8> = hex
            .chars()
            .filter_map(|c| c.to_digit(16).map(|d| d as u8))
            .collect();
        return match h.len() {
            3 => Some((h[0] * 17, h[1] * 17, h[2] * 17)),
            4 if h[3] != 0 => Some((h[0] * 17, h[1] * 17, h[2] * 17)),
            6 => Some((h[0] * 16 + h[1], h[2] * 16 + h[3], h[4] * 16 + h[5])),
            8 if h[6] != 0 || h[7] != 0 => {
                Some((h[0] * 16 + h[1], h[2] * 16 + h[3], h[4] * 16 + h[5]))
            }
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

/// Apply theme-aware text adjustments only when the message has no authored background.
fn has_authored_background(html: &str) -> bool {
    RE_BG_DECL.captures_iter(html).any(|background| {
        let value = background[1]
            .trim()
            .trim_end_matches("!important")
            .trim()
            .to_ascii_lowercase();
        !value.is_empty()
            && !matches!(
                value.as_str(),
                "transparent" | "none" | "initial" | "unset" | "revert"
            )
    })
}

fn adjust_css_text_colors(css: &str, lighten: bool) -> String {
    RE_CSS_TEXT_COLOR
        .replace_all(css, |captures: &regex::Captures<'_>| {
            let value = &captures[3];
            if value.to_ascii_lowercase().contains("var(") {
                return captures[0].to_owned();
            }
            let replacement = RE_CSS_COLOR
                .find_iter(value)
                .find_map(|color| parse_color(color.as_str()))
                .and_then(|color| {
                    let current_luminance = luminance(color);
                    let target_luminance = if lighten { 0.72 } else { 0.18 };
                    let needs_adjustment = if lighten {
                        current_luminance < 0.55
                    } else {
                        current_luminance > 0.45
                    };
                    needs_adjustment.then(|| {
                        let mix = if lighten {
                            ((target_luminance - current_luminance) / (1.0 - current_luminance))
                                .clamp(0.0, 1.0)
                        } else {
                            ((current_luminance - target_luminance) / current_luminance)
                                .clamp(0.0, 1.0)
                        };
                        let adjust = |channel: u8| {
                            if lighten {
                                (channel as f32 + (255.0 - channel as f32) * mix).round() as u8
                            } else {
                                (channel as f32 * (1.0 - mix)).round() as u8
                            }
                        };
                        let (red, green, blue) = color;
                        format!(
                            "#{:02x}{:02x}{:02x}",
                            adjust(red),
                            adjust(green),
                            adjust(blue)
                        )
                    })
                });
            let Some(replacement) = replacement else {
                return captures[0].to_owned();
            };
            let important = if value.to_ascii_lowercase().contains("!important") {
                "!important"
            } else {
                ""
            };
            format!("{}{}{replacement}{important}", &captures[1], &captures[2])
        })
        .into_owned()
}

fn adjust_email_text_colors(html: &str, lighten: bool) -> String {
    let styled_blocks = RE_STYLE_BLOCK.replace_all(html, |captures: &regex::Captures<'_>| {
        format!(
            "<style>{}</style>",
            adjust_css_text_colors(&captures[1], lighten)
        )
    });
    RE_STYLE_ATTRIBUTE
        .replace_all(&styled_blocks, |captures: &regex::Captures<'_>| {
            format!(
                "style=\"{}\"",
                adjust_css_text_colors(&captures[1], lighten)
            )
        })
        .into_owned()
}

/// Analyze an HTML body and return a sanitized version plus privacy warnings.
#[derive(Clone, Default, PartialEq)]
pub struct HtmlAnalysis {
    /// The sanitized HTML ready for rendering.
    pub sanitized: String,
    /// Number of resource attributes blocked by the active privacy policy.
    pub remote_resources_blocked: usize,
    /// Number of likely tracking-pixel image sources blocked by the active policy.
    pub tracking_pixels_blocked: usize,
}

/// Sanitize HTML content for safe rendering in an email client, blocking
/// external resources unless the user explicitly chooses to load them.
pub fn sanitize_html(html: &str) -> HtmlAnalysis {
    sanitize_html_for_theme(html, ThemeMode::Dark)
}

fn sanitize_html_for_theme(html: &str, theme: ThemeMode) -> HtmlAnalysis {
    sanitize_html_with_policy_and_theme(html, false, false, theme)
}

fn sanitize_html_with_policy(
    html: &str,
    allow_remote: bool,
    allow_tracking_pixels: bool,
) -> HtmlAnalysis {
    sanitize_html_with_policy_and_theme(html, allow_remote, allow_tracking_pixels, ThemeMode::Dark)
}

fn sanitize_html_with_policy_and_theme(
    html: &str,
    allow_remote: bool,
    allow_tracking_pixels: bool,
    theme: ThemeMode,
) -> HtmlAnalysis {
    let (mut result, remote_resources_blocked, tracking_pixels_blocked) =
        sanitize_fragment(html, allow_remote, allow_tracking_pixels);

    // Ammonia parses a fragment using the HTML5 parsing algorithm, so document
    // wrapper casing and doctype placement no longer affect style insertion.
    let has_background = has_authored_background(html);
    let has_dark_color_scheme = RE_COLOR_SCHEME_DARK.is_match(html);
    let adjust_text =
        !has_background && (!has_dark_color_scheme || matches!(theme, ThemeMode::Light));
    let style = if adjust_text {
        let (default_color, lighten) = match theme {
            ThemeMode::Light => ("#20222b", false),
            ThemeMode::Dark | ThemeMode::Black => ("#e5e7eb", true),
        };
        result = adjust_email_text_colors(&result, lighten);
        format!(
            "<style>html,body{{color:{default_color}!important}}a{{cursor:pointer!important}}</style>"
        )
    } else {
        "<style>a{cursor:pointer!important}</style>".to_string()
    };
    result.push_str(&style);
    result.push_str(
        "<style>html,body{min-width:0!important}img,video{max-width:100%!important}img{height:auto!important}</style>",
    );
    HtmlAnalysis {
        sanitized: result,
        remote_resources_blocked,
        tracking_pixels_blocked,
    }
}

/// Sanitize HTML before including it in a forwarded message. Remote resources
/// stay blocked so tracking URLs are not propagated to the recipient.
pub(crate) fn sanitize_forwarded_html(html: &str) -> String {
    sanitize_fragment(html, false, false).0
}

fn sanitize_fragment(
    html: &str,
    allow_remote: bool,
    allow_tracking_pixels: bool,
) -> (String, usize, usize) {
    let mut stylesheets = String::new();
    let mut body_styles = String::new();
    for body_tag in RE_BODY_TAG.captures_iter(html) {
        for attribute in RE_HTML_ATTRIBUTE.captures_iter(&body_tag[1]) {
            if !attribute[1].eq_ignore_ascii_case("style") {
                continue;
            }
            let style = attribute
                .get(2)
                .or_else(|| attribute.get(3))
                .or_else(|| attribute.get(4))
                .map(|value| value.as_str())
                .unwrap_or_default();
            body_styles.push_str("body{");
            body_styles.push_str(style);
            body_styles.push_str("}\n");
            break;
        }
    }
    let html_without_styles = RE_STYLE_BLOCK
        .replace_all(html, |captures: &regex::Captures<'_>| {
            stylesheets.push_str(&captures[1]);
            stylesheets.push('\n');
            ""
        })
        .into_owned();
    stylesheets.push_str(&body_styles);
    let blocked = Arc::new(AtomicUsize::new(0));
    let blocked_for_filter = Arc::clone(&blocked);
    let tracking_pixels_blocked = Arc::new(AtomicUsize::new(0));
    let tracking_pixels_blocked_for_filter = Arc::clone(&tracking_pixels_blocked);
    let protected_css_values = Arc::new(Mutex::new(HashMap::new()));
    let protected_css_values_for_filter = Arc::clone(&protected_css_values);
    let tracking_pixel_sources = tracking_pixel_sources(html);
    let mut sanitizer = HtmlSanitizer::default();
    sanitizer
        .clean_content_tags(HashSet::from(["script", "style", "title"]))
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .add_generic_attributes(&[
            "style",
            "class",
            "id",
            "align",
            "valign",
            "width",
            "height",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "colspan",
            "rowspan",
            "dir",
            "role",
        ])
        .add_generic_attribute_prefixes(&["data-"])
        .filter_style_properties(allowed_style_properties())
        .attribute_filter(move |tag, attribute, value| {
            if attribute == "href" && safe_external_url(value).is_none() {
                return None;
            }
            if attribute == "style" {
                let mut protected_css_values = protected_css_values_for_filter
                    .lock()
                    .expect("CSS value map lock");
                let style = protect_rgba_functions(value, &mut protected_css_values);
                return Some(Cow::Owned(sanitize_css_declarations(&style)));
            }
            let is_resource = matches!(attribute, "src" | "srcset" | "background" | "poster");
            let allowed_embedded_resource = is_embedded_resource_reference(value);
            let is_tracking_pixel = tag == "img"
                && attribute == "src"
                && normalized_web_resource_reference(value)
                    .is_some_and(|source| tracking_pixel_sources.contains(&source));
            if is_tracking_pixel && !allow_tracking_pixels {
                tracking_pixels_blocked_for_filter.fetch_add(1, Ordering::Relaxed);
            }
            let allowed_remote_resource = attribute != "srcset"
                && is_web_resource_reference(value)
                && !is_private_network_reference(value)
                && (if is_tracking_pixel {
                    allow_tracking_pixels
                } else {
                    allow_remote
                });
            if is_resource && !allowed_embedded_resource && !allowed_remote_resource {
                blocked_for_filter.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Some(Cow::Borrowed(value))
        });

    let cleaned_html = sanitizer.clean(&html_without_styles).to_string();
    let mut sanitized = render_blocked_image_fallbacks(&cleaned_html);
    restore_style_attribute_values(&mut sanitized, &protected_css_values);
    let sanitized_styles = sanitize_stylesheet(&stylesheets);
    if !sanitized_styles.is_empty() {
        sanitized.insert_str(0, &format!("<style>{sanitized_styles}</style>"));
    }
    let blocked = blocked.load(Ordering::Relaxed);
    let tracking_pixels_blocked = tracking_pixels_blocked.load(Ordering::Relaxed);
    (sanitized, blocked, tracking_pixels_blocked)
}

fn tracking_pixel_sources(html: &str) -> HashSet<String> {
    RE_IMG_TAG
        .captures_iter(html)
        .filter_map(|captures| {
            let image = &captures[0];
            let mut source = None;
            let mut width = None;
            let mut height = None;
            let mut hidden = false;

            for attribute in RE_HTML_ATTRIBUTE.captures_iter(image) {
                let name = attribute[1].to_ascii_lowercase();
                let value = attribute
                    .get(2)
                    .or_else(|| attribute.get(3))
                    .or_else(|| attribute.get(4))
                    .map(|value| value.as_str().trim())
                    .unwrap_or_default();
                match name.as_str() {
                    "src" => source = Some(value),
                    "width" => width = parse_image_dimension(value),
                    "height" => height = parse_image_dimension(value),
                    "style" => {
                        for declaration in value.split(';') {
                            let Some((property, value)) = declaration.split_once(':') else {
                                continue;
                            };
                            let property = property.trim();
                            let value = value.trim().to_ascii_lowercase();
                            match property.to_ascii_lowercase().as_str() {
                                "width" => width = parse_image_dimension(&value),
                                "height" => height = parse_image_dimension(&value),
                                "display" if value == "none" => hidden = true,
                                "visibility" if value == "hidden" => hidden = true,
                                "opacity" if value == "0" => hidden = true,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }

            let source = source?;
            if !is_web_resource_reference(source) || is_private_network_reference(source) {
                return None;
            }
            let is_tiny = matches!((width, height), (Some(width), Some(height)) if width <= 3. && height <= 3.);
            (hidden || is_tiny)
                .then(|| normalized_web_resource_reference(source))
                .flatten()
        })
        .collect()
}

fn parse_image_dimension(value: &str) -> Option<f32> {
    value
        .trim()
        .strip_suffix("px")
        .unwrap_or(value.trim())
        .parse::<f32>()
        .ok()
        .filter(|dimension| dimension.is_finite() && *dimension >= 0.)
}

fn normalized_web_resource_reference(value: &str) -> Option<String> {
    let decoded = value
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
        .replace("&#X26;", "&");
    let value = decoded.trim();
    let absolute = if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value.to_string()
    };
    let url = Url::parse(&absolute).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

fn render_blocked_image_fallbacks(html: &str) -> String {
    RE_IMG_TAG
        .replace_all(html, |captures: &regex::Captures<'_>| {
            let image = &captures[0];
            let has_source = RE_IMG_SOURCE
                .captures(image)
                .is_some_and(|source| !source[1].trim().is_empty());
            if has_source {
                return image.to_owned();
            }

            match RE_IMG_ALT.captures(image) {
                Some(alt) if !alt[1].trim().is_empty() => {
                    format!("<span>{}</span>", &alt[1])
                }
                _ => String::new(),
            }
        })
        .into_owned()
}

fn allowed_style_properties() -> HashSet<&'static str> {
    HashSet::from([
        "align-items",
        "background",
        "background-color",
        "border",
        "border-bottom",
        "border-bottom-color",
        "border-bottom-style",
        "border-bottom-width",
        "border-color",
        "border-left",
        "border-left-color",
        "border-left-style",
        "border-left-width",
        "border-radius",
        "border-right",
        "border-right-color",
        "border-right-style",
        "border-right-width",
        "border-style",
        "border-top",
        "border-top-color",
        "border-top-style",
        "border-top-width",
        "border-width",
        "box-shadow",
        "color",
        "display",
        "font",
        "font-family",
        "font-size",
        "font-style",
        "font-weight",
        "height",
        "letter-spacing",
        "line-height",
        "margin",
        "margin-bottom",
        "margin-left",
        "margin-right",
        "margin-top",
        "max-height",
        "max-width",
        "min-height",
        "min-width",
        "opacity",
        "overflow",
        "overflow-x",
        "overflow-y",
        "padding",
        "padding-bottom",
        "padding-left",
        "padding-right",
        "padding-top",
        "text-align",
        "text-decoration",
        "text-indent",
        "text-transform",
        "transition",
        "vertical-align",
        "visibility",
        "white-space",
        "width",
        "word-break",
        "word-wrap",
    ])
}

fn sanitize_stylesheet(css: &str) -> String {
    let mut sanitized = String::new();
    append_safe_css_rules(css, &mut sanitized);
    sanitized
}

fn append_safe_css_rules(css: &str, output: &mut String) {
    let mut cursor = 0;
    while let Some(open) = find_css_open_brace(css, cursor) {
        if let Some(semicolon) = css[cursor..open].rfind(';') {
            cursor += semicolon + 1;
            continue;
        }
        let prelude = css[cursor..open].trim();
        let Some(close) = find_css_close_brace(css, open) else {
            break;
        };
        let body = &css[open + 1..close];

        if is_safe_media_rule(prelude) {
            let mut nested = String::new();
            append_safe_css_rules(body, &mut nested);
            if !nested.is_empty() {
                output.push_str(prelude);
                output.push('{');
                output.push_str(&nested);
                output.push('}');
            }
        } else if !prelude.starts_with('@') && is_safe_css_selector(prelude) {
            let mut protected_values = HashMap::new();
            let protected_body = protect_rgba_functions(body, &mut protected_values);
            let declarations = sanitize_css_declarations(&protected_body);
            let declarations = restore_css_values(&declarations, &protected_values);
            if !declarations.is_empty() {
                output.push_str(prelude);
                output.push('{');
                output.push_str(&declarations);
                output.push('}');
            }
        }

        cursor = close + 1;
    }
}

fn find_css_open_brace(css: &str, start: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let mut index = start;
    let mut quote = None;
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = skip_css_comment(bytes, index)?;
            continue;
        }
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => parentheses += 1,
                b')' => parentheses = parentheses.saturating_sub(1),
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'{' if parentheses == 0 && brackets == 0 => return Some(index),
                b'}' if parentheses == 0 && brackets == 0 => return None,
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn find_css_close_brace(css: &str, open: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let mut index = open + 1;
    let mut depth = 1usize;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = skip_css_comment(bytes, index)?;
            continue;
        }
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn skip_css_comment(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            return Some(index + 2);
        }
        index += 1;
    }
    None
}

fn is_safe_css_selector(selector: &str) -> bool {
    !selector.is_empty()
        && selector.len() <= 4096
        && !selector
            .chars()
            .any(|character| character.is_control() || matches!(character, '<' | '>' | '\\' | '@'))
}

fn is_safe_media_rule(rule: &str) -> bool {
    let Some(condition) = rule.strip_prefix("@media") else {
        return false;
    };
    !condition.trim().is_empty()
        && condition.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(
                    character,
                    '-' | '_' | ':' | '(' | ')' | '.' | ',' | '%' | '/'
                )
        })
}

fn sanitize_css_declarations(declarations: &str) -> String {
    let escaped = declarations
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let input = format!("<span style=\"{escaped}\"></span>");
    let mut sanitizer = HtmlSanitizer::default();
    sanitizer
        .add_generic_attributes(&["style"])
        .filter_style_properties(allowed_style_properties())
        .url_schemes(HashSet::new())
        .url_relative(ammonia::UrlRelative::Deny);
    let clean = sanitizer.clean(&input).to_string();
    let Some(captures) = RE_STYLE_ATTRIBUTE.captures(&clean) else {
        return String::new();
    };
    let style = decode_style_attribute(&captures[1]);
    let lowercase = style.to_ascii_lowercase();
    if style
        .chars()
        .any(|character| matches!(character, '<' | '>'))
        || lowercase.contains("url(")
        || lowercase.contains("expression(")
        || lowercase.contains("-moz-binding")
    {
        String::new()
    } else {
        style
    }
}

fn protect_rgba_functions(css: &str, protected_values: &mut HashMap<String, String>) -> String {
    let mut protected = String::with_capacity(css.len());
    let mut cursor = 0;
    for captures in RE_RGBA_FUNCTION.captures_iter(css) {
        let Some(function) = captures.get(0) else {
            continue;
        };
        protected.push_str(&css[cursor..function.start()]);
        let placeholder = format!("photonmailrgba{}", protected_values.len());
        protected_values.insert(placeholder.clone(), function.as_str().to_owned());
        protected.push_str(&placeholder);
        cursor = function.end();
    }
    protected.push_str(&css[cursor..]);
    protected
}

fn restore_css_values(css: &str, protected_values: &HashMap<String, String>) -> String {
    protected_values
        .iter()
        .fold(css.to_owned(), |css, (placeholder, value)| {
            css.replace(placeholder, value)
        })
}

fn restore_style_attribute_values(
    html: &mut String,
    protected_values: &Arc<Mutex<HashMap<String, String>>>,
) {
    let Ok(values) = protected_values.lock() else {
        return;
    };
    if values.is_empty() {
        return;
    }
    *html = RE_STYLE_ATTRIBUTE
        .replace_all(html, |captures: &regex::Captures<'_>| {
            format!("style=\"{}\"", restore_css_values(&captures[1], &values))
        })
        .into_owned();
}

fn decode_style_attribute(style: &str) -> String {
    style
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#x22;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn is_embedded_resource_reference(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with('#') || value.starts_with("cid:") || value.starts_with("data:")
}

fn is_web_resource_reference(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("http://") || value.starts_with("https://") || value.starts_with("//")
}

fn is_private_network_reference(value: &str) -> bool {
    let value = value.trim();
    let lowercase = value.to_ascii_lowercase();
    let absolute = if value.starts_with("//") {
        format!("https:{value}")
    } else if lowercase.starts_with("http:") || lowercase.starts_with("https:") {
        value.to_string()
    } else {
        return false;
    };
    let Ok(url) = Url::parse(&absolute) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
    {
        return true;
    }

    let ip_host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(&host);
    ip_host
        .parse::<IpAddr>()
        .is_ok_and(|address| match address {
            IpAddr::V4(address) => {
                address.is_private()
                    || address.is_loopback()
                    || address.is_link_local()
                    || address.is_unspecified()
                    || address.is_broadcast()
                    || address.is_multicast()
            }
            IpAddr::V6(address) => {
                address.is_loopback()
                    || address.is_unique_local()
                    || address.is_unicast_link_local()
                    || address.is_unspecified()
                    || address.is_multicast()
                    || address.to_ipv4_mapped().is_some_and(|mapped| {
                        mapped.is_private()
                            || mapped.is_loopback()
                            || mapped.is_link_local()
                            || mapped.is_unspecified()
                            || mapped.is_broadcast()
                            || mapped.is_multicast()
                    })
            }
        })
}

fn safe_external_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https" | "mailto").then(|| parsed.to_string())
}

/// A secure HTML viewer component for rendering email bodies.
/// Wraps `HtmlViewer` with pre-sanitization, security warnings, link click interception,
/// and pointer cursor on hover over links.
#[derive(PartialEq)]
pub struct SecureHtmlViewer {
    pub html: String,
    /// Sender address displayed in the confirmation dialog and optional sender preference.
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
        let remote_content_allowed = use_state(|| false);
        let tracking_pixels_allowed = use_state(|| false);
        let mut show_privacy_explanation = use_state(|| false);
        let privacy_tooltip_position = use_state(|| (0.0_f32, 0.0_f32));

        // Sanitize the HTML content
        let html_for_analysis = html.clone();
        let remote_allowed_for_analysis = remote_content_allowed.clone();
        let tracking_allowed_for_analysis = tracking_pixels_allowed.clone();
        let state_for_analysis = state;
        let analysis = use_memo(move || {
            let settings = state_for_analysis.settings.read();
            let allow_remote = !settings.block_remote_images || *remote_allowed_for_analysis.read();
            let allow_tracking_pixels =
                !settings.block_tracking_pixels || *tracking_allowed_for_analysis.read();
            let theme = settings.theme;
            if allow_remote || allow_tracking_pixels {
                sanitize_html_with_policy_and_theme(
                    &html_for_analysis,
                    allow_remote,
                    allow_tracking_pixels,
                    theme,
                )
            } else {
                sanitize_html_for_theme(&html_for_analysis, theme)
            }
        });
        let settings = state.settings.read();
        let allow_remote = !settings.block_remote_images || *remote_content_allowed.read();
        let allow_tracking_pixels =
            !settings.block_tracking_pixels || *tracking_pixels_allowed.read();
        let sanitized_ref = analysis.read();
        let sanitized = sanitized_ref.sanitized.clone();
        let remote_resources_blocked = sanitized_ref.remote_resources_blocked;
        let tracking_pixels_blocked = sanitized_ref.tracking_pixels_blocked;

        let has_warnings = remote_resources_blocked > 0;

        // Initialize the HtmlHandle with the sanitized HTML.
        // When the email changes, the parent keys this component by the mail index,
        // forcing a full remount so use_hook runs again with the new HTML.
        let mut handle = use_html(|| HtmlSource::html(sanitized.clone()));
        let mut rendered_html = use_state(|| sanitized.clone());
        if *rendered_html.read() != sanitized {
            handle.replace_html(sanitized.clone());
            rendered_html.set(sanitized.clone());
        }
        handle.set_navigation_interception(true);

        // Watch for link clicks without navigating away from the email document.
        // Use create_sync so the first run is immediate and establishes reactive deps right away.
        use_hook(|| {
            let mut handle = handle;
            let sender_email = sender_email.clone();
            let mut dialog_url = state.link_dialog_url.clone();
            let mut dialog_sender = state.link_dialog_sender.clone();
            let mut dialog_open = state.link_dialog_open.clone();
            let link_prefs = state.link_prefs.clone();

            Effect::create_sync(move || {
                let url = match handle.take_intercepted_navigation() {
                    Some(u) => u,
                    None => return,
                };

                let Some(url) = safe_external_url(&url) else {
                    return;
                };

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

        let mut viewer = HtmlViewer::new(handle).height(Size::flex(1.));
        if quote_toggle_idx.is_some() {
            // Report where the spacer is so the native button can be drawn over it
            viewer = viewer.track(crate::mail::quoting::ANCHOR_ID);
        }

        let mut col = rect()
            .height(Size::fill())
            .width(Size::fill())
            .vertical()
            .content(Content::Flex)
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
                    .child(QuoteToggle {
                        state,
                        mail_idx: idx,
                    }),
            );
        }

        if has_warnings {
            let warning_text = format!(
                "{} remote resource{} blocked; {} likely tracking pixel{} blocked",
                remote_resources_blocked,
                if remote_resources_blocked == 1 {
                    ""
                } else {
                    "s"
                },
                tracking_pixels_blocked,
                if tracking_pixels_blocked == 1 {
                    ""
                } else {
                    "s"
                }
            );
            let warning_details = rect().width(Size::flex(1.)).vertical().spacing(4.).child(
                paragraph()
                    .width(Size::fill())
                    .span(warning_text)
                    .color(DARK.warning)
                    .font_size(11.),
            );
            let root_size = *Platform::get().current_window().root_size.read();
            let (pointer_x, pointer_y) = privacy_tooltip_position();
            let tooltip_width = (root_size.width - 32.).min(260.).max(0.);
            let max_tooltip_x = (root_size.width - tooltip_width - 24.).max(8.);
            let tooltip_x = if pointer_x + 14. <= max_tooltip_x {
                pointer_x + 14.
            } else {
                pointer_x - tooltip_width - 30.
            }
            .clamp(8., max_tooltip_x);
            let tooltip_height = 48.;
            let max_tooltip_y = (root_size.height - tooltip_height - 8.).max(8.);
            let tooltip_y = if pointer_y + 10. + tooltip_height <= root_size.height - 8. {
                pointer_y + 10.
            } else {
                pointer_y - tooltip_height - 8.
            }
            .clamp(8., max_tooltip_y);
            let mut privacy_info = rect()
                .width(Size::px(24.))
                .height(Size::px(24.))
                .center()
                .on_pointer_enter(move |_| show_privacy_explanation.set(true))
                .on_pointer_leave(move |_| show_privacy_explanation.set(false))
                .on_pointer_move({
                    let mut position = privacy_tooltip_position.clone();
                    move |e: Event<PointerEventData>| {
                        let pointer = e.global_location();
                        position.set((pointer.x as f32, pointer.y as f32));
                    }
                })
                .child(icon(INFO, 16., DARK.warning));
            if show_privacy_explanation() {
                privacy_info = privacy_info.child(
                    rect()
                        .layer(Layer::Overlay)
                        .position(
                            Position::new_global()
                                .top(tooltip_y)
                                .left(tooltip_x),
                        )
                        .width(Size::px(tooltip_width))
                        .padding(8.)
                        .corner_radius(6.)
                        .background(DARK.panel)
                        .child(
                            label()
                                .text("Loading remote content may reveal your IP address and that you opened this message.")
                                .color(DARK.text)
                                .font_size(11.),
                        ),
                );
            }
            let mut warning_actions = rect()
                .horizontal()
                .spacing(8.)
                .cross_align(Alignment::Center);

            if !allow_remote {
                let mut handle = handle;
                let mut remote_content_allowed = remote_content_allowed.clone();
                let html_with_remote =
                    sanitize_html_with_policy(&html, true, allow_tracking_pixels).sanitized;
                warning_actions = warning_actions.child(
                    rect()
                        .padding((3., 8., 3., 8.))
                        .corner_radius(4.)
                        .background(DARK.hover_strong)
                        .cursor(CursorIcon::Pointer)
                        .on_press(move |_| {
                            handle.replace_html(html_with_remote.clone());
                            remote_content_allowed.set(true);
                        })
                        .child(
                            label()
                                .text("Load remote content")
                                .color(DARK.text)
                                .font_size(11.),
                        ),
                );
            }
            if tracking_pixels_blocked > 0 {
                let mut handle = handle;
                let mut tracking_pixels_allowed = tracking_pixels_allowed.clone();
                let html_with_tracking_pixels =
                    sanitize_html_with_policy(&html, allow_remote, true).sanitized;
                warning_actions = warning_actions.child(
                    rect()
                        .padding((3., 8., 3., 8.))
                        .corner_radius(4.)
                        .background(DARK.hover_strong)
                        .cursor(CursorIcon::Pointer)
                        .on_press(move |_| {
                            handle.replace_html(html_with_tracking_pixels.clone());
                            tracking_pixels_allowed.set(true);
                        })
                        .child(
                            label()
                                .text("Show tracking pixels")
                                .color(DARK.text)
                                .font_size(11.),
                        ),
                );
            }
            let warning = rect()
                .width(Size::fill())
                .padding((6., 10., 6., 10.))
                .corner_radius(6.)
                .background(DARK.warning_bg)
                .horizontal()
                .content(Content::Flex)
                .spacing(8.)
                .cross_align(Alignment::Center)
                .child(privacy_info)
                .child(warning_details)
                .child(warning_actions);
            col = col.child(warning);
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
        let always_open = use_state(|| false);
        let sender_open = use_state(|| false);

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
            let mut link_prefs = state.link_prefs.clone();
            let mut dialog_open = state.link_dialog_open.clone();
            let mut dialog_url = state.link_dialog_url.clone();
            let mut dialog_sender = state.link_dialog_sender.clone();
            let always_open = always_open.clone();
            let sender_open = sender_open.clone();
            let sender = sender.clone();
            EventHandler::new(move |_| {
                let ao = *always_open.read();
                let so = *sender_open.read();
                if ao || so {
                    {
                        let mut prefs = link_prefs.write();
                        if ao {
                            prefs.set_always_open();
                        }
                        if so {
                            prefs.trust_sender(&sender);
                        }
                    }
                    link_prefs.read().save_to_disk();
                }
                dialog_open.set(false);
                dialog_url.set(String::new());
                dialog_sender.set(String::new());
                if let Some(url) = safe_external_url(&url) {
                    let _ = open::that(url);
                }
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
                        "This email from {} contains an external link:",
                        sender
                    ))
                    .font_size(13.)
                    .color(DARK.muted),
            )
            .maybe_child((!sender.trim().is_empty()).then(|| {
                label()
                    .text("The message's From address is not authenticated.")
                    .font_size(11.)
                    .color(DARK.muted)
            }))
            .child(
                // Full URL display
                rect()
                    .width(Size::fill())
                    .padding(10.)
                    .corner_radius(6.)
                    .background(DARK.hover)
                    .child(label().text(url.clone()).font_size(12.).color(DARK.accent)),
            )
            .maybe_child((!sender.trim().is_empty()).then(|| {
                link_preference_option("Do not show again for this sender", sender_open.clone())
            }))
            .child(link_preference_option(
                "Do not show again (all senders)",
                always_open.clone(),
            ))
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
                            .background(DARK.hover)
                            .cursor(CursorIcon::Pointer)
                            .on_press(close_dialog.clone())
                            .child(label().text("Cancel").font_size(13.).color(DARK.text)),
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

fn link_preference_option(text: &'static str, checked: State<bool>) -> Element {
    let is_checked = *checked.read();
    let mut checked_state = checked;
    rect()
        .horizontal()
        .spacing(8.)
        .cross_align(Alignment::Center)
        .cursor(CursorIcon::Pointer)
        .on_press(move |_event: Event<freya::prelude::PressEventData>| {
            checked_state.set(!checked_state());
        })
        .child(
            rect()
                .width(Size::px(18.))
                .height(Size::px(18.))
                .corner_radius(4.)
                .border(freya::prelude::Border::new().width(1.5).fill(DARK.muted))
                .background(if is_checked {
                    DARK.accent
                } else {
                    (30, 30, 40)
                })
                .center()
                .child(if is_checked {
                    label()
                        .text("✓")
                        .color((255, 255, 255))
                        .font_size(12.)
                        .font_weight(FontWeight::BOLD)
                } else {
                    label().text("")
                }),
        )
        .child(label().text(text).font_size(12.).color(DARK.muted))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html5_sanitization_removes_obfuscated_active_markup_without_rewriting_text() {
        let html = concat!(
            "<p>javascript: is mentioned as plain text.</p>",
            "<SCRIPT>script text should be removed</SCRIPT >",
            "<script>unclosed script text should also be removed",
            "<img/onerror=alert(1) src='x'>",
            "<img src=x onerror=alert(2)>",
            "<a href='javascript:alert(3)'>unsafe link</a>",
        );

        let sanitized = sanitize_html(html).sanitized.to_ascii_lowercase();

        assert!(sanitized.contains("javascript: is mentioned as plain text."));
        assert!(!sanitized.contains("script text"));
        assert!(!sanitized.contains("onerror"));
        assert!(!sanitized.contains("href=\"javascript:"));
    }

    #[test]
    fn remote_images_are_blocked_by_default_but_tracking_pixels_require_separate_opt_in() {
        let html = concat!(
            "<img src='https://tracker.example/pixel' width='1' alt='open tracker' data-id='abc' height='1'>",
            "<img src='https://example.test/collections/banner.png' width='500' height='120'>",
            "<img src='https://spacer.example/spacer.gif' width='1' height='20'>",
        );

        let blocked = sanitize_html(html);
        let allowed = sanitize_html_with_policy(html, true, false);
        let trackers_only = sanitize_html_with_policy(html, false, true);
        let all_allowed = sanitize_html_with_policy(html, true, true);

        assert_eq!(blocked.remote_resources_blocked, 3);
        assert_eq!(blocked.tracking_pixels_blocked, 1);
        assert!(!blocked.sanitized.contains("https://tracker.example"));
        assert!(!blocked.sanitized.contains("https://example.test"));
        assert_eq!(allowed.remote_resources_blocked, 1);
        assert_eq!(allowed.tracking_pixels_blocked, 1);
        assert!(!allowed.sanitized.contains("https://tracker.example/pixel"));
        assert!(
            allowed
                .sanitized
                .contains("https://example.test/collections/banner.png")
        );
        assert!(
            allowed
                .sanitized
                .contains("https://spacer.example/spacer.gif")
        );
        assert_eq!(trackers_only.remote_resources_blocked, 2);
        assert_eq!(trackers_only.tracking_pixels_blocked, 0);
        assert!(
            trackers_only
                .sanitized
                .contains("https://tracker.example/pixel")
        );
        assert!(
            !trackers_only
                .sanitized
                .contains("https://example.test/collections/banner.png")
        );
        assert_eq!(all_allowed.remote_resources_blocked, 0);
        assert_eq!(all_allowed.tracking_pixels_blocked, 0);
        assert!(
            all_allowed
                .sanitized
                .contains("https://tracker.example/pixel")
        );
    }

    #[test]
    fn hidden_remote_images_are_kept_blocked_with_remote_content_opted_in() {
        let html = "<img src='https://tracker.example/open?id=123' style='display:none' width='300' height='20'>";

        let allowed = sanitize_html_with_policy(html, true, false);

        assert_eq!(allowed.remote_resources_blocked, 1);
        assert_eq!(allowed.tracking_pixels_blocked, 1);
        assert!(!allowed.sanitized.contains("https://tracker.example/open"));
    }

    #[test]
    fn blocked_remote_images_keep_their_alt_text_visible() {
        let analysis = sanitize_html(
            "<img src='https://cdn.example.test/logo.png' alt='LinkedIn' width='80' height='24'>",
        );

        assert_eq!(analysis.remote_resources_blocked, 1);
        assert!(!analysis.sanitized.contains("cdn.example.test"));
        assert!(analysis.sanitized.contains("<span>LinkedIn</span>"));
        assert!(!analysis.sanitized.contains("<img"));
    }

    #[test]
    fn responsive_sizing_does_not_override_nested_email_tables() {
        let sanitized = sanitize_html(
            "<table width='600'><tr><td><table width='280'><tr><td>content</td></tr></table></td></tr></table>",
        )
        .sanitized;

        assert!(!sanitized.contains("img,table,video{max-width:100%!important}"));
        assert!(sanitized.contains("img,video{max-width:100%!important}"));
    }

    #[test]
    fn safe_email_stylesheets_are_kept_without_enabling_css_resources() {
        let html = concat!(
            "<style>",
            "body { width: 720px; max-width: 720px; margin: 0 auto; }",
            ".message { width: 720px; color: #333; background-image: url(https://tracker.example/bg); }",
            "@import url(https://tracker.example/import.css);",
            "@media screen and (max-width: 600px) { .message { max-width: 100%; } }",
            "</style><div class='message'>Message</div>"
        );

        let sanitized = sanitize_html(html).sanitized;

        assert!(sanitized.contains(".message"));
        assert!(sanitized.contains("width:720px"));
        assert!(sanitized.contains("color:#333"));
        assert!(sanitized.contains("@media screen and (max-width: 600px)"));
        assert!(sanitized.contains("max-width:100%"));
        assert!(!sanitized.contains("url("));
        assert!(!sanitized.contains("@import"));
        assert!(sanitized.contains("body{width:720px;max-width:720px;margin:0 auto}"));
        assert!(!sanitized.contains("html,body{width:100%!important"));
    }

    #[test]
    fn dark_text_is_lightened_without_inverting_backgroundless_emails() {
        let sanitized = sanitize_html("<p style=\"color:#111\">Message text</p>").sanitized;

        assert!(sanitized.contains("color:#b8b8b8"), "{sanitized}");
        assert!(
            sanitized.contains("html,body{color:#e5e7eb!important}"),
            "{sanitized}"
        );
        assert!(!sanitized.contains("filter:invert"), "{sanitized}");
    }

    #[test]
    fn light_mode_keeps_backgroundless_email_text_dark_and_readable() {
        let sanitized =
            sanitize_html_for_theme("<p style=\"color:#fff\">Message text</p>", ThemeMode::Light)
                .sanitized;

        assert!(
            sanitized.contains("html,body{color:#20222b!important}"),
            "{sanitized}"
        );
        assert!(sanitized.contains("color:#2e2e2e"), "{sanitized}");
        assert!(!sanitized.contains("color:#e5e7eb"), "{sanitized}");
    }

    #[test]
    fn light_mode_adjusts_dark_scheme_text_without_an_authored_background() {
        let sanitized = sanitize_html_for_theme(
            "<style>html{color-scheme:dark}.copy{color:#fff}</style><p class=\"copy\">Text</p>",
            ThemeMode::Light,
        )
        .sanitized;

        assert!(
            sanitized.contains("html,body{color:#20222b!important}"),
            "{sanitized}"
        );
        assert!(sanitized.contains("color:#2e2e2e"), "{sanitized}");
    }

    #[test]
    fn white_background_emails_keep_their_authored_background_and_text_colors() {
        let sanitized = sanitize_html(concat!(
            "<style>body{background-color:#fff}.copy{color:#222}</style>",
            "<body><p class=\"copy\">Message text</p></body>",
        ))
        .sanitized;

        assert!(sanitized.contains("background-color:#fff"), "{sanitized}");
        assert!(sanitized.contains("color:#222"), "{sanitized}");
        assert!(!sanitized.contains("html,body{color:"), "{sanitized}");
        assert!(!sanitized.contains("filter:invert"), "{sanitized}");
    }

    #[test]
    fn common_inline_email_background_styles_survive_sanitization() {
        let sanitized = sanitize_html(concat!(
            "<body style=\"margin:0;padding:0;font-family:'Helvetica Neue',Arial,sans-serif; ",
            "background:linear-gradient(to bottom,#020617,#000000);color:#ededed\">",
            "<table style=\"background:linear-gradient(to bottom, rgba(30, 58, 138, 0.8), ",
            "rgba(15, 23, 42, 0.9)); border-radius:12px;overflow:hidden; ",
            "box-shadow:0 10px 40px rgba(0, 0, 0, 0.5)\">",
            "<a style=\"display:inline-block;padding:16px 48px;background:#2563eb;color:#fff; ",
            "text-decoration:none;border-radius:8px;box-shadow:0 4px 14px rgba(37,99,235,.4); ",
            "transition:background .2s\">Confirm</a></table></body>",
        ))
        .sanitized;

        assert!(sanitized.contains("body{margin:0;padding:0"), "{sanitized}");
        assert!(
            sanitized.contains("background:linear-gradient"),
            "{sanitized}"
        );
        assert!(sanitized.contains("rgba(30, 58, 138, 0.8)"), "{sanitized}");
        assert!(sanitized.contains("rgba(15, 23, 42, 0.9)"), "{sanitized}");
        assert!(sanitized.contains("box-shadow:0 10px 40px"), "{sanitized}");
        assert!(
            sanitized.contains("transition:background 0.2s"),
            "{sanitized}"
        );
        assert!(!sanitized.contains("photonmailrgba"), "{sanitized}");
        assert!(!sanitized.contains("filter:invert(1)"), "{sanitized}");
    }

    #[test]
    fn remote_css_and_non_web_link_schemes_are_removed() {
        let html = concat!(
            "<style>@import url('https://tracker.example/style.css');</style>",
            "<p style=\"background:URL(https://tracker.example/bg.png);color:#333\">text</p>",
            "<a href='file:///etc/passwd'>file</a>",
            "<a href='/relative/path'>relative</a>",
            "<a href='cid:embedded@example.test'>cid</a>",
            "<a href='mailto:someone@example.test'>mail</a>",
        );

        let sanitized = sanitize_html(html).sanitized;

        assert!(!sanitized.contains("tracker.example"));
        assert!(!sanitized.contains("file:///"));
        assert!(!sanitized.contains("href=\"/relative/path\""));
        assert!(!sanitized.contains("cid:embedded@example.test"));
        assert!(sanitized.contains("href=\"mailto:someone@example.test\""));
        assert!(sanitized.contains("color"));
    }

    #[test]
    fn explicitly_allowed_remote_content_still_cannot_reach_local_network_hosts() {
        let html = concat!(
            "<img src='http://127.0.0.1/private'>",
            "<img src='http://192.168.1.1/admin'>",
            "<img src='http://[::1]/private'>",
            "<img src='http://printer.local./status'>",
            "<img src='HTTPS://127.0.0.1./private'>",
            "<img srcset='https://cdn.example.com/image.png 1x, http://127.0.0.1/pixel 2x'>",
            "<img src='https://cdn.example.com/image.png'>",
        );

        let allowed = sanitize_html_with_policy(html, true, false);

        assert_eq!(allowed.remote_resources_blocked, 5);
        assert!(!allowed.sanitized.contains("127.0.0.1"));
        assert!(!allowed.sanitized.contains("192.168.1.1"));
        assert!(!allowed.sanitized.contains("[::1]"));
        assert!(!allowed.sanitized.contains("printer.local"));
        assert!(!allowed.sanitized.contains("127.0.0.1."));
        assert!(!allowed.sanitized.contains("srcset"));
        assert!(
            allowed
                .sanitized
                .contains("https://cdn.example.com/image.png")
        );
    }

    #[test]
    fn style_injection_handles_uppercase_wrappers_and_doctype() {
        let sanitized = sanitize_html("<!DOCTYPE html><HTML><BODY>hello</BODY></HTML>").sanitized;

        assert!(sanitized.contains("<style>"));
        assert!(sanitized.contains("max-width:100%!important"));
        assert!(!sanitized.to_ascii_lowercase().contains("doctype"));
        assert!(sanitized.contains("hello"));
    }

    #[test]
    fn document_title_is_not_rendered_as_message_body_text() {
        let subject = "Hoffman Construction Company mentioned you in a comment.";
        let html = format!(
            "<!doctype html><html><head><title>{subject}</title></head><body><p>Message body</p></body></html>"
        );

        let sanitized = sanitize_html(&html).sanitized;

        assert!(!sanitized.contains(subject));
        assert!(sanitized.contains("Message body"));
    }

    #[test]
    fn hidden_preheader_styles_survive_sanitization() {
        let sanitized = sanitize_html(
            "<style>.preheader{display:none;max-height:0;max-width:0;overflow:hidden;opacity:0;visibility:hidden}</style><span class='preheader'>Hidden preview text</span><table><tr><td>Visible email</td></tr></table>",
        )
        .sanitized;

        assert!(sanitized.contains("display:none"));
        assert!(sanitized.contains("overflow:hidden"));
        assert!(sanitized.contains("opacity:0"));
        assert!(sanitized.contains("visibility:hidden"));
        assert!(sanitized.contains("Hidden preview text"));
        assert!(sanitized.contains("Visible email"));

        let inline_styled = sanitize_html(
            "<span style='display:none;max-height:0;max-width:0;overflow:hidden;opacity:0;visibility:hidden'>Inline preview text</span>",
        )
        .sanitized;
        assert!(inline_styled.contains("overflow:hidden"));
        assert!(inline_styled.contains("opacity:0"));
        assert!(inline_styled.contains("visibility:hidden"));
    }

    #[test]
    fn forwarded_html_is_sanitized_and_link_launch_schemes_are_limited() {
        let forwarded = sanitize_forwarded_html(
            "<script>secret()</script><img src='https://tracker.example/pixel'><p>forward</p>",
        );

        assert!(!forwarded.contains("<script"));
        assert!(!forwarded.contains("tracker.example"));
        assert!(forwarded.contains("forward"));
        assert!(safe_external_url("https://example.com").is_some());
        assert!(safe_external_url("mailto:someone@example.com").is_some());
        assert!(safe_external_url("javascript:alert(1)").is_none());
        assert!(safe_external_url("file:///etc/passwd").is_none());
        assert!(safe_external_url("data:text/html,hi").is_none());
    }
}
