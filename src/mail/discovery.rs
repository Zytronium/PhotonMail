//! Async email server auto-discovery.
//!
//! Tries multiple methods in order, returning the first successful result:
//! 1. Built-in provider database (offline, instant)
//! 2. DNS SRV records (RFC 6186) — `_imaps._tcp` and `_submission._tcp`
//! 3. DNS MX records — finds the mail server hostname
//! 4. HTTP autoconfig — Mozilla/Thunderbird XML format
//! 5. Heuristic fallback — guesses `imap.{domain}` / `smtp.{domain}`

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hickory_resolver::{
    config::ResolverOpts,
    proto::rr::RecordType,
    TokioResolver,
};

/// Discovered IMAP and SMTP server settings.
#[derive(Clone, Debug)]
pub struct DiscoveredSettings {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
}

/// Result of an async discovery attempt.
#[derive(Clone, Debug)]
pub enum DiscoveryResult {
    /// Settings were found.
    Found(DiscoveredSettings),
    /// Discovery failed with a human-readable message.
    Failed(String),
}

/// Run the full async discovery pipeline for an email address.
pub async fn discover_async(email: &str) -> DiscoveryResult {
    let Some(domain) = email.split('@').nth(1) else {
        return DiscoveryResult::Failed("Invalid email address".to_string());
    };

    let domain = domain.to_lowercase();

    // 1. Try DNS SRV records (RFC 6186)
    if let Some(settings) = lookup_srv_records(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 2. Try DNS MX records
    if let Some(settings) = lookup_mx_records(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 3. Try HTTP autoconfig (Mozilla/Thunderbird format)
    if let Some(settings) = lookup_autoconfig(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 4. Heuristic fallback
    DiscoveryResult::Found(heuristic_guess(&domain))
}

/// Build a DNS resolver from system config with a timeout.
fn build_resolver() -> Option<TokioResolver> {
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(5);
    TokioResolver::builder_tokio().ok()?.with_options(opts).build().into()
}

// ─── DNS SRV Records (RFC 6186) ───────────────────────────────────────────────

async fn lookup_srv_records(domain: &str) -> Option<DiscoveredSettings> {
    let resolver = build_resolver()?;

    let mut imap_host = String::new();
    let mut imap_port = 993u16;
    let mut smtp_host = String::new();
    let mut smtp_port = 465u16;

    // IMAP SRV
    let imap_query = format!("_imaps._tcp.{domain}");
    if let Ok(response) = resolver.lookup(&imap_query, RecordType::SRV).await {
        for record in response.record_iter() {
            if let Some(srv) = record.data().as_srv() {
                imap_host = srv.target().to_string();
                if imap_host.ends_with('.') {
                    imap_host.pop();
                }
                imap_port = srv.port();
                break;
            }
        }
    }

    // SMTP SRV
    let smtp_query = format!("_submission._tcp.{domain}");
    if let Ok(response) = resolver.lookup(&smtp_query, RecordType::SRV).await {
        for record in response.record_iter() {
            if let Some(srv) = record.data().as_srv() {
                smtp_host = srv.target().to_string();
                if smtp_host.ends_with('.') {
                    smtp_host.pop();
                }
                smtp_port = srv.port();
                break;
            }
        }
    }

    if imap_host.is_empty() && smtp_host.is_empty() {
        return None;
    }

    if imap_host.is_empty() {
        imap_host = smtp_host.clone();
        imap_port = 993;
    }
    if smtp_host.is_empty() {
        smtp_host = imap_host.clone();
        smtp_port = 465;
    }

    Some(DiscoveredSettings {
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
    })
}

// ─── DNS MX Records ───────────────────────────────────────────────────────────

async fn lookup_mx_records(domain: &str) -> Option<DiscoveredSettings> {
    let resolver = build_resolver()?;
    let response = resolver.lookup(domain, RecordType::MX).await.ok()?;

    for record in response.record_iter() {
        if let Some(mx) = record.data().as_mx() {
            let exchange = mx.exchange().to_string();
            let exchange = if exchange.ends_with('.') {
                exchange[..exchange.len() - 1].to_string()
            } else {
                exchange
            };
            if exchange.is_empty() {
                continue;
            }
            return Some(DiscoveredSettings {
                imap_host: exchange.clone(),
                imap_port: 993,
                smtp_host: exchange,
                smtp_port: 465,
            });
        }
    }

    None
}

// ─── HTTP Autoconfig (Mozilla/Thunderbird) ────────────────────────────────────

async fn lookup_autoconfig(domain: &str) -> Option<DiscoveredSettings> {
    let url = format!("https://autoconfig.{domain}/mail/config-v1.1.xml");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let body = resp.text().await.ok()?;
    parse_autoconfig_xml(&body, domain)
}

fn parse_autoconfig_xml(body: &str, domain: &str) -> Option<DiscoveredSettings> {
    let marker = format!("id=\"{}\"", domain);
    let start = body.find(&marker)?;
    let block_end = body[start..]
        .find("<emailProvider")
        .map(|p| start + p)
        .unwrap_or(body.len());
    let block = &body[start..block_end];

    let (mut imap_host, mut imap_port) =
        extract_server(block, "incomingServer").unwrap_or((String::new(), 993));
    let (mut smtp_host, mut smtp_port) =
        extract_server(block, "outgoingServer").unwrap_or((String::new(), 465));

    if imap_host.is_empty() && smtp_host.is_empty() {
        return None;
    }
    if imap_host.is_empty() {
        imap_host = smtp_host.clone();
        imap_port = 993;
    }
    if smtp_host.is_empty() {
        smtp_host = imap_host.clone();
        smtp_port = 465;
    }

    Some(DiscoveredSettings {
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
    })
}

fn extract_server(block: &str, tag: &str) -> Option<(String, u16)> {
    let open = format!("<{tag}");
    let start = block.find(&open)?;
    let close = format!("</{tag}>");
    let end = block[start..].find(&close)? + start;
    let server_block = &block[start..end];

    let hostname = extract_tag_value(server_block, "hostname")?;
    let port = extract_tag_port(server_block).unwrap_or(993);
    Some((hostname, port))
}

fn extract_tag_value(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let value = block[start..end].trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn extract_tag_port(block: &str) -> Option<u16> {
    let open = "<port>";
    let close = "</port>";
    let start = block.find(open)? + open.len();
    let end = block[start..].find(close)? + start;
    block[start..end].trim().parse().ok()
}

// ─── Heuristic Fallback ───────────────────────────────────────────────────────

fn heuristic_guess(domain: &str) -> DiscoveredSettings {
    DiscoveredSettings {
        imap_host: format!("imap.{domain}"),
        imap_port: 993,
        smtp_host: format!("smtp.{domain}"),
        smtp_port: 465,
    }
}

/// Synchronous wrapper that runs `discover_async` on a background thread with its own
/// Tokio runtime. This is needed because `hickory-resolver` uses `tokio::spawn` internally,
/// which requires a Tokio reactor — but Freya's UI event loop doesn't provide one.
///
/// The caller (UI thread) blocks waiting for the result via a channel, while the DNS
/// work runs on a dedicated thread. The thread is limited by the timeouts in the resolver
/// config (~5s per DNS query) so it won't hang indefinitely.
pub fn discover_sync(email: &str) -> DiscoveryResult {
    let (tx, rx) = mpsc::channel();
    let email = email.to_string();

    thread::spawn(move || {
        // Create a new Tokio runtime on this thread
        let rt = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
        let result = rt.block_on(discover_async(&email));
        // Send result back to the UI thread
        let _ = tx.send(result);
    });

    // Wait for the result with a timeout to avoid hanging forever
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(result) => result,
        Err(_) => DiscoveryResult::Failed("Discovery timed out".to_string()),
    }
}
