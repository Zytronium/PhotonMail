//! Mail backend: account config, IMAP workers, and the command/event types that
//! connect them to the UI.
//!
//! Architecture (all I/O lives off the UI thread):
//!
//! ```text
//!   UI thread (Freya)                       "photon-mail-io" thread (tokio)
//!   ─────────────────                       ───────────────────────────────
//!   AppState ── Command ──────────────────▶ manager ──▶ Worker (1 per account)
//!      ▲                                                  │  work connection: sync, bodies, flags, moves
//!      └──────────── Event (async_channel) ◀──────────────┤
//!                                                         └─ idle task: 2nd connection, IMAP IDLE on INBOX
//! ```

pub mod config;
pub mod discovery;
pub mod engine;
mod imap;
pub mod parse;
pub mod quoting;
mod smtp;

pub use config::AccountConfig;
pub use engine::Engine;

use std::sync::Arc;

use crate::data::MailContentType;

// Folder slots. These match the order of `data::FOLDERS` so the existing sidebar and
// Ctrl+1..6 shortcuts keep working. Each account maps its server mailboxes onto these
// six roles (via RFC 6154 SPECIAL-USE attributes, falling back to common names).
pub const INBOX: usize = 0;
pub const DRAFTS: usize = 1;
pub const SENT: usize = 2;
pub const ARCHIVE: usize = 3;
pub const SPAM: usize = 4;
pub const TRASH: usize = 5;

#[derive(Clone, PartialEq)]
pub enum SyncStatus {
    Connecting,
    Syncing,
    /// Unix timestamp of the last completed sync pass.
    Idle { at: i64 },
    Error(String),
}

/// Everything the message list needs, fetched without touching message bodies.
#[derive(Clone)]
pub struct HeaderRow {
    pub uid: u32,
    pub unread: bool,
    /// Sender name (or "To: …" for Sent/Drafts).
    pub from: String,
    pub from_email: String,
    pub subject: String,
    /// INTERNALDATE as a unix timestamp.
    pub ts: i64,
    pub size: u32,
    /// RFC 2822 Message-ID from the ENVELOPE (without angle brackets).
    pub message_id: String,
    /// RFC 2822 In-Reply-To from the ENVELOPE (without angle brackets).
    pub in_reply_to: String,
}

/// Metadata about a single email attachment.
#[derive(Clone, Debug)]
pub struct AttachmentInfo {
    /// Suggested filename from Content-Disposition or Content-Type.
    pub name: String,
    /// Size in bytes of the decoded attachment data.
    pub size: usize,
    /// MIME content type (e.g. "application/pdf").
    pub content_type: String,
    /// Decoded binary data of the attachment, if inlined with the body.
    pub data: Option<Vec<u8>>,
    /// Content-ID (without angle brackets) for inline parts referenced as `cid:` from the HTML.
    pub content_id: Option<String>,
}

#[derive(Clone)]
pub struct BodyData {
    pub content_type: MailContentType,
    pub text: Arc<str>,
    pub preview: String,
    /// Attachments found in the message.
    pub attachments: Vec<AttachmentInfo>,
}

/// Worker → UI.
pub enum Event {
    Status { account: usize, status: SyncStatus },
    /// New or updated rows (upsert by uid).
    Headers { account: usize, folder: usize, rows: Vec<HeaderRow> },
    /// Flag changes made elsewhere: (uid, unread).
    Flags { account: usize, folder: usize, updates: Vec<(u32, bool)> },
    /// Messages that no longer exist on the server.
    Removed { account: usize, folder: usize, uids: Vec<u32> },
    /// UIDVALIDITY changed: every cached uid in this folder is meaningless.
    Reset { account: usize, folder: usize },
    /// A move failed; bring the optimistically removed message back.
    Restore { account: usize, folder: usize, uid: u32 },
    Body { account: usize, folder: usize, uid: u32, result: Result<BodyData, String> },
    Notice { text: String },
    AccountAdded { cfg: AccountConfig },
    AddAccountFailed { error: String },
    /// A draft was saved on the server; `uid` is the (possibly new) UID of the draft.
    DraftSaved { account: usize, uid: u32 },
    /// Email was sent successfully.
    EmailSent { account: usize },
    /// Email sending failed.
    EmailSendFailed { account: usize, error: String },
}

/// UI → one account's worker.
pub enum WorkerCmd {
    /// Sync a folder if it has never been synced or is stale.
    SyncFolder { folder: usize },
    /// Re-sync every folder now.
    SyncAll,
    /// Incremental INBOX sync (sent by the IDLE task).
    Refresh,
    FetchBody { folder: usize, uid: u32 },
    SetSeen { folder: usize, uid: u32, seen: bool },
    MoveTo { folder: usize, uid: u32, to: usize },
    /// Append a new draft message to the Drafts folder on the server.
    AppendDraft { raw: String },
    /// Update an existing draft: delete the old one (by UID) and append the new content.
    UpdateDraft { uid: u32, raw: String },
    /// Send an email via SMTP. `raw` is the RFC-2822 message.
    SendEmail { raw: String },
}

/// UI → manager.
pub enum Command {
    Account { account: usize, cmd: WorkerCmd },
    AddAccount { cfg: AccountConfig, password: String },
    /// Update an existing account's config (e.g. SMTP settings) and persist to disk.
    UpdateAccount { index: usize, cfg: AccountConfig },
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
