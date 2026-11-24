//! Turning raw IMAP data into display strings. Pure functions, no I/O.

use std::sync::Arc;

use mail_parser::{MessageParser, MimeHeaders, PartType};

use super::{AttachmentInfo, BodyData};
use crate::data::MailContentType;

/// ENVELOPE strings arrive raw, so a subject or display name can still be an RFC 2047
/// encoded word (`=?UTF-8?B?…?=`). Let mail-parser decode it by wrapping it in a header.
pub fn decode_text(raw: &[u8]) -> String {
    let plain = String::from_utf8_lossy(raw);
    if !plain.contains("=?") {
        return plain.trim().to_string();
    }
    let mut buf = Vec::with_capacity(raw.len() + 16);
    buf.extend_from_slice(b"Subject: ");
    buf.extend_from_slice(raw);
    buf.extend_from_slice(b"\r\n\r\n");
    MessageParser::default()
        .parse_headers(&buf)
        .and_then(|m| m.subject().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| plain.trim().to_string())
}

/// Parse a full RFC 822 message into what the reader needs.
pub fn parse_body(raw: &[u8]) -> BodyData {
    let Some(msg) = MessageParser::default().parse(raw) else {
        let text = String::from_utf8_lossy(raw).into_owned();
        return BodyData {
            content_type: MailContentType::PlainText,
            preview: make_preview(&text),
            text: Arc::from(text),
            source: Arc::from(raw),
            attachments: Vec::new(),
            references: Vec::new(),
        };
    };
    let references = extract_references(raw);

    // Extract attachments with their binary data
    let attachments: Vec<AttachmentInfo> = msg
        .attachments()
        .map(|part| {
            // Extract the decoded binary data from the attachment part
            let data = match &part.body {
                PartType::Binary(data) | PartType::InlineBinary(data) => Some(data.to_vec()),
                PartType::Text(text) => {
                    // Some attachments may be text-based (e.g., CSV, plain text files)
                    Some(text.as_bytes().to_vec())
                }
                _ => None,
            };

            AttachmentInfo {
                name: part
                    .attachment_name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "attachment".to_string()),
                size: part.len(),
                content_type: part
                    .content_type()
                    .map(|ct| {
                        if let Some(sub) = &ct.c_subtype {
                            format!("{}/{}", ct.c_type, sub)
                        } else {
                            ct.c_type.to_string()
                        }
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                data,
                content_id: part.content_id().map(|s| {
                    s.trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string()
                }),
            }
        })
        .collect();

    // For HTML-only mail, `body_text` renders the HTML down to plain text, which is
    // what we want for the list preview.
    let preview = msg
        .body_text(0)
        .map(|t| make_preview(&t))
        .unwrap_or_default();

    // `body_html` also fabricates HTML from plain text, so look at the real part type
    // to decide whether this is genuinely an HTML message.
    let html_part = msg
        .html_body
        .first()
        .and_then(|&id| msg.parts.get(id as usize));
    if let Some(PartType::Html(html)) = html_part.map(|p| &p.body) {
        return BodyData {
            content_type: MailContentType::Html,
            preview,
            text: Arc::from(html.as_ref()),
            source: Arc::from(raw),
            attachments,
            references,
        };
    }

    let text = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();
    BodyData {
        content_type: MailContentType::PlainText,
        preview,
        text: Arc::from(text),
        source: Arc::from(raw),
        attachments,
        references,
    }
}

/// Return a header value from a raw RFC 822 message, unfolding continuation lines.
pub fn header_value(raw: &[u8], wanted: &str) -> Option<String> {
    let crlf = raw.windows(4).position(|w| w == b"\r\n\r\n");
    let lf = raw.windows(2).position(|w| w == b"\n\n");
    let end = match (crlf, lf) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => raw.len(),
    };
    let headers = String::from_utf8_lossy(&raw[..end]);
    let mut value: Option<String> = None;
    let mut matching = false;

    for line in headers.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            if matching {
                if let Some(value) = &mut value {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            }
            continue;
        }
        if matching {
            break;
        }
        if let Some((name, field_value)) = line.split_once(':') {
            matching = name.trim().eq_ignore_ascii_case(wanted);
            if matching {
                value = Some(field_value.trim().to_string());
            }
        }
    }

    value.filter(|value| !value.is_empty())
}

/// All `<id>` tokens in a Message-ID / In-Reply-To / References value, without the angle
/// brackets. A bracket-less single token is accepted as-is (some broken mailers omit them).
pub fn message_ids(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else { break };
        let id = after[..end].trim();
        if !id.is_empty() {
            out.push(id.to_string());
        }
        rest = &after[end + 1..];
    }
    if out.is_empty() {
        let t = value.trim();
        if !t.is_empty() && !t.contains(char::is_whitespace) {
            out.push(t.to_string());
        }
    }
    out
}

/// The ids in the message's `References` header (RFC 5322 §3.6.4), oldest first.
/// Reads only the header block and unfolds continuation lines.
fn extract_references(raw: &[u8]) -> Vec<String> {
    let crlf = raw.windows(4).position(|w| w == b"\r\n\r\n");
    let lf = raw.windows(2).position(|w| w == b"\n\n");
    let end = match (crlf, lf) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => raw.len(),
    };
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut value = String::new();
    let mut in_refs = false;
    for line in head.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if in_refs {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        in_refs = false;
        if let Some((name, rest)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("references") {
                in_refs = true;
                value.push(' ');
                value.push_str(rest.trim());
            }
        }
    }
    message_ids(&value)
}

fn make_preview(text: &str) -> String {
    const MAX: usize = 140;
    let mut out = String::with_capacity(MAX + 4);
    let mut count = 0;
    let mut last_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                count += 1;
                last_space = true;
            }
        } else {
            out.push(c);
            count += 1;
            last_space = false;
        }
        if count >= MAX {
            break;
        }
    }
    out.trim().to_string()
}

/// Strip HTML tags from a string, returning plain text.
/// Handles common tags: converts `<br>` to newlines, `<p>` to double newlines,
/// and decodes common HTML entities.
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script_or_style = false;
    let mut i = 0usize;
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();

    while i < len {
        let ch = chars[i];

        if in_script_or_style {
            if in_tag {
                if ch == '>' {
                    in_tag = false;
                    in_script_or_style = false;
                }
            }
            i += 1;
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }

        if ch == '<' {
            // Look ahead to determine tag type
            let rest: String = chars[i..].iter().collect();
            if let Some(tag_end) = rest.find('>') {
                let tag_content: String = rest[1..tag_end].chars().collect();
                let tag_lower = tag_content.trim().to_lowercase();

                if tag_lower == "br" || tag_lower == "br/" || tag_lower.starts_with("br ") {
                    result.push('\n');
                } else if tag_lower == "p" || tag_lower == "/p" {
                    // Only add paragraph break if last char isn't already a newline
                    if !result.ends_with('\n') {
                        result.push_str("\n\n");
                    } else if result.ends_with('\n') && !result.ends_with("\n\n") {
                        result.push('\n');
                    }
                } else if tag_lower.starts_with("script") || tag_lower.starts_with("style") {
                    in_script_or_style = true;
                    in_tag = true;
                    i += 1;
                    continue;
                } else if tag_lower.starts_with("<!--") {
                    // HTML comment — skip until -->
                    if let Some(comment_end) = rest.find("-->") {
                        i += comment_end + 3;
                        continue;
                    }
                }
                in_tag = true;
            } else {
                // No closing >, skip rest
                break;
            }
            i += 1;
            continue;
        }

        // Decode common HTML entities
        if ch == '&' {
            let rest: String = chars[i..].iter().collect();
            if let Some(amp_end) = rest.find(';') {
                let entity = &rest[1..amp_end];
                match entity {
                    "amp" => result.push('&'),
                    "lt" => result.push('<'),
                    "gt" => result.push('>'),
                    "quot" => result.push('"'),
                    "apos" => result.push('\''),
                    "nbsp" => result.push(' '),
                    "copy" => result.push('©'),
                    "reg" => result.push('®'),
                    "mdash" => result.push('—'),
                    "ndash" => result.push('–'),
                    "laquo" => result.push('«'),
                    "raquo" => result.push('»'),
                    "bull" => result.push('•'),
                    _ => {
                        // Not a known entity, keep as-is
                        result.push(ch);
                        i += 1;
                        continue;
                    }
                }
                i += amp_end + 1;
                continue;
            } else {
                result.push(ch);
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    // Collapse 3+ consecutive newlines to 2
    let mut out = String::with_capacity(result.len());
    let mut consecutive_newlines = 0;
    for ch in result.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                out.push(ch);
            }
        } else {
            consecutive_newlines = 0;
            out.push(ch);
        }
    }

    out.trim().to_string()
}

/// List-style date: time today, "Yesterday", "Nov 12", or "Nov 12, 2025".
pub fn fmt_date(ts: i64) -> String {
    use chrono::{DateTime, Datelike, Local, Utc};
    let Some(utc) = DateTime::<Utc>::from_timestamp(ts, 0) else {
        return String::new();
    };
    let dt = utc.with_timezone(&Local);
    let now = Local::now();
    let days = (now.date_naive() - dt.date_naive()).num_days();
    if days == 0 {
        dt.format("%H:%M").to_string()
    } else if days == 1 {
        "Yesterday".to_string()
    } else if dt.year() == now.year() {
        dt.format("%b %-d").to_string()
    } else {
        dt.format("%b %-d, %Y").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::header_value;

    #[test]
    fn header_value_is_case_insensitive_and_unfolds_continuations() {
        let raw =
            b"Subject: hello\r\nDKIM-Signature: v=1;\r\n d=example.test;\r\n s=mail\r\n\r\nbody";

        assert_eq!(
            header_value(raw, "dkim-signature").as_deref(),
            Some("v=1; d=example.test; s=mail")
        );
    }

    #[test]
    fn header_value_does_not_read_into_the_message_body() {
        let raw = b"Subject: hello\n\nX-Test: body-only";

        assert_eq!(header_value(raw, "X-Test"), None);
    }
}
