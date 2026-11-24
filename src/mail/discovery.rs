//! Async email server auto-discovery.
//!
//! Tries multiple methods in order, returning the first successful result:
//! 1. DNS SRV records (RFC 6186) — encrypted services first
//! 2. HTTPS autoconfig — Mozilla/Thunderbird XML format
//! 3. A provider table keyed by recognized MX hosts
//! 4. The built-in table keyed by email domain
//! 5. Heuristic fallback — guesses `imap.{domain}` / `smtp.{domain}`

use std::thread;
use std::time::Duration;

use hickory_resolver::{
    config::ResolverOpts,
    proto::rr::RecordType,
    TokioResolver,
};
use rand::Rng;

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
    let Some(domain) = super::config::email_domain(email) else {
        return DiscoveryResult::Failed("Invalid email address".to_string());
    };

    let domain = domain.to_ascii_lowercase();

    // 1. Prefer explicit protocol discovery and encrypted service records.
    if let Some(settings) = lookup_srv_records(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 2. Try provider-hosted autoconfig before inferring from inbound MX records.
    if let Some(settings) = lookup_autoconfig(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 3. MX hosts identify a provider but are never used as client-access servers.
    if let Some(settings) = lookup_mx_provider_settings(&domain).await {
        return DiscoveryResult::Found(settings);
    }

    // 4. Known email domains still get their official client-access endpoints.
    if let Some(settings) = super::config::discover_settings(email) {
        return DiscoveryResult::Found(DiscoveredSettings {
            imap_host: settings.imap.host.to_string(),
            imap_port: settings.imap.port,
            smtp_host: settings.smtp.host.to_string(),
            smtp_port: settings.smtp.port,
        });
    }

    if let Some(message) = super::config::unsupported_provider_message(&domain) {
        return DiscoveryResult::Failed(message.to_string());
    }

    // 5. Heuristic fallback for domains without recognized service records.
    DiscoveryResult::Found(heuristic_guess(&domain))
}

/// Build a DNS resolver from system config with a timeout.
fn build_resolver() -> Option<TokioResolver> {
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(5);
    TokioResolver::builder_tokio().ok()?.with_options(opts).build().into()
}

// ─── DNS SRV Records (RFC 6186) ───────────────────────────────────────────────

#[derive(Clone, Debug)]
struct SrvTarget {
    priority: u16,
    weight: u16,
    port: u16,
    host: String,
}

async fn lookup_srv_records(domain: &str) -> Option<DiscoveredSettings> {
    let resolver = build_resolver()?;
    let imap = match lookup_srv_service(&resolver, &format!("_imaps._tcp.{domain}"), 993).await {
        Some(settings) => settings,
        None => lookup_srv_service(&resolver, &format!("_imap._tcp.{domain}"), 143).await?,
    };
    let smtp = match lookup_srv_service(&resolver, &format!("_submissions._tcp.{domain}"), 465).await {
        Some(settings) => settings,
        None => lookup_srv_service(&resolver, &format!("_submission._tcp.{domain}"), 587).await?,
    };

    Some(DiscoveredSettings {
        imap_host: imap.host,
        imap_port: imap.port,
        smtp_host: smtp.host,
        smtp_port: smtp.port,
    })
}

async fn lookup_srv_service(resolver: &TokioResolver, query: &str, expected_port: u16) -> Option<SrvTarget> {
    let response = resolver.lookup(query, RecordType::SRV).await.ok()?;
    let targets: Vec<SrvTarget> = response
        .record_iter()
        .filter_map(|record| {
            let srv = record.data().as_srv()?;
            let host = srv.target().to_string();
            let host = host.trim_end_matches('.');
            if host.is_empty() || host == "." || srv.port() != expected_port {
                return None;
            }
            Some(SrvTarget {
                priority: srv.priority(),
                weight: srv.weight(),
                port: srv.port(),
                host: host.to_string(),
            })
        })
        .collect();
    choose_srv_target(&targets)
}

fn choose_srv_target(records: &[SrvTarget]) -> Option<SrvTarget> {
    let priority = records.iter().map(|record| record.priority).min()?;
    let eligible: Vec<&SrvTarget> = records.iter().filter(|record| record.priority == priority).collect();
    let total_weight: u64 = eligible.iter().map(|record| u64::from(record.weight)).sum();
    let choices = if total_weight > 0 { total_weight } else { eligible.len() as u64 };
    let draw = rand::rng().random_range(0..choices);
    select_srv_target(records, draw)
}

fn select_srv_target(records: &[SrvTarget], draw: u64) -> Option<SrvTarget> {
    let priority = records.iter().map(|record| record.priority).min()?;
    let eligible: Vec<&SrvTarget> = records.iter().filter(|record| record.priority == priority).collect();
    let total_weight: u64 = eligible.iter().map(|record| u64::from(record.weight)).sum();
    if total_weight == 0 {
        return eligible.get((draw % eligible.len() as u64) as usize).map(|record| (*record).clone());
    }

    let mut slot = draw % total_weight;
    for record in eligible {
        if slot < u64::from(record.weight) {
            return Some(record.clone());
        }
        slot -= u64::from(record.weight);
    }
    None
}

// ─── DNS MX Records ───────────────────────────────────────────────────────────

async fn lookup_mx_provider_settings(domain: &str) -> Option<DiscoveredSettings> {
    let resolver = build_resolver()?;
    let response = resolver.lookup(domain, RecordType::MX).await.ok()?;
    let mut mx_hosts = Vec::new();
    for record in response.record_iter() {
        if let Some(mx) = record.data().as_mx() {
            let host = mx.exchange().to_string();
            let host = host.trim_end_matches('.');
            if !host.is_empty() {
                mx_hosts.push((mx.preference(), host.to_string()));
            }
        }
    }
    settings_for_mx_hosts(mx_hosts)
}

fn settings_for_mx_hosts(mut mx_hosts: Vec<(u16, String)>) -> Option<DiscoveredSettings> {
    mx_hosts.sort_unstable_by_key(|(preference, _)| *preference);
    let best_preference = mx_hosts.first()?.0;
    mx_hosts
        .into_iter()
        .take_while(|(preference, _)| *preference == best_preference)
        .find_map(|(_, host)| super::config::discover_settings_for_mx_host(&host))
        .map(|settings| DiscoveredSettings {
            imap_host: settings.imap.host.to_string(),
            imap_port: settings.imap.port,
            smtp_host: settings.smtp.host.to_string(),
            smtp_port: settings.smtp.port,
        })
}

// ─── HTTPS Autoconfig (Mozilla/Thunderbird) ───────────────────────────────────

async fn lookup_autoconfig(domain: &str) -> Option<DiscoveredSettings> {
    let url = format!("https://autoconfig.{domain}/mail/config-v1.1.xml");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || attempt.url().scheme() != "https" {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .ok()?;

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let body = resp.text().await.ok()?;
    if body.len() > 1_048_576 {
        return None;
    }
    parse_autoconfig_xml(&body, domain)
}

fn parse_autoconfig_xml(body: &str, domain: &str) -> Option<DiscoveredSettings> {
    let document = roxmltree::Document::parse(body).ok()?;
    let provider = document.descendants().find(|node| {
        node.has_tag_name("emailProvider")
            && (node
                .attribute("id")
                .is_some_and(|id| id.eq_ignore_ascii_case(domain))
                || node.children().any(|child| {
                    child.has_tag_name("domain")
                        && child
                            .text()
                            .is_some_and(|value| value.trim().eq_ignore_ascii_case(domain))
                }))
    })?;

    let imap = provider
        .children()
        .filter(|node| node.has_tag_name("incomingServer"))
        .filter(|node| node.attribute("type").is_some_and(|kind| kind.eq_ignore_ascii_case("imap")))
        .find_map(|node| parse_autoconfig_server(node, false))?;
    let smtp = provider
        .children()
        .filter(|node| node.has_tag_name("outgoingServer"))
        .filter(|node| node.attribute("type").is_some_and(|kind| kind.eq_ignore_ascii_case("smtp")))
        .find_map(|node| parse_autoconfig_server(node, true))?;

    Some(DiscoveredSettings {
        imap_host: imap.0,
        imap_port: imap.1,
        smtp_host: smtp.0,
        smtp_port: smtp.1,
    })
}

fn parse_autoconfig_server(node: roxmltree::Node<'_, '_>, smtp: bool) -> Option<(String, u16)> {
    let child_text = |tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };

    let host = child_text("hostname")?;
    if !valid_hostname(host) {
        return None;
    }
    let socket_type = child_text("socketType")?.to_ascii_uppercase();
    let (port, expected_port) = match (smtp, socket_type.as_str()) {
        (false, "SSL") => (993, 993),
        (false, "STARTTLS") => (143, 143),
        (true, "SSL") => (465, 465),
        (true, "STARTTLS") => (587, 587),
        _ => return None,
    };
    let port = child_text("port").map_or(Some(port), |value| value.parse::<u16>().ok())?;
    if port != expected_port {
        return None;
    }
    Some((host.to_string(), port))
}

fn valid_hostname(host: &str) -> bool {
    if host.len() > 253 || host.is_empty() {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
    })
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

/// Runs discovery on a dedicated thread and returns immediately with a receiver.
/// Freya state stays on the UI executor; DNS and HTTPS work never block it.
pub fn discover_in_background(email: String) -> async_channel::Receiver<DiscoveryResult> {
    let (tx, rx) = async_channel::bounded(1);
    let worker_tx = tx.clone();
    let spawn_result = thread::Builder::new()
        .name("mail-server-discovery".to_string())
        .spawn(move || {
            let result = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime.block_on(async {
                    match tokio::time::timeout(Duration::from_secs(40), discover_async(&email)).await {
                        Ok(result) => result,
                        Err(_) => DiscoveryResult::Failed("Server discovery timed out".to_string()),
                    }
                }),
                Err(error) => DiscoveryResult::Failed(format!("Could not start discovery: {error}")),
            };
            let _ = worker_tx.send_blocking(result);
        });
    if let Err(error) = spawn_result {
        let _ = tx.send_blocking(DiscoveryResult::Failed(format!("Could not start discovery: {error}")));
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_discovery_runs_with_its_own_runtime() {
        let result = discover_in_background("invalid-address".to_string())
            .recv_blocking()
            .expect("discovery worker should return a result");

        assert!(matches!(result, DiscoveryResult::Failed(message) if message == "Invalid email address"));
    }

    #[test]
    fn srv_selection_obeys_priority_and_weight() {
        let records = [
            SrvTarget { priority: 10, weight: 1, port: 993, host: "first.test".to_string() },
            SrvTarget { priority: 10, weight: 3, port: 993, host: "weighted.test".to_string() },
            SrvTarget { priority: 20, weight: 100, port: 993, host: "fallback.test".to_string() },
        ];

        assert_eq!(select_srv_target(&records, 0).unwrap().host, "first.test");
        assert_eq!(select_srv_target(&records, 1).unwrap().host, "weighted.test");
        assert_eq!(select_srv_target(&records, 3).unwrap().host, "weighted.test");
    }

    #[test]
    fn mx_provider_detection_only_considers_the_best_preference() {
        let fallback = settings_for_mx_hosts(vec![
            (10, "unrecognized.example.test".to_string()),
            (20, "mx.transip.email".to_string()),
        ]);
        assert!(fallback.is_none());

        let equal_priority = settings_for_mx_hosts(vec![
            (10, "unrecognized.example.test".to_string()),
            (10, "mx.transip.email".to_string()),
        ])
        .unwrap();
        assert_eq!(equal_priority.imap_host, "imap.transip.email");
    }

    #[test]
    fn autoconfig_selects_imap_instead_of_pop_and_defaults_by_socket_type() {
        let xml = r#"
            <clientConfig version="1.1">
                <emailProvider id="example.test">
                    <incomingServer type="pop3">
                        <hostname>pop.example.test</hostname><port>995</port>
                        <socketType>SSL</socketType>
                    </incomingServer>
                    <incomingServer type="imap">
                        <hostname>imap.example.test</hostname><socketType>SSL</socketType>
                    </incomingServer>
                    <outgoingServer type="smtp">
                        <hostname>smtp.example.test</hostname><socketType>STARTTLS</socketType>
                    </outgoingServer>
                </emailProvider>
            </clientConfig>
        "#;

        let settings = parse_autoconfig_xml(xml, "example.test").unwrap();
        assert_eq!(settings.imap_host, "imap.example.test");
        assert_eq!(settings.imap_port, 993);
        assert_eq!(settings.smtp_host, "smtp.example.test");
        assert_eq!(settings.smtp_port, 587);
    }

    #[test]
    fn autoconfig_rejects_plaintext_and_unsupported_server_types() {
        let pop_only = r#"
            <clientConfig><emailProvider id="example.test">
                <incomingServer type="pop3"><hostname>pop.example.test</hostname><port>995</port>
                    <socketType>SSL</socketType></incomingServer>
                <outgoingServer type="smtp"><hostname>smtp.example.test</hostname><port>465</port>
                    <socketType>SSL</socketType></outgoingServer>
            </emailProvider></clientConfig>
        "#;
        assert!(parse_autoconfig_xml(pop_only, "example.test").is_none());

        let plaintext = r#"
            <clientConfig><emailProvider id="example.test">
                <incomingServer type="imap"><hostname>imap.example.test</hostname><port>143</port>
                    <socketType>plain</socketType></incomingServer>
                <outgoingServer type="smtp"><hostname>smtp.example.test</hostname><port>587</port>
                    <socketType>STARTTLS</socketType></outgoingServer>
            </emailProvider></clientConfig>
        "#;
        assert!(parse_autoconfig_xml(plaintext, "example.test").is_none());
    }
}
