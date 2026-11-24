//! The IMAP side: one `Worker` per account (the "work" connection) plus an `idle_loop`
//! (a second connection that sits in IMAP IDLE on INBOX and pokes the worker).
//!
//! Efficiency notes, in order of impact:
//!
//! * **Headers only.** The list is built from a single batched
//!   `FETCH (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)`; no bodies, no per-message
//!   round trips. The newest page is fetched and shown first, then older rows follow.
//! * **Bodies on demand** (`BODY.PEEK[]`, so reading never sets `\Seen` by accident),
//!   plus a small background prefetch of the newest INBOX messages so they open instantly.
//! * **Incremental sync.** After the first pass we only ask for `UID last+1:*`, plus a
//!   cheap `(UID FLAGS)` sweep over the window we already know to detect read/unread
//!   changes and deletions made by other clients. UIDVALIDITY is checked on every SELECT.
//! * **Push, not poll.** IDLE on its own connection wakes the worker the moment the
//!   server reports a change.
//! * **User actions jump the queue.** Background work is split into small jobs, and
//!   commands are drained between jobs, so clicking a message never waits for a sync.
//! * **Resilient.** Every command has a timeout; any connection-level failure drops the
//!   session and reconnects with backoff. Because per-folder state survives a reconnect,
//!   recovery is an incremental sync, not a full re-download.

use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    fmt::Display,
    future::Future,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use async_imap::{
    extensions::idle::IdleResponse,
    types::{Fetch, Flag, Name, NameAttribute},
    Authenticator,
};
use futures::TryStreamExt;
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
    time::{sleep, timeout},
};
use tokio_rustls::{
    rustls::{self, pki_types::ServerName},
    TlsConnector,
};

use super::{
    config::{self, AccountConfig},
    now, parse, BodyData, Event, HeaderRow, SyncStatus, WorkerCmd, ARCHIVE, DRAFTS, INBOX, SENT,
    SPAM, TRASH,
};
use crate::data::FOLDERS;

type Res<T> = Result<T, String>;
type Session = async_imap::Session<tokio_rustls::client::TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(20 * 60); // servers may drop IDLE at 30 min
const REFRESH_AFTER: Duration = Duration::from_secs(20);

/// Newest messages fetched first, so the list paints quickly.
const FIRST_PAGE: u32 = 50;
/// Most recent messages kept per folder after the first sync.
const INBOX_LIMIT: u32 = 300;
const OTHER_LIMIT: u32 = 100;

const PREFETCH_COUNT: usize = 20;
const PREFETCH_MAX_SIZE: u32 = 512 * 1024;
const PREFETCH_BATCH: usize = 5;
const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

const HEADER_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)";

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

trait Ctx<T> {
    fn ctx(self, what: &str) -> Res<T>;
}
impl<T, E: Display> Ctx<T> for Result<T, E> {
    fn ctx(self, what: &str) -> Res<T> {
        self.map_err(|e| format!("{what}: {e}"))
    }
}

/// Run one IMAP operation under a timeout so a dead connection can't hang a worker.
async fn io<T, E: Display>(what: &str, fut: impl Future<Output = Result<T, E>>) -> Res<T> {
    match timeout(IO_TIMEOUT, fut).await {
        Ok(result) => result.ctx(what),
        Err(_) => Err(format!("{what}: timed out")),
    }
}

fn not_connected() -> String {
    "not connected".to_string()
}

fn is_connection_failure(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error == "not connected"
        || error.contains(": io:")
        || error.starts_with("io:")
        || error.contains(": connection lost")
        || error.starts_with("connection lost")
        || error.contains(": parse:")
        || error.starts_with("parse:")
        || error.contains("timed out")
}

fn message_id_for() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "<{nanos:x}.{}.{:x}@photonmail.local>",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn message_timestamp(date: Option<&[u8]>, internal_date: Option<i64>) -> i64 {
    date.and_then(|date| std::str::from_utf8(date).ok())
        .and_then(|date| chrono::DateTime::parse_from_rfc2822(date).ok())
        .map(|date| date.timestamp())
        .or(internal_date)
        .unwrap_or(0)
}

fn replace_message_id(raw: &str, message_id: &str) -> String {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let (headers, body) = normalized.split_once("\n\n").unwrap_or((&normalized, ""));
    let mut output = String::new();
    let mut skipping_message_id = false;
    for line in headers.lines() {
        if line.starts_with([' ', '\t']) {
            if !skipping_message_id {
                output.push_str(line);
                output.push_str("\r\n");
            }
            continue;
        }
        skipping_message_id = line
            .split_once(':')
            .is_some_and(|(name, _)| name.eq_ignore_ascii_case("message-id"));
        if !skipping_message_id {
            output.push_str(line);
            output.push_str("\r\n");
        }
    }
    output.push_str("Message-ID: ");
    output.push_str(message_id);
    output.push_str("\r\n\r\n");
    output.push_str(&body.replace('\n', "\r\n"));
    output
}

async fn expunge_uid(session: &mut Session, uid: u32, has_uidplus: bool) -> Res<()> {
    let set = uid.to_string();
    if has_uidplus {
        io("UID EXPUNGE", async {
            session.uid_expunge(&set).await?.try_collect::<Vec<u32>>().await
        })
        .await?;
        return Ok(());
    }

    // RFC 4315's fallback preserves other clients' \\Deleted messages around EXPUNGE.
    let deleted: std::collections::HashSet<u32> = io("UID SEARCH DELETED", session.uid_search("DELETED")).await?;
    let preserve: Vec<String> = deleted
        .into_iter()
        .filter(|deleted_uid| *deleted_uid != uid)
        .map(|deleted_uid| deleted_uid.to_string())
        .collect();
    let preserve_set = preserve.join(",");
    if !preserve_set.is_empty() {
        io("STORE", async {
            session
                .uid_store(&preserve_set, "-FLAGS.SILENT (\\Deleted)")
                .await?
                .try_collect::<Vec<Fetch>>()
                .await
        })
        .await?;
    }

    let expunge_result = io("EXPUNGE", async {
        session.expunge().await?.try_collect::<Vec<u32>>().await
    })
    .await;
    let restore_result = if preserve_set.is_empty() {
        Ok(())
    } else {
        io("STORE", async {
            session
                .uid_store(&preserve_set, "+FLAGS.SILENT (\\Deleted)")
                .await?
                .try_collect::<Vec<Fetch>>()
                .await
        })
        .await
        .map(|_| ())
    };
    expunge_result?;
    restore_result
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            // Name the crypto provider explicitly so we never depend on a process-wide
            // default (other crates in the tree may enable a different provider).
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("rustls protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// TCP → TLS (implicit, normally port 993) or STARTTLS (port 143) → authenticate.
async fn connect(cfg: &AccountConfig, password: &str) -> Res<Session> {
    let tcp = match timeout(CONNECT_TIMEOUT, TcpStream::connect((cfg.host.as_str(), cfg.port))).await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return Err(format!("Can't reach {}:{} ({e})", cfg.host, cfg.port)),
        Err(_) => return Err(format!("Timed out connecting to {}:{}", cfg.host, cfg.port)),
    };
    let _ = tcp.set_nodelay(true);

    if cfg.port == 143 {
        let mut client = async_imap::Client::new(tcp);
        match timeout(IO_TIMEOUT, client.read_response()).await {
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) => return Err("IMAP server closed before greeting".to_string()),
            Ok(Err(e)) => return Err(format!("IMAP greeting: {e}")),
            Err(_) => return Err("IMAP greeting timed out".to_string()),
        }
        match timeout(IO_TIMEOUT, client.run_command_and_check_ok("STARTTLS", None)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("STARTTLS failed: {e}")),
            Err(_) => return Err("STARTTLS timed out".to_string()),
        }
        let tcp = client.into_inner();
        let tls = tls_stream(&cfg.host, tcp).await?;
        authenticate(async_imap::Client::new(tls), cfg, password).await
    } else {
        let tls = tls_stream(&cfg.host, tcp).await?;
        authenticate(async_imap::Client::new(tls), cfg, password).await
    }
}

async fn tls_stream(
    host: &str,
    tcp: TcpStream,
) -> Res<tokio_rustls::client::TlsStream<TcpStream>> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| format!("Invalid server name: {host}"))?;
    io("TLS handshake", TlsConnector::from(tls_config()).connect(name, tcp)).await
}

struct Xoauth2Authenticator {
    username: String,
    token: String,
}

impl Authenticator for Xoauth2Authenticator {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.username, self.token)
    }
}

async fn authenticate(
    client: async_imap::Client<tokio_rustls::client::TlsStream<TcpStream>>,
    cfg: &AccountConfig,
    secret: &str,
) -> Res<Session> {
    let auth = if let Some(token) = secret.strip_prefix("oauth2:") {
        if token.is_empty() {
            return Err("OAuth2 access token is empty".to_string());
        }
        timeout(
            IO_TIMEOUT,
            client.authenticate(
                "XOAUTH2",
                Xoauth2Authenticator { username: cfg.username.clone(), token: token.to_string() },
            ),
        )
        .await
    } else {
        timeout(IO_TIMEOUT, client.login(&cfg.username, secret)).await
    };
    match auth {
        Ok(Ok(session)) => Ok(session),
        Ok(Err((e, _client))) => Err(format!("Authentication failed: {e}")),
        Err(_) => Err("Authentication timed out".to_string()),
    }
}

/// Used by "Add account": proves host, TLS and credentials work before anything is saved.
pub async fn verify(cfg: &AccountConfig, password: &str) -> Res<()> {
    let mut session = connect(cfg, password).await?;
    let _ = timeout(Duration::from_secs(5), session.logout()).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Folder discovery
// ---------------------------------------------------------------------------

/// Map the server's mailboxes onto our six fixed roles.
/// Priority: RFC 6154 SPECIAL-USE attribute, then well-known names.
fn map_folders(names: &[Name]) -> [Option<String>; 6] {
    let mut by_attr: [Option<String>; 6] = Default::default();
    let mut by_name: [Option<String>; 6] = Default::default();
    let mut all_mail: Option<String> = None; // Gmail's "\All": archive of last resort

    for n in names {
        let path = n.name();
        let attrs = n.attributes();
        if attrs.iter().any(|a| matches!(a, NameAttribute::NoSelect)) {
            continue;
        }

        let role = attrs.iter().find_map(|a| match a {
            NameAttribute::Drafts => Some(DRAFTS),
            NameAttribute::Sent => Some(SENT),
            NameAttribute::Archive => Some(ARCHIVE),
            NameAttribute::Junk => Some(SPAM),
            NameAttribute::Trash => Some(TRASH),
            _ => None,
        });
        if let Some(role) = role {
            by_attr[role].get_or_insert_with(|| path.to_string());
            continue;
        }
        if attrs.iter().any(|a| matches!(a, NameAttribute::All)) {
            all_mail.get_or_insert_with(|| path.to_string());
            continue;
        }

        // Fall back to the last path segment's name ("[Gmail]/Spam" → "spam").
        let leaf = path
            .rsplit(|c| c == '/' || c == '.')
            .next()
            .unwrap_or(path)
            .to_lowercase();
        let guess = match leaf.as_str() {
            "drafts" | "draft" => Some(DRAFTS),
            "sent" | "sent items" | "sent mail" | "sent messages" => Some(SENT),
            "archive" | "archives" => Some(ARCHIVE),
            "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" => Some(SPAM),
            "trash" | "deleted" | "deleted items" | "deleted messages" | "bin" => Some(TRASH),
            _ => None,
        };
        if let Some(role) = guess {
            by_name[role].get_or_insert_with(|| path.to_string());
        }
    }

    let mut out: [Option<String>; 6] = Default::default();
    for role in [DRAFTS, SENT, ARCHIVE, SPAM, TRASH] {
        out[role] = by_attr[role].take().or_else(|| by_name[role].take());
    }
    if out[ARCHIVE].is_none() {
        out[ARCHIVE] = all_mail;
    }
    out[INBOX] = Some("INBOX".to_string());
    out
}

// ---------------------------------------------------------------------------
// FETCH helpers
// ---------------------------------------------------------------------------

async fn fetch_seq(session: &mut Session, set: &str, query: &str) -> Res<Vec<Fetch>> {
    io("FETCH", async {
        session.fetch(set, query).await?.try_collect::<Vec<Fetch>>().await
    })
        .await
}

async fn fetch_uid(session: &mut Session, set: &str, query: &str) -> Res<Vec<Fetch>> {
    io("UID FETCH", async {
        session.uid_fetch(set, query).await?.try_collect::<Vec<Fetch>>().await
    })
        .await
}

fn is_unread(f: &Fetch) -> bool {
    !f.flags().any(|flag| matches!(flag, Flag::Seen))
}

fn lossy(bytes: &Option<Cow<'_, [u8]>>) -> String {
    bytes
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

/// Bare `user@host` addresses from an ENVELOPE address list. Entries without both a mailbox
/// and a host are skipped (they are RFC 3501 group start/end markers).
macro_rules! addr_emails {
    ($list:expr) => {{
        let mut out: Vec<String> = Vec::new();
        if let Some(list) = $list {
            for a in list {
                let mailbox = lossy(&a.mailbox);
                let host = lossy(&a.host);
                if !mailbox.is_empty() && !host.is_empty() {
                    out.push(format!("{mailbox}@{host}"));
                }
            }
        }
        out
    }};
}

fn header_row(f: &Fetch, sent_like: bool) -> Option<HeaderRow> {
    let uid = f.uid?;
    let env = f.envelope();

    let subject = env
        .and_then(|e| e.subject.as_deref())
        .map(parse::decode_text)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no subject)".to_string());

    // Inbox-like folders show who it's from; Sent/Drafts show who it's to.
    let people = env.and_then(|e| if sent_like { e.to.as_ref() } else { e.from.as_ref() });
    let first = people.and_then(|v| v.first());

    let email = match first {
        Some(a) => {
            let mailbox = lossy(&a.mailbox);
            let host = lossy(&a.host);
            if host.is_empty() { mailbox } else { format!("{mailbox}@{host}") }
        }
        None => String::new(),
    };
    let name = first
        .and_then(|a| a.name.as_deref())
        .map(parse::decode_text)
        .filter(|n| !n.is_empty());

    let mut from = name.unwrap_or_else(|| email.clone());
    if from.is_empty() {
        from = if sent_like { "(no recipient)" } else { "(unknown sender)" }.to_string();
    }
    if sent_like {
        from = format!("To: {from}");
    }

    // Message-ID and In-Reply-To for reply threading. The ENVELOPE returns them exactly as
    // written in the header, i.e. with the angle brackets; we store them without.
    let message_id = env
        .and_then(|e| e.message_id.as_deref())
        .map(|m| String::from_utf8_lossy(m).into_owned())
        .and_then(|m| parse::message_ids(&m).into_iter().next())
        .unwrap_or_default();
    let in_reply_to = env
        .and_then(|e| e.in_reply_to.as_deref())
        .map(|m| parse::message_ids(&String::from_utf8_lossy(m)).join(" "))
        .unwrap_or_default();

    // Recipients (for Reply All and for reopening drafts) and Reply-To.
    let to_emails = addr_emails!(env.and_then(|e| e.to.as_ref()));
    let cc_emails = addr_emails!(env.and_then(|e| e.cc.as_ref()));
    let bcc_emails = addr_emails!(env.and_then(|e| e.bcc.as_ref()));
    let reply_to = if sent_like {
        String::new()
    } else {
        addr_emails!(env.and_then(|e| e.reply_to.as_ref())).join(", ")
    };

    Some(HeaderRow {
        uid,
        unread: is_unread(f),
        from,
        from_email: email,
        subject,
        ts: message_timestamp(
            env.and_then(|envelope| envelope.date.as_deref()),
            f.internal_date().map(|date| date.timestamp()),
        ),
        size: f.size.unwrap_or(0),
        message_id,
        in_reply_to,
        to_emails,
        cc_emails,
        bcc_emails,
        reply_to,
    })
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FolderState {
    uid_validity: Option<u32>,
    last_uid: u32,
    /// uid → unread, for the window we hold. Used to diff flags and detect deletions.
    known: BTreeMap<u32, bool>,
    synced: bool,
    last_sync: Option<Instant>,
}

fn prune_known(known: &mut BTreeMap<u32, bool>, keep_limit: usize) -> Vec<u32> {
    let remove_count = known.len().saturating_sub(keep_limit);
    let removed: Vec<u32> = known.keys().take(remove_count).copied().collect();
    for uid in &removed {
        known.remove(uid);
    }
    removed
}

enum Job {
    Sync(usize),
    Prefetch { folder: usize, uids: Vec<u32> },
}

enum WorkerSignal {
    Refresh,
    SendFinished(SendResult),
}

struct SendResult {
    raw: String,
    draft_uid: Option<u32>,
    result: Result<(), String>,
}

pub struct Worker {
    index: usize,
    cfg: AccountConfig,
    password: String,
    events: async_channel::Sender<Event>,
    rx: UnboundedReceiver<WorkerCmd>,
    signals_tx: UnboundedSender<WorkerSignal>,
    signals_rx: UnboundedReceiver<WorkerSignal>,
    backlog: VecDeque<WorkerCmd>,
    jobs: VecDeque<Job>,
    session: Option<Session>,
    selected: Option<String>,
    paths: [Option<String>; 6],
    has_move: bool,
    has_uidplus: bool,
    folders: [FolderState; 6],
    busy: bool,
}

impl Worker {
    fn new(
        index: usize,
        cfg: AccountConfig,
        password: String,
        events: async_channel::Sender<Event>,
        rx: UnboundedReceiver<WorkerCmd>,
        signals_tx: UnboundedSender<WorkerSignal>,
        signals_rx: UnboundedReceiver<WorkerSignal>,
    ) -> Self {
        Worker {
            index,
            cfg,
            password,
            events,
            rx,
            signals_tx,
            signals_rx,
            backlog: VecDeque::new(),
            jobs: VecDeque::new(),
            session: None,
            selected: None,
            paths: Default::default(),
            has_move: false,
            has_uidplus: false,
            folders: Default::default(),
            busy: false,
        }
    }

    fn emit(&self, ev: Event) {
        let _ = self.events.try_send(ev);
    }

    fn status(&self, status: SyncStatus) {
        self.emit(Event::Status { account: self.index, status });
    }

    pub async fn run(mut self) {
        let mut backoff = Duration::from_secs(2);
        loop {
            // 1. Make sure we have a live session.
            if self.session.is_none() {
                self.busy = false;
                self.status(SyncStatus::Connecting);
                match self.connect_and_discover().await {
                    Ok(()) => {
                        backoff = Duration::from_secs(2);
                        self.queue_initial_jobs();
                    }
                    Err(e) => {
                        self.status(SyncStatus::Error(e));
                        tokio::select! {
                            _ = sleep(backoff) => {}
                            cmd = self.rx.recv() => match cmd {
                                Some(c) => self.backlog.push_back(c),
                                None => return,
                            },
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                        continue;
                    }
                }
            }

            // 2. Pick the next piece of work: user commands first, then background jobs.
            let next = match self.backlog.pop_front() {
                Some(c) => Some(c),
                None => match self.rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                },
            };

            let signal = if next.is_none() {
                self.signals_rx.try_recv().ok()
            } else {
                None
            };

            let result = if let Some(cmd) = next {
                self.mark_busy();
                self.handle(cmd).await
            } else if let Some(signal) = signal {
                match signal {
                    WorkerSignal::Refresh => {
                        self.push_sync_front(INBOX);
                        Ok(())
                    }
                    WorkerSignal::SendFinished(result) => {
                        self.mark_busy();
                        self.finish_send(result).await
                    }
                }
            } else if let Some(job) = self.jobs.pop_front() {
                self.mark_busy();
                self.run_job(job).await
            } else {
                self.mark_idle();
                let mut completed_send = None;
                tokio::select! {
                    cmd = self.rx.recv() => match cmd {
                        Some(c) => self.backlog.push_back(c),
                        None => return,
                    },
                    signal = self.signals_rx.recv() => match signal {
                        Some(WorkerSignal::Refresh) => self.backlog.push_back(WorkerCmd::Refresh),
                        Some(WorkerSignal::SendFinished(send)) => completed_send = Some(send),
                        None => {}
                    },
                }
                if let Some(send) = completed_send {
                    self.mark_busy();
                    self.finish_send(send).await
                } else {
                    Ok(())
                }
            };

            // 3. Any connection-level failure: drop the session and start over.
            //    Folder state is kept, so the retry is an incremental sync.
            if let Err(e) = result {
                if is_connection_failure(&e) {
                    self.session = None;
                    self.selected = None;
                    self.jobs.clear();
                    self.busy = false;
                    self.status(SyncStatus::Error(e));
                    sleep(Duration::from_secs(2)).await;
                } else {
                    self.emit(Event::Notice { text: e });
                }
            }
        }
    }

    fn mark_busy(&mut self) {
        if !self.busy {
            self.busy = true;
            self.status(SyncStatus::Syncing);
        }
    }

    fn mark_idle(&mut self) {
        if self.busy {
            self.busy = false;
            self.status(SyncStatus::Idle { at: now() });
        }
    }

    async fn connect_and_discover(&mut self) -> Res<()> {
        let mut session = connect(&self.cfg, &self.password).await?;
        let caps = io("CAPABILITY", session.capabilities()).await?;
        self.has_move = caps.has_str("MOVE");
        self.has_uidplus = caps.has_str("UIDPLUS");

        let names: Vec<Name> = io("LIST", async {
            session.list(Some(""), Some("*")).await?.try_collect::<Vec<Name>>().await
        })
            .await?;
        self.paths = map_folders(&names);
        self.session = Some(session);
        self.selected = None;
        Ok(())
    }

    /// INBOX first (and its prefetch), then the other folders.
    fn queue_initial_jobs(&mut self) {
        self.jobs.clear();
        self.jobs.push_back(Job::Sync(INBOX));
        for f in [SENT, DRAFTS, ARCHIVE, SPAM, TRASH] {
            if self.paths[f].is_some() {
                self.jobs.push_back(Job::Sync(f));
            }
        }
    }

    fn push_sync_front(&mut self, folder: usize) {
        self.jobs.retain(|j| !matches!(j, Job::Sync(f) if *f == folder));
        self.jobs.push_front(Job::Sync(folder));
    }

    // -- commands ---------------------------------------------------------------

    async fn handle(&mut self, cmd: WorkerCmd) -> Res<()> {
        match cmd {
            WorkerCmd::SyncFolder { folder } => {
                let stale = self.folders[folder]
                    .last_sync
                    .map_or(true, |t| t.elapsed() > REFRESH_AFTER);
                if stale {
                    self.push_sync_front(folder);
                }
                Ok(())
            }
            WorkerCmd::SyncAll => {
                for f in [TRASH, SPAM, ARCHIVE, DRAFTS, SENT, INBOX] {
                    if self.paths[f].is_some() {
                        self.push_sync_front(f);
                    }
                }
                Ok(())
            }
            WorkerCmd::Refresh => {
                self.push_sync_front(INBOX);
                Ok(())
            }
            WorkerCmd::FetchBody { folder, uid } => {
                let result = if self.ensure_selected(folder).await? {
                    self.fetch_bodies(&[uid])
                        .await?
                        .into_iter()
                        .next()
                        .map(|(_, body)| body)
                        .unwrap_or_else(|| Err("This message is no longer on the server.".to_string()))
                } else {
                    Err("This folder isn't available on the server.".to_string())
                };
                self.emit(Event::Body { account: self.index, folder, uid, result });
                Ok(())
            }
            WorkerCmd::SetSeen { folder, uid, seen } => {
                if !self.ensure_selected(folder).await? {
                    return Ok(());
                }
                let query = if seen { "+FLAGS.SILENT (\\Seen)" } else { "-FLAGS.SILENT (\\Seen)" };
                let set = uid.to_string();
                let session = self.session.as_mut().ok_or_else(not_connected)?;
                io("STORE", async {
                    session.uid_store(&set, query).await?.try_collect::<Vec<Fetch>>().await
                })
                    .await?;
                if let Some(unread) = self.folders[folder].known.get_mut(&uid) {
                    *unread = !seen;
                }
                Ok(())
            }
            WorkerCmd::MoveTo { folder, uid, to } => {
                let result = self.move_message(folder, uid, to).await;
                match &result {
                    Ok(true) => {}
                    Ok(false) => {
                        self.emit(Event::Notice {
                            text: format!("This account has no {} folder.", FOLDERS[to]),
                        });
                        self.emit(Event::Restore { account: self.index, folder, uid });
                    }
                    Err(_) => self.emit(Event::Restore { account: self.index, folder, uid }),
                }
                result.map(|_| ())
            }
            WorkerCmd::AppendDraft { raw } => {
                let new_uid = self.append_draft(raw).await?;
                if let Some(uid) = new_uid {
                    self.emit(Event::DraftSaved { account: self.index, uid });
                }
                Ok(())
            }
            WorkerCmd::UpdateDraft { uid, raw } => {
                let new_uid = self.update_draft(uid, raw).await?;
                if let Some(new_uid) = new_uid {
                    self.emit(Event::DraftSaved { account: self.index, uid: new_uid });
                }
                Ok(())
            }
            WorkerCmd::DeleteDraft { uid } => {
                self.delete_message(DRAFTS, uid).await?;
                self.push_sync_front(DRAFTS);
                Ok(())
            }
            WorkerCmd::UpdateConfig { cfg } => {
                // IMAP connection settings changed: drop the session so the run loop reconnects
                // with the new values. Anything else (SMTP host/port, name, …) just applies.
                let reconnect = cfg.host != self.cfg.host
                    || cfg.port != self.cfg.port
                    || cfg.username != self.cfg.username;
                self.cfg = cfg;
                if reconnect {
                    self.session = None;
                    self.selected = None;
                }
                Ok(())
            }
            WorkerCmd::SendEmail { raw, draft_uid } => {
                let cfg = self.cfg.clone();
                let password = self.password.clone();
                let signals = self.signals_tx.clone();
                tokio::spawn(async move {
                    let result = super::smtp::send_email(&cfg, &password, &raw).await;
                    let _ = signals.send(WorkerSignal::SendFinished(SendResult {
                        raw,
                        draft_uid,
                        result,
                    }));
                });
                Ok(())
            }
        }
    }

    async fn finish_send(&mut self, send: SendResult) -> Res<()> {
        match send.result {
            Ok(()) => {
                self.emit(Event::EmailSent { account: self.index });
                if let Some(uid) = send.draft_uid {
                    match self.delete_message(DRAFTS, uid).await {
                        Ok(()) => self.push_sync_front(DRAFTS),
                        Err(e) => self.emit(Event::Notice {
                            text: format!("Sent, but could not remove the draft: {e}"),
                        }),
                    }
                }
                if !self.smtp_is_gmail() {
                    if let Err(e) = self.append_sent(&send.raw).await {
                        self.emit(Event::Notice {
                            text: format!("Sent, but could not save to Sent folder: {e}"),
                        });
                    }
                }
            }
            Err(error) => {
                let saved = match self.append_draft(send.raw).await {
                    Ok(Some(_)) => " A copy was saved to Drafts.",
                    _ => " It could not be saved to Drafts either.",
                };
                self.emit(Event::EmailSendFailed {
                    account: self.index,
                    error: format!("{}.{saved}", error.trim_end_matches('.')),
                });
            }
        }
        Ok(())
    }

    /// True when outgoing mail goes through Google's SMTP servers (which save their own Sent copy).
    fn smtp_is_gmail(&self) -> bool {
        super::smtp::resolve_smtp(&self.cfg).is_some_and(|(host, _)| {
            let host = host.to_lowercase();
            host == "gmail.com"
                || host.ends_with(".gmail.com")
                || host == "googlemail.com"
                || host.ends_with(".googlemail.com")
        })
    }

    /// Save a copy of a just-sent message in the Sent folder (marked Seen).
    async fn append_sent(&mut self, raw: &str) -> Res<()> {
        let Some(sent_path) = self.paths[SENT].clone() else {
            return Ok(());
        };
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        io("APPEND", async {
            session
                .append(&sent_path, Some("(\\Seen)"), None, raw.as_bytes())
                .await
        })
            .await?;
        self.push_sync_front(SENT);
        Ok(())
    }

    /// Append a raw RFC-2822 message to the Drafts folder on the server.
    /// Returns the UID of the newly appended draft.
    async fn append_draft(&mut self, raw: String) -> Res<Option<u32>> {
        let Some(drafts_path) = self.paths[DRAFTS].clone() else {
            self.emit(Event::Notice {
                text: "This account has no Drafts folder.".to_string(),
            });
            return Ok(None);
        };
        let message_id = message_id_for();
        let raw = replace_message_id(&raw, &message_id);
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        io("APPEND", async {
            session
                .append(&drafts_path, Some("(\\Draft)"), None, raw.as_bytes())
                .await
        })
            .await?;
        // Search for our unique Message-ID, not the highest UID (which may belong to another client).
        let new_uid = self.find_message_uid(DRAFTS, &message_id).await?;
        // Queue a re-sync so the new draft appears in the list, but do it asynchronously
        // so the compose pane doesn't lose focus immediately.
        self.push_sync_front(DRAFTS);
        Ok(new_uid)
    }

    /// Update an existing draft: append the new content first, and only then delete the old
    /// version, so a failed append can never lose the draft. Returns the UID of the new draft.
    async fn update_draft(&mut self, old_uid: u32, raw: String) -> Res<Option<u32>> {
        let new_uid = self.append_draft(raw).await?;
        if new_uid.is_some() {
            if let Err(e) = self.delete_message(DRAFTS, old_uid).await {
                // The new version is safely stored; at worst the old one lingers as a duplicate.
                self.emit(Event::Notice {
                    text: format!("Draft saved, but the previous version could not be removed: {e}"),
                });
            }
        }
        Ok(new_uid)
    }

    /// Delete one message from a folder: flag it \Deleted and expunge just that UID.
    async fn delete_message(&mut self, folder: usize, uid: u32) -> Res<()> {
        if !self.ensure_selected(folder).await? {
            return Ok(());
        }
        let has_uidplus = self.has_uidplus;
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        let set = uid.to_string();
        io("STORE", async {
            session
                .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                .await?
                .try_collect::<Vec<Fetch>>()
                .await
        })
            .await?;
        expunge_uid(session, uid, has_uidplus).await?;
        self.folders[folder].known.remove(&uid);
        Ok(())
    }

    /// Find a newly appended message by its unique RFC 5322 Message-ID.
    async fn find_message_uid(&mut self, folder: usize, message_id: &str) -> Res<Option<u32>> {
        if !self.ensure_selected(folder).await? {
            return Ok(None);
        }
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        let query = format!("HEADER Message-ID \"{message_id}\"");
        let uids: std::collections::HashSet<u32> = io("UID SEARCH", session.uid_search(query)).await?;
        Ok(uids.into_iter().max())
    }

    /// Returns Ok(false) if the destination folder doesn't exist on this account.
    async fn move_message(&mut self, folder: usize, uid: u32, to: usize) -> Res<bool> {
        let Some(dst) = self.paths[to].clone() else {
            return Ok(false);
        };
        if !self.ensure_selected(folder).await? {
            return Ok(false);
        }
        let has_move = self.has_move;
        let has_uidplus = self.has_uidplus;
        let set = uid.to_string();
        let session = self.session.as_mut().ok_or_else(not_connected)?;

        if has_move {
            io("MOVE", session.uid_mv(&set, &dst)).await?;
        } else {
            // No MOVE extension: COPY, flag \Deleted, expunge just that UID.
            io("COPY", session.uid_copy(&set, &dst)).await?;
            io("STORE", async {
                session
                    .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                    .await?
                    .try_collect::<Vec<Fetch>>()
                    .await
            })
                .await?;
            expunge_uid(session, uid, has_uidplus).await?;
        }

        self.folders[folder].known.remove(&uid);
        self.push_sync_front(to);
        self.push_sync_front(folder);
        Ok(true)
    }

    // -- jobs -------------------------------------------------------------------

    async fn run_job(&mut self, job: Job) -> Res<()> {
        match job {
            Job::Sync(folder) => {
                let new_rows = self.sync_folder(folder).await?;
                if folder == INBOX {
                    // Newest small messages get their bodies fetched in the background.
                    let mut pick: Vec<&HeaderRow> = new_rows
                        .iter()
                        .filter(|r| r.size <= PREFETCH_MAX_SIZE)
                        .collect();
                    pick.sort_by(|a, b| b.uid.cmp(&a.uid));
                    pick.truncate(PREFETCH_COUNT);
                    let uids: Vec<u32> = pick.iter().map(|r| r.uid).collect();
                    if !uids.is_empty() {
                        self.jobs.push_front(Job::Prefetch { folder, uids });
                    }
                }
                Ok(())
            }
            Job::Prefetch { folder, mut uids } => {
                let take = uids.len().min(PREFETCH_BATCH);
                let batch: Vec<u32> = uids.drain(..take).collect();
                if !uids.is_empty() {
                    self.jobs.push_front(Job::Prefetch { folder, uids });
                }
                if !self.ensure_selected(folder).await? {
                    return Ok(());
                }
                for (uid, body) in self.fetch_bodies(&batch).await? {
                    self.emit(Event::Body {
                        account: self.index,
                        folder,
                        uid,
                        result: body,
                    });
                }
                Ok(())
            }
        }
    }

    /// SELECT only if the folder isn't already selected. Ok(false) = no such folder.
    async fn ensure_selected(&mut self, folder: usize) -> Res<bool> {
        let Some(path) = self.paths[folder].clone() else {
            return Ok(false);
        };
        if self.selected.as_deref() == Some(path.as_str()) {
            return Ok(true);
        }
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        io("SELECT", session.select(&path)).await?;
        self.selected = Some(path);
        Ok(true)
    }

    /// Fetch and parse full messages from the currently selected folder.
    /// Messages missing from the server are simply absent from the result.
    async fn fetch_bodies(&mut self, uids: &[u32]) -> Res<Vec<(u32, Res<BodyData>)>> {
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let session = self.session.as_mut().ok_or_else(not_connected)?;
        let sizes = fetch_uid(session, &set, "(UID RFC822.SIZE)").await?;
        let mut results = Vec::new();
        let mut allowed = Vec::new();
        for fetch in &sizes {
            let Some(uid) = fetch.uid else { continue };
            if fetch.size.is_none() {
                results.push((uid, Err("The server did not provide this message's size; it was not loaded to avoid an unbounded download.".to_string())));
            } else if fetch.size.is_some_and(|size| size > MAX_MESSAGE_SIZE) {
                results.push((uid, Err(format!(
                    "This message is larger than the {} MiB display limit.",
                    MAX_MESSAGE_SIZE / (1024 * 1024)
                ))));
            } else {
                allowed.push(uid);
            }
        }
        if allowed.is_empty() {
            return Ok(results);
        }

        let allowed_set = allowed.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetched = fetch_uid(session, &allowed_set, "BODY.PEEK[]").await?;
        for fetch in &fetched {
            let Some(uid) = fetch.uid else { continue };
            let Some(body) = fetch.body() else { continue };
            let result = if body.len() > MAX_MESSAGE_SIZE as usize {
                Err(format!(
                    "This message is larger than the {} MiB display limit.",
                    MAX_MESSAGE_SIZE / (1024 * 1024)
                ))
            } else {
                Ok(parse::parse_body(body))
            };
            results.push((uid, result));
        }
        Ok(results)
    }

    /// Bring one folder up to date. Returns the rows that were new in this pass.
    async fn sync_folder(&mut self, f: usize) -> Res<Vec<HeaderRow>> {
        let events = self.events.clone();
        let index = self.index;

        let Some(path) = self.paths[f].clone() else {
            self.folders[f].synced = true;
            return Ok(Vec::new());
        };
        let sent_like = f == SENT || f == DRAFTS;

        let session = self.session.as_mut().ok_or_else(not_connected)?;
        let mailbox = io("SELECT", session.select(&path)).await?;
        self.selected = Some(path);

        let st = &mut self.folders[f];
        let validity = mailbox.uid_validity.unwrap_or(0);
        if st.uid_validity.is_some_and(|v| v != validity) {
            // The server renumbered the mailbox: our UIDs are meaningless now.
            *st = FolderState::default();
            let _ = events.try_send(Event::Reset { account: index, folder: f });
        }
        st.uid_validity = Some(validity);

        let mut new_rows: Vec<HeaderRow> = Vec::new();

        // Empty mailbox.
        if mailbox.exists == 0 {
            if !st.known.is_empty() {
                let uids = st.known.keys().copied().collect();
                let _ = events.try_send(Event::Removed { account: index, folder: f, uids });
                st.known.clear();
            }
            st.last_uid = 0;
            st.synced = true;
            st.last_sync = Some(Instant::now());
            return Ok(new_rows);
        }

        if !st.synced || st.last_uid == 0 {
            // First pass: newest page first (fast paint), then the rest of the window.
            let limit = if f == INBOX { INBOX_LIMIT } else { OTHER_LIMIT };
            let total = mailbox.exists;
            let lowest = total.saturating_sub(limit - 1).max(1);
            let split = total.saturating_sub(FIRST_PAGE - 1).max(lowest);

            let mut ranges = vec![format!("{split}:*")];
            if split > lowest {
                ranges.push(format!("{lowest}:{}", split - 1));
            }
            for range in ranges {
                let fetched = fetch_seq(session, &range, HEADER_QUERY).await?;
                let rows: Vec<HeaderRow> =
                    fetched.iter().filter_map(|x| header_row(x, sent_like)).collect();
                for r in &rows {
                    st.known.insert(r.uid, r.unread);
                    st.last_uid = st.last_uid.max(r.uid);
                }
                if !rows.is_empty() {
                    new_rows.extend(rows.iter().cloned());
                    let _ = events.try_send(Event::Headers { account: index, folder: f, rows });
                }
            }
        } else {
            // Incremental pass.
            // (a) Cheap flag sweep over the window we know: catches read/unread changes
            //     and deletions made by other clients.
            if let (Some(&lo), Some(&hi)) = (st.known.keys().next(), st.known.keys().next_back()) {
                let flags = fetch_uid(session, &format!("{lo}:{hi}"), "(UID FLAGS)").await?;
                let mut present = std::collections::HashSet::new();
                let mut updates = Vec::new();
                for fe in &flags {
                    if let Some(uid) = fe.uid {
                        present.insert(uid);
                        let unread = is_unread(fe);
                        if st.known.get(&uid).is_some_and(|&old| old != unread) {
                            st.known.insert(uid, unread);
                            updates.push((uid, unread));
                        }
                    }
                }
                let gone: Vec<u32> =
                    st.known.keys().copied().filter(|u| !present.contains(u)).collect();
                for u in &gone {
                    st.known.remove(u);
                }
                if !updates.is_empty() {
                    let _ = events.try_send(Event::Flags { account: index, folder: f, updates });
                }
                if !gone.is_empty() {
                    let _ = events.try_send(Event::Removed { account: index, folder: f, uids: gone });
                }
            }

            // (b) Fetch only the newest window of new UIDs so a long-offline account cannot
            //     create an unbounded header batch. UIDNEXT gives the current upper bound.
            let after = st.last_uid;
            let keep_limit = if f == INBOX { INBOX_LIMIT } else { OTHER_LIMIT };
            let (fetched, high_water) = if let Some(next_uid) = mailbox.uid_next {
                let high_water = next_uid.saturating_sub(1);
                if after >= high_water {
                    (Vec::new(), high_water)
                } else {
                    let earliest_kept = high_water.saturating_sub(keep_limit - 1);
                    let first_uid = earliest_kept.max(after.saturating_add(1));
                    if first_uid > high_water {
                        (Vec::new(), high_water)
                    } else {
                        (
                            fetch_uid(
                                session,
                                &format!("{first_uid}:{high_water}"),
                                HEADER_QUERY,
                            )
                            .await?,
                            high_water,
                        )
                    }
                }
            } else {
                // UIDNEXT is optional; sequence numbers still let us fetch a bounded newest page.
                let first_sequence = mailbox.exists.saturating_sub(keep_limit - 1).max(1);
                (fetch_seq(session, &format!("{first_sequence}:*"), HEADER_QUERY).await?, 0)
            };
            let rows: Vec<HeaderRow> = fetched
                .iter()
                .filter(|x| x.uid.is_some_and(|u| u > after))
                .filter_map(|x| header_row(x, sent_like))
                .collect();
            st.last_uid = st.last_uid.max(high_water);
            for r in &rows {
                st.known.insert(r.uid, r.unread);
                st.last_uid = st.last_uid.max(r.uid);
            }
            if !rows.is_empty() {
                new_rows.extend(rows.iter().cloned());
                let _ = events.try_send(Event::Headers { account: index, folder: f, rows });
            }
        }

        let keep_limit = if f == INBOX { INBOX_LIMIT as usize } else { OTHER_LIMIT as usize };
        let removed = prune_known(&mut st.known, keep_limit);
        if !removed.is_empty() {
            let _ = events.try_send(Event::Removed { account: index, folder: f, uids: removed });
        }

        st.synced = true;
        st.last_sync = Some(Instant::now());
        Ok(new_rows)
    }
}

// ---------------------------------------------------------------------------
// IDLE connection
// ---------------------------------------------------------------------------

/// Keeps a dedicated connection in IMAP IDLE on INBOX and pokes the worker when the
/// server reports a change. Reconnects forever with backoff.
async fn idle_loop(
    cfg: AccountConfig,
    password: String,
    poke: UnboundedSender<WorkerSignal>,
    mut cancel: oneshot::Receiver<()>,
) {
    let mut backoff = Duration::from_secs(5);
    loop {
        let result = tokio::select! {
            _ = &mut cancel => return,
            result = idle_session(&cfg, &password, &poke) => result,
        };
        match result {
            Ok(()) => backoff = Duration::from_secs(5),
            Err(_) => backoff = (backoff * 2).min(Duration::from_secs(300)),
        }
        tokio::select! {
            _ = &mut cancel => return,
            _ = sleep(backoff) => {}
        }
    }
}

async fn idle_session(
    cfg: &AccountConfig,
    password: &str,
    poke: &UnboundedSender<WorkerSignal>,
) -> Res<()> {
    let mut session = connect(cfg, password).await?;
    let caps = io("CAPABILITY", session.capabilities()).await?;

    if !caps.has_str("IDLE") {
        // Rare: no push support. Fall back to a gentle poll.
        loop {
            sleep(Duration::from_secs(120)).await;
            if poke.send(WorkerSignal::Refresh).is_err() {
                return Ok(());
            }
        }
    }

    io("SELECT", session.select("INBOX")).await?;
    loop {
        let mut handle = session.idle();
        io("IDLE", handle.init()).await?;
        // Keep `_stop` alive: dropping it would interrupt the wait immediately.
        let (wait, _stop) = handle.wait_with_timeout(IDLE_TIMEOUT);
        let reason = wait.await.ctx("IDLE wait")?;
        session = io("DONE", handle.done()).await?;

        if let IdleResponse::NewData(_) = reason {
            if poke.send(WorkerSignal::Refresh).is_err() {
                return Ok(()); // worker is gone
            }
            sleep(Duration::from_secs(1)).await; // coalesce bursts of notifications
        }
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Start the work connection and the IDLE connection for one account.
/// `password`: pass `Some` when it's already in hand (just added); otherwise it is read
/// from the OS keyring.
pub fn spawn_account(
    index: usize,
    cfg: AccountConfig,
    password: Option<String>,
    events: async_channel::Sender<Event>,
) -> UnboundedSender<WorkerCmd> {
    let (tx, rx) = unbounded_channel();

    tokio::spawn(async move {
        let password = match password {
            Some(p) => p,
            None => {
                let id = cfg.id.clone();
                let loaded = tokio::task::spawn_blocking(move || config::load_password(&id))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                match loaded {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = events.try_send(Event::Status {
                            account: index,
                            status: SyncStatus::Error(format!(
                                "No saved password found in the system keyring ({e})."
                            )),
                        });
                        return;
                    }
                }
            }
        };

        let (signals_tx, signals_rx) = unbounded_channel();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        tokio::spawn(idle_loop(
            cfg.clone(),
            password.clone(),
            signals_tx.clone(),
            cancel_rx,
        ));
        Worker::new(index, cfg, password, events, rx, signals_tx, signals_rx).run().await;
        let _ = cancel_tx.send(());
    });

    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_uid_window_prunes_only_oldest_uids() {
        let mut known = BTreeMap::from([(1, true), (2, false), (3, true), (4, false)]);
        let removed = prune_known(&mut known, 2);
        assert_eq!(removed, vec![1, 2]);
        assert_eq!(known, BTreeMap::from([(3, true), (4, false)]));
    }

    #[test]
    fn date_header_wins_and_internal_date_is_a_fallback() {
        let date = b"Fri, 21 Nov 1997 09:55:06 -0600";
        let expected = chrono::DateTime::parse_from_rfc2822(
            std::str::from_utf8(date).expect("valid date text"),
        )
        .expect("valid RFC 2822 date")
        .timestamp();
        assert_eq!(message_timestamp(Some(date), Some(123)), expected);
        assert_eq!(message_timestamp(None, Some(123)), 123);
        assert_eq!(message_timestamp(Some(b"not a date"), Some(123)), 123);
    }

    #[test]
    fn draft_message_id_is_unique_and_header_replacement_is_single() {
        let first = message_id_for();
        let second = message_id_for();
        assert_ne!(first, second);
        let replaced = replace_message_id(
            "Subject: test\r\nMessage-ID: <old@example.com>\r\n\tcontinued\r\n\r\nbody",
            &first,
        );
        assert_eq!(replaced.matches("Message-ID:").count(), 1);
        assert!(replaced.contains(&format!("Message-ID: {first}\r\n")));
        assert!(replaced.ends_with("\r\n\r\nbody"));
    }

    #[test]
    fn only_transport_errors_trigger_reconnects() {
        assert!(is_connection_failure("SELECT: io: connection reset"));
        assert!(is_connection_failure("SELECT: connection lost"));
        assert!(is_connection_failure("SELECT: timed out"));
        assert!(!is_connection_failure("STORE: no response: mailbox is read-only"));
        assert!(!is_connection_failure("UID MOVE: bad response: unsupported"));
    }

    #[test]
    fn xoauth2_response_uses_the_standard_bearer_payload() {
        let mut authenticator = Xoauth2Authenticator {
            username: "alice@example.com".to_string(),
            token: "access-token".to_string(),
        };
        assert_eq!(
            authenticator.process(&[]),
            "user=alice@example.com\x01auth=Bearer access-token\x01\x01"
        );
    }
}
