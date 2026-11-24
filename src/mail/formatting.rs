use lettre::message::Mailboxes;

pub(crate) fn clean_header_value(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| match character {
            '\r' | '\n' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn encode_unstructured_header(value: &str) -> String {
    let value = clean_header_value(value);
    let already_encoded = value
        .split_whitespace()
        .all(|word| word.starts_with("=?") && word.ends_with("?="));
    if value.is_ascii() && (value.len() <= 60 || already_encoded) {
        return value;
    }

    use base64::Engine;
    let mut words = Vec::new();
    let mut chunk = String::new();
    for character in value.chars() {
        if chunk.len() + character.len_utf8() > 36 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(character);
    }
    if !chunk.is_empty() {
        words.push(chunk);
    }
    words
        .iter()
        .map(|word| {
            format!(
                "=?UTF-8?B?{}?=",
                base64::engine::general_purpose::STANDARD.encode(word.as_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn format_mailbox_header(name: Option<&str>, address: &str) -> String {
    let address = clean_header_value(address);
    let Some(name) = name.map(clean_header_value).filter(|name| !name.is_empty()) else {
        return address;
    };
    let name = if name.is_ascii() && name.len() <= 60 {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        encode_unstructured_header(&name)
    };
    format!("{name} <{address}>")
}

pub(crate) fn format_address_list(value: &str) -> Result<String, String> {
    let value = clean_header_value(value);
    if value.is_empty() {
        return Ok(String::new());
    }
    let mailboxes: Mailboxes = value
        .parse()
        .map_err(|error| format!("Invalid address list '{value}': {error}"))?;
    Ok(mailboxes
        .into_iter()
        .map(|mailbox| format_mailbox_header(mailbox.name.as_deref(), &mailbox.email.to_string()))
        .collect::<Vec<_>>()
        .join(", "))
}

pub(crate) fn encode_mime_parameter(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(crate) fn append_header(raw: &mut String, name: &str, value: &str) {
    let value = clean_header_value(value);
    let mut line = format!("{name}: ");
    let mut whitespace = String::new();
    let mut word = String::new();

    for character in value.chars() {
        if character == ' ' || character == '\t' {
            if !word.is_empty() {
                append_folded_word(raw, &mut line, &word, &whitespace);
                word.clear();
                whitespace.clear();
            }
            whitespace.push(character);
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        append_folded_word(raw, &mut line, &word, &whitespace);
    }
    raw.push_str(&line);
    raw.push_str("\r\n");
}

fn append_folded_word(raw: &mut String, line: &mut String, word: &str, whitespace: &str) {
    if !whitespace.is_empty() && line.len() + whitespace.len() + word.len() > 78 {
        raw.push_str(line);
        raw.push_str("\r\n");
        line.clear();
        line.push_str(whitespace);
    } else {
        line.push_str(whitespace);
    }
    line.push_str(word);
}

pub(crate) fn quoted_printable_encode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut encoded = String::new();
    let mut line_len = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\r' || bytes[index] == b'\n' {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            encoded.push_str("\r\n");
            line_len = 0;
            index += 1;
            continue;
        }

        let byte = bytes[index];
        let token = if (33..=60).contains(&byte) || (62..=126).contains(&byte) {
            char::from(byte).to_string()
        } else {
            format!("={byte:02X}")
        };
        if line_len + token.len() > 75 {
            encoded.push_str("=\r\n");
            line_len = 0;
        }
        line_len += token.len();
        encoded.push_str(&token);
        index += 1;
    }
    encoded
}

pub(crate) fn generate_mime_boundary(kind: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "=_photon_{kind}_{nanos:x}_{:x}_{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn image_content_type(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "ico" => "image/vnd.microsoft.icon",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}

pub(crate) fn image_extension_for_content_type(content_type: &str) -> Option<&'static str> {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/svg+xml" => Some("svg"),
        "image/avif" => Some("avif"),
        "image/vnd.microsoft.icon" | "image/x-icon" => Some("ico"),
        "image/tiff" => Some("tiff"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_values_remove_injection_and_encode_unicode() {
        assert_eq!(
            clean_header_value("hello\r\nBcc: victim@example.com"),
            "hello  Bcc: victim@example.com"
        );
        let encoded = encode_unstructured_header("café");
        assert!(encoded.is_ascii());
        assert!(encoded.starts_with("=?UTF-8?B?"));
    }

    #[test]
    fn address_display_names_are_quoted_or_encoded() {
        let address = format_mailbox_header(Some("Smith, John \"J\""), "john@example.com");
        assert_eq!(address, "\"Smith, John \\\"J\\\"\" <john@example.com>");
        let unicode = format_mailbox_header(Some("Renée"), "renee@example.com");
        assert!(unicode.is_ascii());
        assert!(unicode.contains("<renee@example.com>"));
    }

    #[test]
    fn encoded_address_lists_remain_parseable() {
        let value =
            format_address_list("Renée <renee@example.com>, \"Smith, John\" <john@example.com>")
                .expect("valid address list");
        assert!(value.is_ascii());
        let parsed: Mailboxes = value.parse().expect("formatted address list parses");
        assert_eq!(parsed.into_iter().count(), 2);
    }

    #[test]
    fn long_encoded_words_are_not_encoded_twice() {
        let original = "A long café subject with enough words to exceed the short header limit";
        let encoded = encode_unstructured_header(original);
        assert!(encoded
            .split_whitespace()
            .all(|word| word.starts_with("=?") && word.ends_with("?=")));
        assert_eq!(encode_unstructured_header(&encoded), encoded);
    }

    #[test]
    fn headers_fold_without_exceeding_recommended_width() {
        let mut raw = String::new();
        append_header(&mut raw, "References", &("<id@example.com> ".repeat(12)));
        assert!(raw.split("\r\n").all(|line| line.len() <= 78));
    }

    #[test]
    fn quoted_printable_preserves_utf8_and_limits_line_length() {
        let input = format!("café {}\nend", "word ".repeat(30));
        let encoded = quoted_printable_encode(&input);
        assert!(encoded.is_ascii());
        assert!(encoded.lines().all(|line| line.len() <= 76));
        assert!(encoded.contains("=C3=A9"));
        assert!(encoded.contains("\r\n"));
        assert_eq!(
            decode_quoted_printable(&encoded),
            input.replace('\n', "\r\n").as_bytes()
        );
    }

    fn decode_quoted_printable(value: &str) -> Vec<u8> {
        let bytes = value.as_bytes();
        let mut decoded = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'=' {
                if bytes.get(index + 1..index + 3) == Some(b"\r\n") {
                    index += 3;
                } else {
                    let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
                    decoded.push(u8::from_str_radix(hex, 16).unwrap());
                    index += 3;
                }
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        decoded
    }

    #[test]
    fn mime_boundaries_are_distinct_and_extensions_are_mapped() {
        let first = generate_mime_boundary("alt");
        let second = generate_mime_boundary("related");
        assert_ne!(first, second);
        assert!(first.starts_with("=_photon_alt_"));
        assert_eq!(image_content_type("SVG"), "image/svg+xml");
        assert_eq!(image_content_type("unknown"), "application/octet-stream");
        assert_eq!(
            image_extension_for_content_type("image/svg+xml"),
            Some("svg")
        );
        assert_eq!(image_extension_for_content_type("image/unknown"), None);
        assert_eq!(
            encode_mime_parameter("café image.svg"),
            "caf%C3%A9%20image.svg"
        );
    }
}
