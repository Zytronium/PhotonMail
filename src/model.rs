//! UI-side mail model. One `Store` holds every account and every message header we
//! know about; `AppState` keeps it in a single Freya `State` and the list/reader
//! components read from it. Mail indices are stable (messages are tombstoned, never
//! removed from the Vec), so `AppState::selected` stays valid as mail comes and goes.

use std::{collections::HashMap, sync::Arc};

use crate::{
    data::{MailContentType, DEMO_ACCOUNTS, DEMO_MAILS},
    mail::{parse::fmt_date, AccountConfig, AttachmentInfo, BodyData, Event, HeaderRow, SyncStatus},
    theme::Rgb,
};

/// Accent colors handed out to accounts in order.
const PALETTE: [Rgb; 6] = [
    (109, 74, 255),
    (0, 150, 136),
    (240, 140, 40),
    (220, 70, 90),
    (80, 160, 235),
    (160, 200, 70),
];

#[derive(Clone)]
pub struct Account {
    pub id: String,
    /// Display name shown in the sidebar (account label).
    pub name: String,
    /// Sender display name — the name recipients see in the From header.
    pub sender_name: String,
    pub addr: String,
    pub host: String,
    pub color: Rgb,
    pub status: SyncStatus,
    /// Fixed "synced X ago" text for demo accounts (they never sync).
    demo_sync: Option<String>,
}

impl Account {
    pub fn sync_label(&self) -> String {
        if let Some(s) = &self.demo_sync {
            return format!("Synced {s}");
        }
        match &self.status {
            SyncStatus::Connecting => "Connecting…".to_string(),
            SyncStatus::Syncing => "Syncing…".to_string(),
            SyncStatus::Idle { at } => format!("Synced {}", ago(*at)),
            SyncStatus::Error(e) => format!("Error: {e}"),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self.status, SyncStatus::Error(_))
    }
}

fn ago(at: i64) -> String {
    let secs = (crate::mail::now() - at).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

#[derive(Clone)]
pub enum BodyState {
    NotLoaded,
    Loading,
    Loaded { content_type: MailContentType, text: Arc<str> },
    Failed(String),
}

#[derive(Clone)]
pub struct Mail {
    pub account: usize,
    pub folder: usize,
    /// IMAP UID within (account, folder). 0 for demo mail.
    pub uid: u32,
    pub unread: bool,
    pub from: String,
    pub from_email: String,
    /// Recipient (populated for Sent/Drafts; empty for Inbox).
    pub to: String,
    pub to_email: String,
    pub subject: String,
    pub preview: String,
    pub body: BodyState,
    /// Attachments found in the message body.
    pub attachments: Vec<AttachmentInfo>,
    pub date: String,
    /// For sorting; higher = newer.
    pub ts: i64,
    /// Tombstone: deleted or moved away. Kept so indices stay stable.
    pub removed: bool,
}

/// What the sidebar footer should say.
pub enum Summary {
    NoAccounts,
    Error(String),
    Syncing,
    Synced,
}

#[derive(Default)]
pub struct Store {
    pub accounts: Vec<Account>,
    pub mails: Vec<Mail>,
    index: HashMap<(usize, usize, u32), usize>,
    pub demo: bool,
    /// Last transient problem (failed move, config write error…), shown in the footer.
    pub notice: Option<String>,
    /// Set by a failed "Add account"; shown on the form.
    pub add_error: Option<String>,
    /// Set when sending fails; shown as a dismissible banner until the user closes it.
    pub send_error: Option<String>,
}

impl Store {
    /// Real accounts from disk. Each starts as "Connecting…" until its worker reports.
    pub fn from_configs(configs: &[AccountConfig]) -> Self {
        let mut store = Store::default();
        for cfg in configs {
            store.push_account(cfg);
        }
        store
    }

    /// The built-in placeholder accounts and mail (`--demo`).
    pub fn demo() -> Self {
        let accounts = DEMO_ACCOUNTS
            .iter()
            .map(|a| Account {
                id: a.addr.to_string(),
                name: a.name.to_string(),
                sender_name: a.name.to_string(),
                addr: a.addr.to_string(),
                host: "demo".to_string(),
                color: a.color,
                status: SyncStatus::Idle { at: 0 },
                demo_sync: Some(a.sync.to_string()),
            })
            .collect();
        let mails = DEMO_MAILS
            .iter()
            .map(|m| Mail {
                account: m.account,
                folder: m.folder,
                uid: 0,
                unread: m.unread,
                from: m.from.to_string(),
                from_email: m.from_email.to_string(),
                to: m.to.to_string(),
                to_email: m.to_email.to_string(),
                subject: m.subject.to_string(),
                preview: m.preview.to_string(),
                body: BodyState::Loaded {
                    content_type: m.content_type,
                    text: Arc::from(m.body),
                },
                attachments: Vec::new(),
                date: m.date.to_string(),
                ts: m.ts as i64,
                removed: false,
            })
            .collect();
        Store { accounts, mails, demo: true, ..Store::default() }
    }

    fn push_account(&mut self, cfg: &AccountConfig) {
        let color = PALETTE[self.accounts.len() % PALETTE.len()];
        self.accounts.push(Account {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            sender_name: cfg.sender_name.clone(),
            addr: cfg.email.clone(),
            host: cfg.host.clone(),
            color,
            status: SyncStatus::Connecting,
            demo_sync: None,
        });
    }

    // -- queries ----------------------------------------------------------------

    pub fn summary(&self) -> Summary {
        if self.accounts.is_empty() {
            return Summary::NoAccounts;
        }
        if let Some(n) = &self.notice {
            return Summary::Error(n.clone());
        }
        if let Some(a) = self.accounts.iter().find(|a| a.is_error()) {
            return Summary::Error(format!("{}: sync problem", a.name));
        }
        let busy = self
            .accounts
            .iter()
            .any(|a| matches!(a.status, SyncStatus::Connecting | SyncStatus::Syncing));
        if busy { Summary::Syncing } else { Summary::Synced }
    }

    // -- mutation ---------------------------------------------------------------

    /// Apply a worker event. Returns true if it added an account.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Status { account, status } => {
                if let Some(a) = self.accounts.get_mut(account) {
                    if matches!(status, SyncStatus::Idle { .. }) {
                        self.notice = None;
                    }
                    a.status = status;
                }
            }
            Event::Headers { account, folder, rows } => {
                for row in rows {
                    self.upsert(account, folder, row);
                }
            }
            Event::Flags { account, folder, updates } => {
                for (uid, unread) in updates {
                    if let Some(&i) = self.index.get(&(account, folder, uid)) {
                        self.mails[i].unread = unread;
                    }
                }
            }
            Event::Removed { account, folder, uids } => {
                for uid in uids {
                    if let Some(&i) = self.index.get(&(account, folder, uid)) {
                        self.mails[i].removed = true;
                    }
                }
            }
            Event::Reset { account, folder } => {
                for m in self.mails.iter_mut() {
                    if m.account == account && m.folder == folder {
                        m.removed = true;
                    }
                }
                self.index.retain(|&(a, f, _), _| !(a == account && f == folder));
            }
            Event::Restore { account, folder, uid } => {
                if let Some(&i) = self.index.get(&(account, folder, uid)) {
                    self.mails[i].removed = false;
                }
            }
            Event::Body { account, folder, uid, result } => {
                if let Some(&i) = self.index.get(&(account, folder, uid)) {
                    let m = &mut self.mails[i];
                    match result {
                        Ok(BodyData { content_type, text, preview, attachments }) => {
                            if !preview.is_empty() {
                                m.preview = preview;
                            }
                            m.body = BodyState::Loaded { content_type, text };
                            m.attachments = attachments;
                        }
                        Err(e) => m.body = BodyState::Failed(e),
                    }
                }
            }
            Event::Notice { text } => self.notice = Some(text),
            Event::AccountAdded { cfg } => {
                self.push_account(&cfg);
                self.add_error = None;
                return true;
            }
            Event::AddAccountFailed { error } => self.add_error = Some(error),
            Event::DraftSaved { .. } => {
                // Handled by the event pump in state.rs, nothing to do in the store.
            }
            Event::EmailSent { .. } => {
                // Cleared in state.rs event pump to show a success notice.
            }
            Event::EmailSendFailed { .. } => {
                // Cleared in state.rs event pump to show an error notice.
            }
        }
        false
    }

    fn upsert(&mut self, account: usize, folder: usize, row: HeaderRow) {
        let key = (account, folder, row.uid);
        if let Some(&i) = self.index.get(&key) {
            let m = &mut self.mails[i];
            m.unread = row.unread;
            m.removed = false;
            return;
        }
        self.mails.push(Mail {
            account,
            folder,
            uid: row.uid,
            unread: row.unread,
            from: row.from,
            from_email: row.from_email,
            to: String::new(),
            to_email: String::new(),
            subject: row.subject,
            preview: String::new(),
            body: BodyState::NotLoaded,
            attachments: Vec::new(),
            date: fmt_date(row.ts),
            ts: row.ts,
            removed: false,
        });
        self.index.insert(key, self.mails.len() - 1);
    }

    /// Drop a message from view right away (archive/trash) without losing its index.
    pub fn tombstone(&mut self, i: usize) {
        self.mails[i].removed = true;
    }
}
