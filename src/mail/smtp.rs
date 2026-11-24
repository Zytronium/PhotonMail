//! SMTP sending via lettre. Runs on the Tokio runtime alongside the IMAP worker.
//!
//! Uses implicit TLS (port 465) or STARTTLS (port 587) depending on the port.
//! Reuses the same rustls + ring TLS configuration as the IMAP connection so there
//! is no system OpenSSL dependency.

use std::time::Duration;

use lettre::{
    address::{Address, Envelope},
    message::Mailboxes,
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};

use super::config::AccountConfig;

const SEND_TIMEOUT: Duration = Duration::from_secs(30);

// -------- server resolution --------

/// Use the configured SMTP server, or derive one from the IMAP host
/// (imap.example.com -> smtp.example.com) when the account has none saved,
/// which happens for providers outside the auto-discovery list.
///
/// For self-hosted servers where the IMAP host already looks like an SMTP host
/// (e.g. `mail.example.com` or `smtp.example.com`), falls back to using the
/// IMAP host directly rather than prepending `smtp.`.
pub(super) fn resolve_smtp(cfg: &AccountConfig) -> Option<(String, u16)> {
    let port = if cfg.smtp_port == 0 { 587 } else { cfg.smtp_port };
    if !cfg.smtp_host.trim().is_empty() {
        return Some((cfg.smtp_host.trim().to_string(), port));
    }
    let imap = cfg.host.trim().to_lowercase();
    if imap.is_empty() {
        return None;
    }
    // Known IMAP prefixes that should be swapped for `smtp.`
    for prefix in ["imap-mail.", "imap.", "imaps."] {
        if let Some(rest) = imap.strip_prefix(prefix) {
            return Some((format!("smtp.{rest}"), port));
        }
    }
    // If the host already starts with `smtp.` or `mail.`, use it as-is
    // (self-hosted servers often use the same host for IMAP and SMTP).
    if imap.starts_with("smtp.") || imap.starts_with("mail.") {
        return Some((imap, port));
    }
    // Last resort: prepend `smtp.`
    Some((format!("smtp.{imap}"), port))
}

// -------- header handling --------

/// Split a raw message into (unfolded headers, body). Only the header block is
/// ever inspected, so body lines like "To: ..." can never be mistaken for headers.
/// All line endings are normalised to CRLF (many servers reject bare LF).
fn split_message(raw: &str) -> (Vec<(String, String)>, String) {
    let norm = raw.replace("\r\n", "\n").replace('\r', "\n");
    let (head, body) = match norm.find("\n\n") {
        Some(i) => (&norm[..i], &norm[i + 2..]),
        None => (norm.as_str(), ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.split('\n') {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    (headers, body.replace('\n', "\r\n"))
}

/// RFC 2047 encode a header value when it contains non-ASCII characters.
fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    use base64::Engine;
    // Chunk on char boundaries so each encoded-word stays under 75 chars.
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in value.chars() {
        if chunk.len() + c.len_utf8() > 36 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(chunk);
    }
    words
        .iter()
        .map(|w| {
            format!(
                "=?UTF-8?B?{}?=",
                base64::engine::general_purpose::STANDARD.encode(w.as_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join("\r\n ")
}

fn header_addrs(headers: &[(String, String)], name: &str) -> Result<Vec<Address>, String> {
    let mut out = Vec::new();
    for (k, v) in headers {
        if k.eq_ignore_ascii_case(name) && !v.trim().is_empty() {
            let boxes: Mailboxes = v
                .parse()
                .map_err(|e| format!("Invalid {name} address '{v}': {e}"))?;
            out.extend(boxes.into_iter().map(|m| m.email));
        }
    }
    Ok(out)
}

/// Send a raw RFC-2822 message via SMTP using `send_raw` so the compose pane's
/// multipart structure (HTML, plain text, inline images) is preserved.
///
/// - Recipients come from To/Cc/Bcc (header block only); Bcc is stripped before sending.
/// - Port 465 uses implicit TLS, every other port uses STARTTLS.
pub async fn send_email(
    cfg: &AccountConfig,
    password: &str,
    raw: &str,
) -> Result<(), String> {
    let (smtp_host, smtp_port) = resolve_smtp(cfg).ok_or_else(|| {
        "No SMTP server known for this account. Set \"smtp_host\" and \"smtp_port\" for it in PhotonMail/accounts.json"
            .to_string()
    })?;

    let creds = Credentials::new(cfg.username.clone(), password.to_string());

    let mailer = if smtp_port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host.as_str())
            .map_err(|e| format!("Invalid SMTP host '{}': {e}", smtp_host))?
            .port(465)
            .credentials(creds)
            .timeout(Some(SEND_TIMEOUT))
            .build()
    } else {
        let mut b = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host.as_str())
            .map_err(|e| format!("Invalid SMTP host '{}': {e}", smtp_host))?;
        if smtp_port != 0 {
            b = b.port(smtp_port);
        }
        b.credentials(creds).timeout(Some(SEND_TIMEOUT)).build()
    };

    let (headers, body) = split_message(raw);

    // Envelope sender: From header, falling back to the account address.
    let from_addr = match header_addrs(&headers, "From")?.into_iter().next() {
        Some(a) => a,
        None => cfg
            .email
            .parse::<Address>()
            .map_err(|e| format!("Invalid From address '{}': {e}", cfg.email))?,
    };

    let mut rcpts = header_addrs(&headers, "To")?;
    rcpts.extend(header_addrs(&headers, "Cc")?);
    rcpts.extend(header_addrs(&headers, "Bcc")?);
    rcpts.dedup();
    if rcpts.is_empty() {
        return Err("No recipients found in the message".to_string());
    }

    let envelope = Envelope::new(Some(from_addr.clone()), rcpts)
        .map_err(|e| format!("Invalid envelope: {e}"))?;

    // Rebuild the header block: drop Bcc, encode Subject, ensure From and Message-ID.
    let mut out = String::new();
    let mut has_from = false;
    let mut has_msgid = false;
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("bcc") {
            continue;
        }
        if k.eq_ignore_ascii_case("from") {
            has_from = true;
        }
        if k.eq_ignore_ascii_case("message-id") {
            has_msgid = true;
        }
        let v = if k.eq_ignore_ascii_case("subject") { encode_header(v) } else { v.clone() };
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_from {
        out.push_str(&format!("From: {from_addr}\r\n"));
    }
    if !has_msgid {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let domain = from_addr.domain();
        out.push_str(&format!("Message-ID: <{ts:x}.{}@{domain}>\r\n", std::process::id()));
    }
    out.push_str("\r\n");
    out.push_str(&body);

    // lettre handles dot-stuffing
    mailer
        .send_raw(&envelope, out.as_bytes())
        .await
        .map(|_| ())
        .map_err(|e| format!("SMTP error: {e}"))
}
