//! Detect quoted reply/forward sections in email bodies.
//!
//! Splits an email body into "new content" (the reply text) and "quoted content"
//! (the original message being replied to). This enables collapsed reply rendering
//! where quoted text is hidden by default with a toggle to expand it.

use std::sync::LazyLock;

use regex::Regex;

/// `id` of the empty spacer inserted where the quoted section starts. The HTML viewer tracks
/// it (`HtmlViewer::track`) and draws the native "..." button on top of it.
pub const ANCHOR_ID: &str = "photon-quote-anchor";

/// Start tag of a `<blockquote>`.
static RE_BLOCKQUOTE_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<blockquote\b").unwrap());
/// Start tag of a Gmail quote container (`<div class="gmail_quote ...">`). Requiring the
/// tag avoids matching the class name inside a `<style>` block.
static RE_GMAIL_QUOTE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<(?:div|blockquote)\b[^>]*\bclass\s*=\s*["'][^"']*\bgmail_quote\b"#).unwrap()
});
/// A line of an HTML body that starts with an encoded `>` (plain-text style quoting).
static RE_GT_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|<br\s*/?>|<p[^>]*>|<div[^>]*>|\n)\s*&gt;").unwrap());

/// Whether a message is an actual reply to another message.
///
/// Quoted-text collapsing only applies to replies. A message is a reply when it carries
/// an `In-Reply-To` header or its subject starts with a reply prefix (`Re:`). Forwards,
/// newsletters and ordinary mail that merely contains `>` characters or blockquotes are
/// left untouched.
pub fn is_reply(in_reply_to: &str, subject: &str) -> bool {
    if !in_reply_to.trim().is_empty() {
        return true;
    }
    let subject = subject.trim_start().to_ascii_lowercase();
    subject.starts_with("re:") || subject.starts_with("re[")
}

/// Result of parsing an email body for quoted sections.
#[derive(Clone, Debug, Default)]
pub struct QuotedSplit {
    /// The new content (reply text, forward text, or the entire message if no quotes).
    pub new_content: String,
    /// The quoted content (original message being replied to).
    pub quoted_content: String,
    /// Whether the body contains quoted sections.
    pub has_quotes: bool,
}

/// Parse a plain text email body and split it into new content and quoted content.
///
/// Detects standard email quoting patterns:
/// - `> text` — single-level quotes
/// - `>> text` — nested quotes
/// - `On {date}, {sender} wrote:` — reply headers (treated as quoted)
/// - `--- Forwarded message ---` — forward headers (treated as quoted)
///
/// The algorithm scans from the bottom of the message upward, collecting quoted
/// lines until it reaches the first non-quoted line (the new content).
pub fn split_plain_text(body: &str) -> QuotedSplit {
    let lines: Vec<&str> = body.lines().collect();
    let len = lines.len();
    if len == 0 {
        return QuotedSplit {
            new_content: body.to_string(),
            quoted_content: String::new(),
            has_quotes: false,
        };
    }

    // Find where the quoted section starts by scanning from the bottom.
    // Quoted lines start with '>'. A reply header like "On {date}, {sender} wrote:"
    // is treated as part of the quoted section if followed by quoted lines.
    let mut quoted_start = len; // Start assuming no quotes
    let mut i = len.saturating_sub(1);

    // Skip trailing empty lines
    while i < len && lines[i].trim().is_empty() {
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // Scan upward looking for quoted lines
    while i < len {
        let line = lines[i];
        let trimmed = line.trim_start();

        // Check if this is a quoted line (starts with >)
        if trimmed.starts_with('>') {
            quoted_start = i;
            if i == 0 {
                break;
            }
            i -= 1;
            continue;
        }

        // Check for reply headers like "On {date}, {sender} wrote:"
        // These typically appear right before quoted lines
        if quoted_start < len && is_reply_header(trimmed) {
            quoted_start = i;
            if i == 0 {
                break;
            }
            i -= 1;
            continue;
        }

        // If we've found quoted lines below, an empty line between them is still part of the quote
        if quoted_start < len && trimmed.is_empty() {
            // Check if the next non-empty line below is quoted
            let mut below = i + 1;
            while below < len && lines[below].trim().is_empty() {
                below += 1;
            }
            if below < len
                && (lines[below].trim_start().starts_with('>')
                    || is_reply_header(lines[below].trim_start()))
            {
                quoted_start = i;
                if i == 0 {
                    break;
                }
                i -= 1;
                continue;
            }
        }

        // Not a quoted line — stop scanning
        break;
    }

    if quoted_start >= len {
        // No quotes found
        return QuotedSplit {
            new_content: body.to_string(),
            quoted_content: String::new(),
            has_quotes: false,
        };
    }

    // Split the lines
    let new_lines: Vec<&str> = lines[..quoted_start].to_vec();
    let quoted_lines: Vec<&str> = lines[quoted_start..].to_vec();

    // Clean up: remove trailing empty lines from new content
    let new_content = new_lines.join("\n").trim_end().to_string();

    // Clean up: remove leading empty lines from quoted content
    let mut first_non_empty = 0;
    for (idx, line) in quoted_lines.iter().enumerate() {
        if !line.trim().is_empty() {
            first_non_empty = idx;
            break;
        }
    }
    let quoted_content = quoted_lines[first_non_empty..]
        .join("\n")
        .trim_end()
        .to_string();

    QuotedSplit {
        new_content,
        quoted_content,
        has_quotes: true,
    }
}

/// Strip leading `>` quote characters from plain text email lines.
///
/// Each line that starts with one or more `>` (with optional space after) has those
/// prefixes removed so the text renders cleanly without literal `>` characters.
/// Lines that don't start with `>` are kept as-is.
pub fn strip_quote_chars(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    for (idx, line) in body.lines().enumerate() {
        if idx > 0 {
            result.push('\n');
        }
        let trimmed = line.trim_start();
        // Repeatedly strip '>' and following whitespace to handle nested quotes like "> > text"
        let mut remaining = trimmed;
        loop {
            if let Some(after_gt) = remaining.strip_prefix('>') {
                let next = after_gt.trim_start();
                if next == remaining {
                    // No progress made (shouldn't happen, but safety check)
                    break;
                }
                remaining = next;
            } else {
                break;
            }
        }
        result.push_str(remaining);
    }
    result
}

/// Check if a line looks like a reply header (e.g. "On Monday, 23 November 2026 at 12:32 PM, Sender wrote:")
fn is_reply_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    // Common patterns: "On {date}, {sender} wrote:", "--- Forwarded message ---"
    (lower.starts_with("on ") && lower.ends_with("wrote:")) || lower.contains("forwarded message")
}

/// Check if an HTML email body contains quoted reply sections.
///
/// Detects:
/// - `<blockquote>` tags
/// - Gmail quote containers (`<div class="gmail_quote">`)
/// - two or more lines starting with an HTML-encoded `>` (plain-text style quoting)
///
/// Decorative styling such as `border-left` is deliberately NOT treated as a quote.
pub fn has_html_quotes(html: &str) -> bool {
    RE_BLOCKQUOTE_OPEN.is_match(html)
        || RE_GMAIL_QUOTE_OPEN.is_match(html)
        || RE_GT_LINE.find_iter(html).count() >= 2
}

/// Byte offset of the start tag of the first quoted element, if any.
fn find_html_quote_start(html: &str) -> Option<usize> {
    let blockquote = RE_BLOCKQUOTE_OPEN.find(html).map(|m| m.start());
    let gmail = RE_GMAIL_QUOTE_OPEN.find(html).map(|m| m.start());
    match (blockquote, gmail) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Empty spacer reserving room for the "..." button, which is a native widget drawn over it
/// (see `SecureHtmlViewer`). Its position in the document is the end of the new content.
const ANCHOR_HTML: &str =
    r#"<div id="photon-quote-anchor" style="height:28px;margin:10px 0"></div>"#;
/// Hides quoted elements while collapsed.
const HIDE_QUOTES_CSS: &str = "<style>blockquote,.gmail_quote{display:none!important}</style>";

/// Prepare the HTML of a reply for display with collapsible quotes.
///
/// Inserts the button's spacer directly before the first quoted element, i.e. at the end of
/// the new content inside the email itself, and hides the quoted elements unless
/// `expanded`. Returns `None` when no quoted element is found, in which case the caller
/// should render the email normally.
pub fn prepare_html_quotes(html: &str, expanded: bool) -> Option<String> {
    if !has_html_quotes(html) {
        return None;
    }
    let normalized = normalize_html_quotes(html);
    let start = find_html_quote_start(&normalized)?;

    let mut out = String::with_capacity(normalized.len() + 256);
    out.push_str(&normalized[..start]);
    if !expanded {
        out.push_str(HIDE_QUOTES_CSS);
    }
    out.push_str(ANCHOR_HTML);
    out.push_str(&normalized[start..]);
    Some(out)
}

/// Normalize an HTML email body by converting `&gt;` prefixed paragraphs to
/// proper `<blockquote>` tags, so CSS-based quote collapsing works uniformly.
///
/// Many email clients (including PhotonMail's own output) use `&gt;` entities
/// instead of semantic `<blockquote>` tags. This function converts them.
pub fn normalize_html_quotes(html: &str) -> String {
    // If already has <blockquote> tags, no normalization needed
    if RE_BLOCKQUOTE_OPEN.is_match(html) {
        return html.to_string();
    }

    // Check if there are &gt; patterns that look like quoted text
    let gt_count = html.matches("&gt;").count();
    if gt_count < 2 {
        return html.to_string();
    }

    // Convert &gt; prefixed content to <blockquote> tags.
    // Strategy: find paragraphs or other block elements that contain &gt; and wrap them.
    // Simple approach: wrap consecutive lines/paragraphs starting with &gt; in <blockquote>.
    let mut result = html.to_string();

    // Find all <p> tags that contain &gt; at the start
    // Replace <p>...&gt;...</p> with <blockquote><p>...</p></blockquote>
    let re_p_with_gt =
        regex::Regex::new(r"(?is)(<p[^>]*>)([^<]*&gt;[^<]*(?:<br[^>]*>\s*[^<]*&gt;[^<]*)*</p>)")
            .unwrap();
    result = re_p_with_gt.replace_all(&result, "<blockquote style=\"margin:0 0 0 0.8ex;border-left:2px solid #ccc;padding-left:1ex\">$1$2</blockquote>").to_string();

    // Also handle bare &gt; patterns not inside <p> tags
    // Wrap consecutive &gt; lines in blockquote
    if result == html {
        // Find sequences of &gt; that appear after common delimiters
        let re_bare_gt = regex::Regex::new(r"(?is)((?:<br\s*>|<br/>|\n)\s*&gt;)").unwrap();
        if re_bare_gt.is_match(&result) {
            // Simple approach: add a CSS class to the body that styles &gt; lines
            // This is handled by the CSS injection in sanitize_html
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_quote_chars_simple() {
        let body = "> Hi Alice,\n>\n> Saturday works for me.";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, "Hi Alice,\n\nSaturday works for me.");
    }

    #[test]
    fn test_strip_quote_chars_nested() {
        let body = "> > Are we still on?\n> >\n> > Best,\n> > Alice";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, "Are we still on?\n\nBest,\nAlice");
    }

    #[test]
    fn test_strip_quote_chars_mixed() {
        let body = "My reply\n\n> Original text\n> More original";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, "My reply\n\nOriginal text\nMore original");
    }

    #[test]
    fn test_strip_quote_chars_no_quotes() {
        let body = "Hello world\nNo quotes here";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, body);
    }

    #[test]
    fn test_strip_quote_chars_with_indentation() {
        let body = "  > Indented quote\n  > Another line";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, "Indented quote\nAnother line");
    }

    #[test]
    fn test_strip_quote_chars_empty_lines() {
        let body = ">\n> Text\n>";
        let stripped = strip_quote_chars(body);
        assert_eq!(stripped, "\nText\n");
    }

    #[test]
    fn test_split_plain_text_no_quotes() {
        let body = "Hello,\n\nThis is a test email.\n\nBest regards,\nJohn";
        let split = split_plain_text(body);
        assert!(!split.has_quotes);
        assert_eq!(split.new_content, body);
        assert!(split.quoted_content.is_empty());
    }

    #[test]
    fn test_split_plain_text_with_quotes() {
        let body = "Test reply\n\nOn Monday, 23 November 2026 at 12:32 PM, Sender wrote:\n> Original line 1\n> Original line 2";
        let split = split_plain_text(body);
        assert!(split.has_quotes);
        assert_eq!(split.new_content, "Test reply");
        assert!(split.quoted_content.contains("On Monday"));
        assert!(split.quoted_content.contains("> Original line 1"));
    }

    #[test]
    fn test_split_plain_text_nested_quotes() {
        let body = "My reply\n\n> > Nested quote\n> > Another nested line\n> Reply to the above";
        let split = split_plain_text(body);
        assert!(split.has_quotes);
        assert_eq!(split.new_content, "My reply");
        assert!(split.quoted_content.contains("> Nested quote"));
    }

    #[test]
    fn test_split_plain_text_forwarded() {
        let body =
            "Check this out:\n\n--- Forwarded message ---\n> Forwarded line 1\n> Forwarded line 2";
        let split = split_plain_text(body);
        assert!(split.has_quotes);
        assert_eq!(split.new_content, "Check this out:");
        assert!(split.quoted_content.contains("Forwarded message"));
    }

    #[test]
    fn test_split_plain_text_only_quotes() {
        let body = "> Quoted line 1\n> Quoted line 2";
        let split = split_plain_text(body);
        assert!(split.has_quotes);
        assert!(split.new_content.is_empty());
        assert!(split.quoted_content.contains("Quoted line 1"));
    }

    #[test]
    fn test_has_html_quotes_with_blockquotes() {
        let html = "<html><body><p>Hello</p><blockquote><p>Quote</p></blockquote></body></html>";
        assert!(has_html_quotes(html));
    }

    #[test]
    fn test_has_html_quotes_with_gt_entities() {
        let html = "<p>Reply text</p><p>On sender wrote:<br>&gt;Original line 1<br>&gt;Original line 2</p>";
        assert!(has_html_quotes(html));
    }

    #[test]
    fn test_has_html_quotes_no_quotes() {
        let html = "<html><body><p>Hello</p></body></html>";
        assert!(!has_html_quotes(html));
    }

    #[test]
    fn test_has_html_quotes_single_gt() {
        // A single &gt; is not a quote (e.g. "A > B" comparison)
        let html = "<p>A &gt; B</p>";
        assert!(!has_html_quotes(html));
    }

    #[test]
    fn test_normalize_html_quotes_converts_gt() {
        let html = "<p>Reply text</p><p>&gt;Original line 1</p><p>&gt;Original line 2</p>";
        let normalized = normalize_html_quotes(html);
        assert!(normalized.contains("<blockquote"));
        assert!(normalized.contains("&gt;"));
    }

    #[test]
    fn test_normalize_html_quotes_skips_blockquotes() {
        let html = "<p>Hello</p><blockquote><p>Quote</p></blockquote>";
        let normalized = normalize_html_quotes(html);
        assert_eq!(normalized, html); // No changes needed
    }

    #[test]
    fn test_is_reply() {
        assert!(is_reply("<abc@example.com>", "Hello"));
        assert!(is_reply("", "Re: Hello"));
        assert!(is_reply("", "  RE: Hello"));
        assert!(is_reply("", "Re[2]: Hello"));
        assert!(!is_reply("", "Hello"));
        assert!(!is_reply("  ", "Fwd: Hello"));
        assert!(!is_reply("", "Are we there yet?"));
    }

    #[test]
    fn test_has_html_quotes_ignores_border_left() {
        let html = "<div style=\"border-left:3px solid red\">Promo</div>";
        assert!(!has_html_quotes(html));
    }

    #[test]
    fn test_has_html_quotes_ignores_gmail_class_in_stylesheet() {
        let html = "<style>.gmail_quote{color:red}</style><p>Hello</p>";
        assert!(!has_html_quotes(html));
        assert!(prepare_html_quotes(html, false).is_none());
    }

    #[test]
    fn test_prepare_html_quotes_collapsed() {
        let html = "<html><body><p>My reply</p><p>On Monday, X wrote:</p><blockquote><p>Original</p></blockquote></body></html>";
        let out = prepare_html_quotes(html, false).unwrap();
        let toggle = out.find("photon-quote-anchor").unwrap();
        let quote = out.find("<blockquote").unwrap();
        let reply = out.find("My reply").unwrap();
        // Button sits after the new content and right before the quote
        assert!(reply < toggle && toggle < quote);
        assert!(out.contains(ANCHOR_ID));
        assert!(out.contains("display:none"));
    }

    #[test]
    fn test_prepare_html_quotes_expanded_keeps_quote_visible() {
        let html = "<p>My reply</p><blockquote>Original</blockquote>";
        let out = prepare_html_quotes(html, true).unwrap();
        assert!(out.contains("photon-quote-anchor"));
        assert!(!out.contains("display:none"));
    }

    #[test]
    fn test_prepare_html_quotes_gmail_container_wraps_attribution() {
        let html = "<div>Reply</div><div class=\"gmail_quote\"><div class=\"gmail_attr\">On Mon X wrote:</div><blockquote class=\"gmail_quote\">Orig</blockquote></div>";
        let out = prepare_html_quotes(html, false).unwrap();
        let toggle = out.find("photon-quote-anchor").unwrap();
        let container = out.find("<div class=\"gmail_quote\"").unwrap();
        assert!(toggle < container);
    }

    #[test]
    fn test_prepare_html_quotes_none_without_quotes() {
        assert!(prepare_html_quotes("<p>Hello</p>", false).is_none());
    }
}
