use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, OnceLock},
    time::Duration,
};

use blitz_traits::net::Bytes;
use reqwest::{
    Client,
    redirect::Policy,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::{Host, Url};

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_OUTSTANDING_FETCHES: usize = 4;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) type FetchPermit = OwnedSemaphorePermit;

static FETCH_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(crate) fn try_acquire_fetch() -> Option<FetchPermit> {
    FETCH_LIMITER
        .get_or_init(|| Arc::new(Semaphore::new(MAX_OUTSTANDING_FETCHES)))
        .clone()
        .try_acquire_owned()
        .ok()
}

pub(crate) async fn fetch(url: Url, permit: FetchPermit) -> Result<Bytes, String> {
    let (sender, receiver) = futures_channel::oneshot::channel();
    std::thread::Builder::new()
        .name("remote-content-fetch".to_string())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())
                .and_then(|runtime| {
                    runtime.block_on(async move {
                        tokio::time::timeout(FETCH_TIMEOUT, fetch_with_redirects(url))
                            .await
                            .map_err(|_| "Remote resource request timed out".to_string())?
                    })
                });
            drop(permit);
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;

    receiver
        .await
        .map_err(|_| "Remote resource worker stopped unexpectedly".to_string())?
}

async fn fetch_with_redirects(mut url: Url) -> Result<Bytes, String> {
    for redirect_count in 0..=MAX_REDIRECTS {
        validate_url(&url)?;
        let addresses = resolve_public_addresses(&url).await?;
        let client = build_client(&url, &addresses)?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| error.to_string())?;

        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err("Remote resource exceeded the redirect limit".to_string());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .ok_or_else(|| "Remote resource redirect had no valid location".to_string())?;
            url = redirect_target(&url, location)?;
            continue;
        }

        if !response.status().is_success() {
            return Err(format!("Remote resource returned {}", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES as u64)
        {
            return Err("Remote resource exceeded the body size limit".to_string());
        }

        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default();
        let mut body = Vec::with_capacity(capacity);
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| error.to_string())?
        {
            if !append_bounded_chunk(&mut body, &chunk, MAX_BODY_BYTES) {
                return Err("Remote resource exceeded the body size limit".to_string());
            }
        }
        return Ok(Bytes::from(body));
    }

    Err("Remote resource exceeded the redirect limit".to_string())
}

fn validate_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Remote resource URL is not allowed".to_string());
    }
    Ok(())
}

async fn resolve_public_addresses(url: &Url) -> Result<Vec<SocketAddr>, String> {
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Remote resource URL has no valid port".to_string())?;
    let addresses = match url.host().ok_or_else(|| "Remote resource URL has no host".to_string())? {
        Host::Domain(domain) => {
            if is_blocked_hostname(domain) {
                return Err("Remote resource destination is local".to_string());
            }
            tokio::net::lookup_host((domain, port))
                .await
                .map_err(|error| error.to_string())?
                .collect::<Vec<_>>()
        }
        Host::Ipv4(address) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Host::Ipv6(address) => vec![SocketAddr::new(IpAddr::V6(address), port)],
    };

    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("Remote resource destination is not public".to_string());
    }
    Ok(addresses)
}

fn build_client(url: &Url, addresses: &[SocketAddr]) -> Result<Client, String> {
    let builder = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(FETCH_TIMEOUT);
    let builder = match url.host() {
        Some(Host::Domain(domain)) => builder.resolve_to_addrs(domain, addresses),
        _ => builder,
    };
    builder.build().map_err(|error| error.to_string())
}

fn redirect_target(current: &Url, location: &str) -> Result<Url, String> {
    let target = current
        .join(location)
        .map_err(|error| error.to_string())?;
    validate_url(&target)?;
    if current.scheme() == "https" && target.scheme() != "https" {
        return Err("HTTPS resource redirects cannot downgrade to HTTP".to_string());
    }
    Ok(target)
}

fn is_blocked_hostname(hostname: &str) -> bool {
    let hostname = hostname.trim_end_matches('.').to_ascii_lowercase();
    hostname == "localhost"
        || hostname.ends_with(".localhost")
        || hostname.ends_with(".local")
        || hostname.ends_with(".internal")
        || hostname.ends_with(".lan")
        || hostname.ends_with(".home.arpa")
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || address.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192
                    && ((octets[1] == 0 && (octets[2] == 0 || octets[2] == 2))
                        || (octets[1] == 88 && octets[2] == 99)))
                || (octets[0] == 198
                    && ((octets[1] == 18 || octets[1] == 19)
                        || (octets[1] == 51 && octets[2] == 100)))
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
                || octets[0] >= 240)
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001
                    && (segments[1] <= 0x01ff || segments[1] == 0x0db8))
                && segments[0] != 0x2002
        }
    }
}

fn append_bounded_chunk(body: &mut Vec<u8>, chunk: &[u8], max_bytes: usize) -> bool {
    if chunk.len() > max_bytes.saturating_sub(body.len()) {
        return false;
    }
    body.extend_from_slice(chunk);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static FETCH_LIMITER_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn only_public_ip_ranges_are_allowed() {
        for address in [
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
        ] {
            assert!(is_public_ip(address.parse().unwrap()), "{address}");
        }
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(address.parse().unwrap()), "{address}");
        }
    }

    #[test]
    fn rejects_local_hostnames_and_https_downgrades() {
        for hostname in ["localhost", "mail.local", "printer.internal", "router.home.arpa"] {
            assert!(is_blocked_hostname(hostname), "{hostname}");
        }

        let current = Url::parse("https://example.com/image.png").unwrap();
        assert!(redirect_target(&current, "http://example.com/image.png").is_err());
        assert_eq!(
            redirect_target(&current, "/next.png").unwrap().as_str(),
            "https://example.com/next.png"
        );
    }

    #[test]
    fn streaming_body_limit_accepts_exact_limit_and_rejects_excess() {
        let mut body = Vec::new();
        assert!(append_bounded_chunk(&mut body, b"1234", 6));
        assert!(append_bounded_chunk(&mut body, b"56", 6));
        assert!(!append_bounded_chunk(&mut body, b"7", 6));
        assert_eq!(body, b"123456");
    }

    #[test]
    fn outstanding_fetches_are_limited() {
        let _guard = FETCH_LIMITER_TEST_LOCK.lock().unwrap();
        let permits = (0..MAX_OUTSTANDING_FETCHES)
            .map(|_| try_acquire_fetch().expect("fetch permit should be available"))
            .collect::<Vec<_>>();
        assert!(try_acquire_fetch().is_none());
        drop(permits);
        assert!(try_acquire_fetch().is_some());
    }

    #[test]
    fn fetch_can_be_awaited_without_a_tokio_runtime() {
        let _guard = FETCH_LIMITER_TEST_LOCK.lock().unwrap();
        let url = Url::parse("file:///etc/hosts").unwrap();
        let permit = try_acquire_fetch().expect("fetch permit should be available");
        let result = futures_lite::future::block_on(fetch(url, permit));

        assert!(result.is_err());
    }
}