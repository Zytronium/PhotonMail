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
            attachments: Vec::new(),
        };
    };

    // Extract attachments with their binary data
    let attachments: Vec<AttachmentInfo> = msg
        .attachments()
        .map(|part| {
            // Extract the decoded binary data from the attachment part
            let data = match &part.body {
                PartType::Binary(data) | PartType::InlineBinary(data) => {
                    Some(data.to_vec())
                }
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
            }
        })
        .collect();

    // For HTML-only mail, `body_text` renders the HTML down to plain text, which is
    // what we want for the list preview.
    let preview = msg.body_text(0).map(|t| make_preview(&t)).unwrap_or_default();

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
            attachments,
        };
    }

    let text = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();
    BodyData {
        content_type: MailContentType::PlainText,
        preview,
        text: Arc::from(text),
        attachments,
    }
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
