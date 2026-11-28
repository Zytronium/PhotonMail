//! UI-side mail model. One `Store` holds every account and every message header we
//! know about; `AppState` keeps it in a single Freya `State` and the list/reader
//! components read from it. Removed messages are tombstoned until periodic compaction,
//! which returns an index map so `AppState::selected` remains valid.

use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    cache::{CachedAttachment, CachedBody, CachedEmail, EmailCache},
    data::{DEMO_ACCOUNTS, DEMO_MAILS, MailContentType},
    mail::{
        AccountConfig, AttachmentInfo, BodyData, Event, HeaderRow, SyncStatus, parse::fmt_date,
    },
    theme::{ACCOUNT_COLORS, Rgb},
};

const MAX_CACHED_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_MAIL_RECORDS: usize = 1_200;
const MAX_PERSISTED_BODY_BYTES: usize = 8 * 1024 * 1024;

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
    Loaded {
        content_type: MailContentType,
        text: Arc<str>,
        source: Arc<[u8]>,
    },
    Failed(String),
}

#[derive(Clone)]
pub struct Mail {
    pub account: usize,
    pub folder: usize,
    /// IMAP UID within (account, folder). 0 for demo mail.
    pub uid: u32,
    pub unread: bool,
    pub starred: bool,
    pub from: String,
    pub from_email: String,
    /// Recipient (populated for Sent/Drafts; empty for Inbox).
    pub to: String,
    /// All To recipients, comma-separated bare addresses (empty for demo mail without any).
    pub to_email: String,
    /// All Cc / Bcc recipients, comma-separated bare addresses.
    pub cc_email: String,
    pub bcc_email: String,
    /// Reply-To addresses, comma-separated; empty when the sender set none.
    pub reply_to: String,
    pub subject: String,
    pub preview: String,
    pub body: BodyState,
    /// Attachments found in the message body.
    pub attachments: Vec<AttachmentInfo>,
    pub date: String,
    /// For sorting; higher = newer.
    pub ts: i64,
    /// RFC 5322 Message-ID (without angle brackets). Used for In-Reply-To/References.
    pub message_id: String,
    /// RFC 5322 In-Reply-To: space-separated ids, without angle brackets.
    pub in_reply_to: String,
    /// Ids from the References header (without angle brackets), oldest first. Known once the
    /// body has been loaded.
    pub references: Vec<String>,
    /// Tombstone: deleted or moved away. Kept so indices stay stable.
    pub removed: bool,
    body_cache_stamp: u64,
}

/// What the sidebar footer should say.
pub enum Summary {
    NoAccounts,
    Error(String),
    Notice(String),
    Syncing,
    Synced,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NoticeSeverity {
    Info,
    Error,
}

#[derive(Default)]
pub struct Store {
    pub accounts: Vec<Account>,
    pub mails: Vec<Mail>,
    index: HashMap<(usize, usize, u32), usize>,
    pub demo: bool,
    /// Current informational or error notice shown in the footer.
    notice: Option<(String, NoticeSeverity, u64)>,
    notice_revision: u64,
    /// Set by a failed "Add account"; shown on the form.
    pub add_error: Option<String>,
    /// Set when sending fails; shown as a dismissible banner until the user closes it.
    pub send_error: Option<String>,
    body_cache_clock: u64,
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
                starred: false,
                from: m.from.to_string(),
                from_email: m.from_email.to_string(),
                to: m.to.to_string(),
                to_email: m.to_email.to_string(),
                cc_email: String::new(),
                bcc_email: String::new(),
                reply_to: String::new(),
                subject: m.subject.to_string(),
                preview: m.preview.to_string(),
                body: BodyState::Loaded {
                    content_type: m.content_type,
                    text: Arc::from(m.body),
                    source: Arc::from(&b""[..]),
                },
                attachments: Vec::new(),
                date: m.date.to_string(),
                ts: m.ts as i64,
                message_id: String::new(),
                in_reply_to: String::new(),
                references: Vec::new(),
                removed: false,
                body_cache_stamp: 0,
            })
            .collect();
        Store {
            accounts,
            mails,
            demo: true,
            ..Store::default()
        }
    }

    fn push_account(&mut self, cfg: &AccountConfig) {
        let color = cfg
            .color
            .unwrap_or(ACCOUNT_COLORS[self.accounts.len() % ACCOUNT_COLORS.len()]);
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
        if let Some((text, severity, _)) = &self.notice {
            return match severity {
                NoticeSeverity::Info => Summary::Notice(text.clone()),
                NoticeSeverity::Error => Summary::Error(text.clone()),
            };
        }
        if let Some(a) = self.accounts.iter().find(|a| a.is_error()) {
            return Summary::Error(format!("{}: sync problem", a.name));
        }
        let busy = self
            .accounts
            .iter()
            .any(|a| matches!(a.status, SyncStatus::Connecting | SyncStatus::Syncing));
        if busy {
            Summary::Syncing
        } else {
            Summary::Synced
        }
    }

    pub fn set_notice(&mut self, text: impl Into<String>, severity: NoticeSeverity) -> u64 {
        self.notice_revision = self.notice_revision.wrapping_add(1);
        self.notice = Some((text.into(), severity, self.notice_revision));
        self.notice_revision
    }

    pub fn clear_notice_if_revision(&mut self, revision: u64) {
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, _, current)| *current == revision)
        {
            self.notice = None;
        }
    }

    // -- mutation ---------------------------------------------------------------

    /// Apply a worker event. Returns true if it added an account.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Status { account, status } => {
                if let Some(a) = self.accounts.get_mut(account) {
                    a.status = status;
                }
            }
            Event::Headers {
                account,
                folder,
                rows,
            } => {
                for row in rows {
                    self.upsert(account, folder, row);
                }
            }
            Event::FolderSynced {
                account,
                folder,
                uids,
            } => {
                let present: HashSet<u32> = uids.into_iter().collect();
                let removed: Vec<usize> = self
                    .mails
                    .iter()
                    .enumerate()
                    .filter(|(_, mail)| {
                        mail.account == account
                            && mail.folder == folder
                            && mail.uid != 0
                            && !present.contains(&mail.uid)
                    })
                    .map(|(index, _)| index)
                    .collect();
                for index in removed {
                    self.remove_payload(index);
                }
                self.index.retain(|&(a, f, uid), _| {
                    a != account || f != folder || present.contains(&uid)
                });
            }
            Event::Flags {
                account,
                folder,
                updates,
            } => {
                for (uid, unread, starred) in updates {
                    if let Some(&i) = self.index.get(&(account, folder, uid)) {
                        self.mails[i].unread = unread;
                        self.mails[i].starred = starred;
                    }
                }
            }
            Event::Removed {
                account,
                folder,
                uids,
            } => {
                for uid in uids {
                    if let Some(&i) = self.index.get(&(account, folder, uid)) {
                        self.remove_payload(i);
                    }
                }
            }
            Event::Reset { account, folder } => {
                for m in self.mails.iter_mut() {
                    if m.account == account && m.folder == folder {
                        m.body = BodyState::NotLoaded;
                        m.attachments.clear();
                        m.references.clear();
                        m.body_cache_stamp = 0;
                    }
                }
            }
            Event::Restore {
                account,
                folder,
                uid,
            } => {
                if let Some(&i) = self.index.get(&(account, folder, uid)) {
                    self.mails[i].removed = false;
                }
            }
            Event::Body {
                account,
                folder,
                uid,
                result,
            } => {
                if let Some(&i) = self.index.get(&(account, folder, uid)) {
                    match result {
                        Ok(BodyData {
                            content_type,
                            text,
                            source,
                            preview,
                            attachments,
                            references,
                        }) => {
                            self.body_cache_clock = self.body_cache_clock.wrapping_add(1);
                            let stamp = self.body_cache_clock;
                            let m = &mut self.mails[i];
                            if !preview.is_empty() {
                                m.preview = preview;
                            }
                            m.body = BodyState::Loaded {
                                content_type,
                                text,
                                source,
                            };
                            m.attachments = attachments;
                            m.references = references;
                            m.body_cache_stamp = stamp;
                            self.enforce_body_cache_budget();
                        }
                        Err(e) => self.mails[i].body = BodyState::Failed(e),
                    }
                }
            }
            Event::Notice { text } => {
                self.set_notice(text, NoticeSeverity::Error);
            }
            Event::AccountAdded { cfg } => {
                self.push_account(&cfg);
                self.add_error = None;
                return true;
            }
            Event::AccountRemoved { index } => {
                if index >= self.accounts.len() {
                    return false;
                }
                self.accounts.remove(index);
                for mail_index in 0..self.mails.len() {
                    if self.mails[mail_index].account == index {
                        self.remove_payload(mail_index);
                    } else if self.mails[mail_index].account > index {
                        self.mails[mail_index].account -= 1;
                    }
                }
                self.index.clear();
                for (mail_index, mail) in self.mails.iter().enumerate() {
                    if !mail.removed && mail.uid != 0 {
                        self.index
                            .insert((mail.account, mail.folder, mail.uid), mail_index);
                    }
                }
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
            Event::NewMail { .. } => {
                // Handled by the event pump in state.rs to show a desktop notification.
            }
        }
        false
    }

    fn upsert(&mut self, account: usize, folder: usize, row: HeaderRow) {
        let key = (account, folder, row.uid);
        if let Some(&i) = self.index.get(&key) {
            let to = if matches!(folder, crate::mail::DRAFTS | crate::mail::SENT) {
                row.from
                    .strip_prefix("To: ")
                    .unwrap_or(&row.from)
                    .to_string()
            } else {
                String::new()
            };
            let m = &mut self.mails[i];
            m.unread = row.unread;
            m.starred = row.starred;
            m.removed = false;
            m.from = row.from;
            m.from_email = row.from_email;
            m.to = to;
            m.to_email = row.to_emails.join(", ");
            m.cc_email = row.cc_emails.join(", ");
            m.bcc_email = row.bcc_emails.join(", ");
            m.reply_to = row.reply_to;
            m.subject = row.subject;
            m.date = fmt_date(row.ts);
            m.ts = row.ts;
            m.message_id = row.message_id;
            m.in_reply_to = row.in_reply_to;
            return;
        }
        let to = if matches!(folder, crate::mail::DRAFTS | crate::mail::SENT) {
            row.from
                .strip_prefix("To: ")
                .unwrap_or(&row.from)
                .to_string()
        } else {
            String::new()
        };
        self.mails.push(Mail {
            account,
            folder,
            uid: row.uid,
            unread: row.unread,
            starred: row.starred,
            from: row.from,
            from_email: row.from_email,
            to,
            to_email: row.to_emails.join(", "),
            cc_email: row.cc_emails.join(", "),
            bcc_email: row.bcc_emails.join(", "),
            reply_to: row.reply_to,
            subject: row.subject,
            preview: String::new(),
            body: BodyState::NotLoaded,
            attachments: Vec::new(),
            date: fmt_date(row.ts),
            ts: row.ts,
            message_id: row.message_id,
            in_reply_to: row.in_reply_to,
            references: Vec::new(),
            removed: false,
            body_cache_stamp: 0,
        });
        self.index.insert(key, self.mails.len() - 1);
    }

    pub fn restore_cache(
        &mut self,
        cache: EmailCache,
        synced_folders: &HashSet<(usize, usize)>,
        reset_folders: &HashSet<(usize, usize)>,
    ) {
        let account_indices: HashMap<&str, usize> = self
            .accounts
            .iter()
            .enumerate()
            .map(|(index, account)| (account.id.as_str(), index))
            .collect();
        for cached in cache.emails {
            let Some(&account) = account_indices.get(cached.account_id.as_str()) else {
                continue;
            };
            let folder_key = (account, cached.folder);
            if cached.uid == 0
                || cached.folder >= crate::data::FOLDERS.len()
                || synced_folders.contains(&folder_key)
                || reset_folders.contains(&folder_key)
            {
                continue;
            }
            let key = (account, cached.folder, cached.uid);
            if self.index.contains_key(&key) {
                continue;
            }
            let (body, attachments) = match cached.body {
                Some(body) => {
                    self.body_cache_clock = self.body_cache_clock.wrapping_add(1);
                    let attachments = body
                        .attachments
                        .into_iter()
                        .map(|attachment| AttachmentInfo {
                            name: attachment.name,
                            size: attachment.size,
                            content_type: attachment.content_type,
                            data: attachment.data,
                            content_id: attachment.content_id,
                        })
                        .collect();
                    (
                        BodyState::Loaded {
                            content_type: body.content_type,
                            text: Arc::from(body.text),
                            source: Arc::from(body.source),
                        },
                        attachments,
                    )
                }
                None => (BodyState::NotLoaded, Vec::new()),
            };
            self.mails.push(Mail {
                account,
                folder: cached.folder,
                uid: cached.uid,
                unread: cached.unread,
                starred: cached.starred,
                from: cached.from,
                from_email: cached.from_email,
                to: cached.to,
                to_email: cached.to_email,
                cc_email: cached.cc_email,
                bcc_email: cached.bcc_email,
                reply_to: cached.reply_to,
                subject: cached.subject,
                preview: cached.preview,
                body,
                attachments,
                date: cached.date,
                ts: cached.ts,
                message_id: cached.message_id,
                in_reply_to: cached.in_reply_to,
                references: cached.references,
                removed: false,
                body_cache_stamp: self.body_cache_clock,
            });
            self.index.insert(key, self.mails.len() - 1);
        }
        self.enforce_body_cache_budget();
    }

    pub fn cache_snapshot(&self) -> EmailCache {
        let mut order: Vec<usize> = self
            .mails
            .iter()
            .enumerate()
            .filter(|(_, mail)| !mail.removed && mail.uid != 0)
            .map(|(index, _)| index)
            .collect();
        order.sort_by_key(|&index| Reverse(self.mails[index].ts));

        let mut body_bytes_left = MAX_PERSISTED_BODY_BYTES;
        let emails = order
            .into_iter()
            .filter_map(|index| {
                let mail = &self.mails[index];
                let account_id = self.accounts.get(mail.account)?.id.clone();
                let body = match &mail.body {
                    BodyState::Loaded {
                        content_type,
                        text,
                        source,
                    } if text.len() <= body_bytes_left => {
                        body_bytes_left -= text.len();
                        let source = if source.len() <= body_bytes_left {
                            body_bytes_left -= source.len();
                            source.to_vec()
                        } else {
                            Vec::new()
                        };
                        let attachments = mail
                            .attachments
                            .iter()
                            .map(|attachment| {
                                let data = attachment.data.as_ref().and_then(|data| {
                                    if data.len() <= body_bytes_left {
                                        body_bytes_left -= data.len();
                                        Some(data.clone())
                                    } else {
                                        None
                                    }
                                });
                                CachedAttachment {
                                    name: attachment.name.clone(),
                                    size: attachment.size,
                                    content_type: attachment.content_type.clone(),
                                    data,
                                    content_id: attachment.content_id.clone(),
                                }
                            })
                            .collect();
                        Some(CachedBody {
                            content_type: *content_type,
                            text: text.to_string(),
                            source,
                            attachments,
                        })
                    }
                    _ => None,
                };
                Some(CachedEmail {
                    account_id,
                    folder: mail.folder,
                    uid: mail.uid,
                    unread: mail.unread,
                    starred: mail.starred,
                    from: mail.from.clone(),
                    from_email: mail.from_email.clone(),
                    to: mail.to.clone(),
                    to_email: mail.to_email.clone(),
                    cc_email: mail.cc_email.clone(),
                    bcc_email: mail.bcc_email.clone(),
                    reply_to: mail.reply_to.clone(),
                    subject: mail.subject.clone(),
                    preview: mail.preview.clone(),
                    body,
                    date: mail.date.clone(),
                    ts: mail.ts,
                    message_id: mail.message_id.clone(),
                    in_reply_to: mail.in_reply_to.clone(),
                    references: mail.references.clone(),
                })
            })
            .collect();
        EmailCache::new(emails)
    }

    /// Drop a message from view right away (archive/trash) without losing its index.
    pub fn tombstone(&mut self, i: usize) {
        self.remove_payload(i);
    }

    /// Compact removed records once the append-only vector crosses a bounded threshold.
    /// Returns old-index → new-index mappings so UI selection can remain coherent.
    pub fn compact_removed(&mut self) -> Option<Vec<Option<usize>>> {
        if self.mails.len() <= MAX_MAIL_RECORDS || !self.mails.iter().any(|mail| mail.removed) {
            return None;
        }

        let mut remap = vec![None; self.mails.len()];
        let mut kept = Vec::with_capacity(self.mails.len());
        for (old_index, mail) in self.mails.drain(..).enumerate() {
            if !mail.removed {
                remap[old_index] = Some(kept.len());
                kept.push(mail);
            }
        }
        self.mails = kept;
        self.index.clear();
        for (index, mail) in self.mails.iter().enumerate() {
            if mail.uid != 0 {
                self.index
                    .insert((mail.account, mail.folder, mail.uid), index);
            }
        }
        Some(remap)
    }

    fn remove_payload(&mut self, index: usize) {
        let mail = &mut self.mails[index];
        mail.removed = true;
        mail.body = BodyState::NotLoaded;
        mail.attachments.clear();
        mail.references.clear();
        mail.body_cache_stamp = 0;
    }

    fn enforce_body_cache_budget(&mut self) {
        self.evict_body_cache_to_budget(MAX_CACHED_BODY_BYTES);
    }

    fn evict_body_cache_to_budget(&mut self, budget: usize) {
        while self.cached_body_bytes() > budget {
            let oldest = self
                .mails
                .iter()
                .enumerate()
                .filter(|(_, mail)| matches!(&mail.body, BodyState::Loaded { .. }))
                .min_by_key(|(_, mail)| mail.body_cache_stamp)
                .map(|(index, _)| index);
            let Some(index) = oldest else { break };
            self.mails[index].body = BodyState::NotLoaded;
            self.mails[index].attachments.clear();
            self.mails[index].body_cache_stamp = 0;
        }
    }

    fn cached_body_bytes(&self) -> usize {
        self.mails
            .iter()
            .filter_map(|mail| match &mail.body {
                BodyState::Loaded { text, source, .. } => Some(
                    text.len()
                        + source.len()
                        + mail
                            .attachments
                            .iter()
                            .filter_map(|attachment| attachment.data.as_ref())
                            .map(Vec::len)
                            .sum::<usize>(),
                ),
                _ => None,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(uid: u32) -> HeaderRow {
        HeaderRow {
            uid,
            unread: false,
            starred: false,
            from: "Sender".to_string(),
            from_email: "sender@example.com".to_string(),
            subject: format!("Message {uid}"),
            ts: uid as i64,
            size: 0,
            message_id: String::new(),
            in_reply_to: String::new(),
            to_emails: Vec::new(),
            cc_emails: Vec::new(),
            bcc_emails: Vec::new(),
            reply_to: String::new(),
        }
    }

    fn body(text: &str) -> BodyData {
        BodyData {
            content_type: MailContentType::PlainText,
            text: Arc::from(text),
            source: Arc::from(&b""[..]),
            preview: String::new(),
            attachments: Vec::new(),
            references: Vec::new(),
        }
    }

    fn store_with_idle_account() -> Store {
        let mut store = Store::default();
        store.accounts.push(Account {
            id: "account".to_string(),
            name: "Account".to_string(),
            sender_name: String::new(),
            addr: "user@example.test".to_string(),
            host: "mail.example.test".to_string(),
            color: ACCOUNT_COLORS[0],
            status: SyncStatus::Idle { at: 0 },
            demo_sync: None,
        });
        store
    }

    #[test]
    fn success_notices_are_informational_and_idle_does_not_clear_notices() {
        let mut store = store_with_idle_account();
        let revision = store.set_notice("Message sent successfully.", NoticeSeverity::Info);

        store.apply(Event::Status {
            account: 0,
            status: SyncStatus::Idle { at: 1 },
        });

        assert!(
            matches!(store.summary(), Summary::Notice(message) if message == "Message sent successfully.")
        );
        store.clear_notice_if_revision(revision);
        assert!(matches!(store.summary(), Summary::Synced));
    }

    #[test]
    fn sent_headers_preserve_recipient_name_and_all_recipient_emails() {
        let mut store = Store::default();
        let mut sent = row(1);
        sent.from = "To: Alice Example".to_string();
        sent.from_email = "alice@example.test".to_string();
        sent.to_emails = vec![
            "alice@example.test".to_string(),
            "bob@example.test".to_string(),
        ];

        store.upsert(0, crate::mail::SENT, sent);

        assert_eq!(store.mails[0].to, "Alice Example");
        assert_eq!(
            store.mails[0].to_email,
            "alice@example.test, bob@example.test"
        );
    }

    #[test]
    fn cached_mail_restores_headers_and_loaded_body() {
        let mut source = store_with_idle_account();
        let mut header = row(5);
        header.starred = true;
        source.upsert(0, 0, header);
        let mut cached_body = body("cached body");
        cached_body.source = Arc::from(&b"raw message source"[..]);
        cached_body.attachments.push(AttachmentInfo {
            name: "file.txt".to_string(),
            size: 4,
            content_type: "text/plain".to_string(),
            data: Some(b"data".to_vec()),
            content_id: None,
        });
        source.apply(Event::Body {
            account: 0,
            folder: 0,
            uid: 5,
            result: Ok(cached_body),
        });
        let cache = source.cache_snapshot();

        let mut restored = store_with_idle_account();
        restored.restore_cache(cache, &HashSet::new(), &HashSet::new());

        assert_eq!(restored.mails.len(), 1);
        assert_eq!(restored.mails[0].subject, "Message 5");
        assert!(restored.mails[0].starred);
        assert!(matches!(
            &restored.mails[0].body,
            BodyState::Loaded { text, .. } if text.as_ref() == "cached body"
        ));
        let BodyState::Loaded { source, .. } = &restored.mails[0].body else {
            panic!("cached message body was not restored");
        };
        assert_eq!(source.as_ref(), b"raw message source");
        assert_eq!(
            restored.mails[0].attachments[0].data.as_deref(),
            Some(&b"data"[..])
        );
    }

    #[test]
    fn flag_updates_apply_read_and_starred_state_independently() {
        let mut store = store_with_idle_account();
        store.upsert(0, 0, row(5));

        store.apply(Event::Flags {
            account: 0,
            folder: 0,
            updates: vec![(5, true, true)],
        });

        assert!(store.mails[0].unread);
        assert!(store.mails[0].starred);
    }

    #[test]
    fn completed_folder_sync_removes_only_cached_uids_missing_from_server() {
        let mut store = store_with_idle_account();
        store.upsert(0, 0, row(5));
        store.upsert(0, 0, row(6));
        store.apply(Event::FolderSynced {
            account: 0,
            folder: 0,
            uids: vec![6],
        });

        assert!(store.mails[0].removed);
        assert!(!store.mails[1].removed);
        assert_eq!(store.cache_snapshot().emails.len(), 1);
    }

    #[test]
    fn folder_reset_keeps_cached_rows_visible_until_sync_completes() {
        let mut store = store_with_idle_account();
        store.upsert(0, 0, row(5));
        store.apply(Event::Reset {
            account: 0,
            folder: 0,
        });

        assert!(!store.mails[0].removed);
        assert!(matches!(store.mails[0].body, BodyState::NotLoaded));
    }

    #[test]
    fn stale_notice_timer_cannot_clear_a_newer_error() {
        let mut store = store_with_idle_account();
        let old_revision = store.set_notice("Message sent successfully.", NoticeSeverity::Info);
        store.set_notice(
            "Sent, but could not save to Sent folder.",
            NoticeSeverity::Error,
        );

        store.clear_notice_if_revision(old_revision);

        assert!(matches!(
            store.summary(),
            Summary::Error(message) if message == "Sent, but could not save to Sent folder."
        ));
    }

    #[test]
    fn compaction_remaps_indices_and_rebuilds_uid_index() {
        let mut store = Store::default();
        for uid in 1..=(MAX_MAIL_RECORDS as u32 + 1) {
            store.upsert(0, 0, row(uid));
        }
        store.tombstone(0);

        let remap = store.compact_removed().expect("over-limit records compact");
        assert_eq!(remap[0], None);
        assert_eq!(remap[1], Some(0));
        assert_eq!(store.mails.len(), MAX_MAIL_RECORDS);
        assert_eq!(store.index.get(&(0, 0, 2)), Some(&0));
    }

    #[test]
    fn body_cache_evicts_oldest_loaded_body_and_tombstones_release_payloads() {
        let mut store = Store::default();
        store.upsert(0, 0, row(1));
        store.upsert(0, 0, row(2));
        for uid in [1, 2] {
            store.apply(Event::Body {
                account: 0,
                folder: 0,
                uid,
                result: Ok(body("four")),
            });
        }
        store.evict_body_cache_to_budget(4);
        assert!(matches!(store.mails[0].body, BodyState::NotLoaded));
        assert!(matches!(store.mails[1].body, BodyState::Loaded { .. }));

        store.tombstone(1);
        assert!(matches!(store.mails[1].body, BodyState::NotLoaded));
        assert!(store.mails[1].attachments.is_empty());
    }

    #[test]
    fn removing_an_account_drops_its_mail_and_reindexes_remaining_mail() {
        let mut store = store_with_idle_account();
        let mut second_account = store.accounts[0].clone();
        second_account.id = "second-account".to_string();
        second_account.addr = "second@example.test".to_string();
        store.accounts.push(second_account);
        store.upsert(0, 0, row(5));
        store.upsert(1, 0, row(6));

        store.apply(Event::AccountRemoved { index: 0 });

        assert_eq!(store.accounts.len(), 1);
        assert_eq!(store.accounts[0].id, "second-account");
        assert!(store.mails[0].removed);
        assert_eq!(store.mails[1].account, 0);
        assert_eq!(store.index.get(&(0, 0, 6)), Some(&1));
        store.upsert(0, 0, row(6));
        assert_eq!(store.mails.len(), 2);
    }
}
