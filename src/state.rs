use std::cmp::Reverse;
use std::io::Cursor;
use std::time::Duration;

use freya::prelude::*;
use notify_rust::Notification as DesktopNotification;
use rodio::{Decoder, OutputStreamBuilder, Sink};

use crate::{
    keyboard::FocusState,
    link_prefs::LinkPrefs,
    mail::{ARCHIVE, Engine, INBOX, NewMailMessage, TRASH, WorkerCmd, config},
    model::{BodyState, NoticeSeverity, Store},
    settings_view::AppSettings,
};

const NOTIFICATION_SOUND: &[u8] = include_bytes!("../assets/sounds/notification-classic.mp3");

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Mail,
    Compose,
    AddAccount,
    Preferences,
    Settings(usize),
    EditAccount(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub struct AppState {
    pub account: State<Option<usize>>, // None = all accounts
    pub folder: State<usize>,
    pub selected: State<Option<usize>>,
    pub store: State<Store>,        // accounts + message headers/bodies
    pub engine: State<Engine>,      // channel to the background IMAP workers
    pub expanded: State<Vec<bool>>, // per-account sidebar group (missing = open)
    pub view: State<View>,
    pub settings: State<AppSettings>,
    pub mark_read_generation: State<u64>,
    pub pending_sends: State<Vec<PendingSend>>,
    pub next_pending_send_id: State<u64>,
    pub unread_only: State<bool>,
    pub sidebar_width: State<f32>,
    pub message_list_width: State<f32>,
    pub focus_state: State<FocusState>,    // keyboard focus state
    pub link_prefs: State<LinkPrefs>,      // "do not show again" link preferences
    pub link_dialog_url: State<String>,    // URL for the link confirmation dialog
    pub link_dialog_sender: State<String>, // Sender email for the link confirmation dialog
    pub link_dialog_open: State<bool>,     // Whether the link confirmation dialog is visible
    pub search_query: State<String>,       // Search query for filtering messages
    pub attachment_preview_mail: State<Option<usize>>, // Mail index of the attachment being previewed
    pub attachment_preview_index: State<usize>,        // Index within the mail's attachments list
    pub attachment_preview_open: State<bool>,          // Whether the preview modal is open
    pub attachment_preview_fullscreen: State<bool>,    // Whether the preview is fullscreen
    pub draft_mail: State<Option<(usize, usize, u32)>>, // (account, folder, uid) of the draft being edited (None = new message)
    /// Pins the compose pane's mount key across saves. `(key, None)` = a save is pending;
    /// `(key, Some(uid))` = the save finished and `uid` is the draft's new UID. While the
    /// pin matches `draft_mail`, the compose pane keeps its key and is NOT remounted.
    pub draft_pin: State<Option<(String, Option<u32>)>>,
    /// Pre-filled compose data for Reply/Reply All/Forward. When Some, the compose pane
    /// uses these values instead of starting with a blank message.
    pub compose_pre: State<Option<ComposePre>>,
    /// Monotonically increasing counter that changes whenever `compose_pre` is set to `Some`.
    /// Used as part of the compose pane's mount key so the pane remounts for new replies/forwards
    /// but stays stable after `.take()` consumes the data.
    pub compose_pre_gen: State<u32>,
    /// Set of message indices that have their quoted text expanded.
    pub quoted_expanded: State<std::collections::HashSet<usize>>,
}

#[derive(Clone)]
pub struct PendingSend {
    pub id: u64,
    pub delay_seconds: u32,
    account: usize,
    raw: String,
    draft_origin: Option<(usize, u32)>,
}

/// Pre-filled compose data for reply and forward actions.
#[derive(Clone, Default)]
pub struct ComposePre {
    pub to: String,
    pub cc: String,
    pub subject: String,
    /// Plain text body pre-filled in the editor (replies, plain-text forwards).
    /// When `html_body` is Some (HTML forward), this field is empty so the editor starts blank.
    pub body: String,
    /// HTML body for forwarded messages. When present, the compose pane sends
    /// a multipart/alternative message with this HTML as the forwarded content.
    pub html_body: Option<String>,
    /// Plain-text version of the forwarded content (for the text/plain part of the MIME message).
    /// Used only when `html_body` is Some, so the plain-text alternative still carries the forwarded text.
    pub forwarded_plain: Option<String>,
    /// Account index to send from (the account that received the original email).
    pub from_account: usize,
    /// Original message's Message-ID (for In-Reply-To/References headers).
    pub original_message_id: String,
    /// Original message's In-Reply-To (space-separated ids, no angle brackets).
    pub original_in_reply_to: String,
    /// The original's ancestors for the References header: its own References chain (or its
    /// In-Reply-To when it has none), space-separated, no angle brackets, oldest first.
    pub original_references: String,
}

pub fn use_app_state() -> AppState {
    let demo = crate::is_demo();

    // Saved accounts are read from disk exactly once.
    let configs = use_hook(move || {
        if demo {
            Vec::new()
        } else {
            config::load_accounts()
        }
    });
    let first_run = !demo && configs.is_empty();
    let settings = use_state(AppSettings::load_from_disk);
    let auto_sync_interval = settings.peek().auto_sync_interval();

    let store_configs = configs.clone();
    let store = use_state(move || {
        if demo {
            Store::demo()
        } else {
            Store::from_configs(&store_configs)
        }
    });
    // Starting the engine spawns the I/O thread, which connects every saved account.
    let engine = use_state(move || {
        if demo {
            Engine::offline()
        } else {
            Engine::start(configs.clone(), auto_sync_interval)
        }
    });

    let app = AppState {
        account: use_state(|| None),
        folder: use_state(|| 0),
        selected: use_state(|| None),
        store,
        engine,
        expanded: use_state(Vec::new),
        // No accounts yet: land on the Add account screen instead of an empty inbox.
        view: use_state(move || {
            if first_run {
                View::AddAccount
            } else {
                View::Mail
            }
        }),
        settings,
        mark_read_generation: use_state(|| 0),
        pending_sends: use_state(Vec::new),
        next_pending_send_id: use_state(|| 0),
        unread_only: use_state(|| false),
        sidebar_width: use_state(|| 260.),
        message_list_width: use_state(|| 380.),
        focus_state: use_state(|| FocusState::default()),
        link_prefs: use_state(|| LinkPrefs::load_from_disk()),
        link_dialog_url: use_state(|| String::new()),
        link_dialog_sender: use_state(|| String::new()),
        link_dialog_open: use_state(|| false),
        search_query: use_state(|| String::new()),
        attachment_preview_mail: use_state(|| None),
        attachment_preview_index: use_state(|| 0),
        attachment_preview_open: use_state(|| false),
        attachment_preview_fullscreen: use_state(|| false),
        draft_mail: use_state(|| None),
        draft_pin: use_state(|| None),
        compose_pre: use_state(|| None),
        compose_pre_gen: use_state(|| 0),
        quoted_expanded: use_state(|| std::collections::HashSet::new()),
    };

    // Drain worker events into the store, once, for the life of the app.
    use_hook(move || start_event_pump(app.clone()));

    app
}

/// Runs on the UI thread as a Freya task: waits for events from the IMAP workers
/// and applies them to the store, which re-renders whatever reads it.
fn start_event_pump(app: AppState) {
    let rx = app.engine.peek().events.clone();
    let is_app_focused = Platform::get().current_window().is_app_focused;
    spawn(async move {
        let mut app = app;
        while let Ok(event) = rx.recv().await {
            if let crate::mail::Event::NewMail { account, messages } = &event {
                let account_id = app
                    .store
                    .peek()
                    .accounts
                    .get(*account)
                    .map(|account| account.id.clone());
                let settings = app.settings.peek().clone();
                if !*is_app_focused.read()
                    && account_id
                        .as_deref()
                        .is_some_and(|id| settings.notifications_for_account(id))
                {
                    let (summary, body) = new_mail_notification(messages);
                    if settings.notification_sound {
                        std::thread::spawn(play_notification_sound);
                    }
                    std::thread::spawn(move || {
                        let _ = DesktopNotification::new()
                            .summary(&summary)
                            .body(&body)
                            .show();
                    });
                }
            }
            // Handle DraftSaved before applying to store. The server gives the saved draft a
            // new UID; track it (also for brand-new drafts) and pin the compose pane's key so
            // the pane keeps its contents instead of remounting.
            if let crate::mail::Event::DraftSaved { account, uid } = &event {
                let pin = app.draft_pin.read().clone();
                if let Some((key, _)) = pin {
                    let in_compose = matches!(*app.view.read(), View::Compose);
                    if in_compose {
                        app.draft_pin.set(Some((key, Some(*uid))));
                        app.draft_mail
                            .set(Some((*account, crate::mail::DRAFTS, *uid)));
                    }
                }
            }
            // Handle email send results: show a notice and close the compose window on success.
            if let crate::mail::Event::EmailSent { account: _ } = &event {
                let revision = {
                    let mut store = app.store.write();
                    store.send_error = None;
                    store.set_notice("Message sent successfully.", NoticeSeverity::Info)
                };
                // Clear the notice after a delay — use futures-timer instead of tokio::time::sleep
                // because Freya's spawn runs on futures-lite, not a Tokio runtime.
                let mut app_clone = app.clone();
                freya::prelude::spawn(async move {
                    futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
                    app_clone.store.write().clear_notice_if_revision(revision);
                });
            }
            if let crate::mail::Event::EmailSendFailed { error, .. } = &event {
                // Keep failures visible in the compose banner until the user dismisses them.
                app.store.write().send_error = Some(format!("Message not sent: {error}"));
            }
            let (added, remap) = {
                let mut store = app.store.write();
                let added = store.apply(event);
                let remap = store.compact_removed();
                (added, remap)
            };
            if let Some(remap) = remap {
                let selected = *app.selected.read();
                app.selected
                    .set(selected.and_then(|selected| remap.get(selected).copied().flatten()));

                let preview = *app.attachment_preview_mail.read();
                if let Some(preview) = preview {
                    if let Some(Some(new_index)) = remap.get(preview) {
                        app.attachment_preview_mail.set(Some(*new_index));
                    } else {
                        app.attachment_preview_mail.set(None);
                        app.attachment_preview_open.set(false);
                    }
                }
            }
            if added {
                // A new account just finished signing in: show its inbox.
                let last = app.store.peek().accounts.len().saturating_sub(1);
                app.account.set(Some(last));
                app.folder.set(INBOX);
                app.set_view(View::Mail);
            }
        }
    });
}

fn play_notification_sound() {
    let Ok(stream) = OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let sink = Sink::connect_new(stream.mixer());
    let Ok(decoder) = Decoder::try_from(Cursor::new(NOTIFICATION_SOUND)) else {
        return;
    };
    sink.append(decoder);
    sink.sleep_until_end();
}

impl AppState {
    /// Mails in the current account + folder, after the unread filter, newest first.
    pub fn visible(&self) -> Vec<usize> {
        let folder = *self.folder.read();
        let unread_only = *self.unread_only.read();
        let account = *self.account.read();
        let store = self.store.read();
        let mut v: Vec<usize> = store
            .mails
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                !m.removed
                    && m.folder == folder
                    && account.map_or(true, |a| m.account == a)
                    && (!unread_only || m.unread)
            })
            .map(|(i, _)| i)
            .collect();
        v.sort_by_key(|&i| Reverse((store.mails[i].ts, store.mails[i].uid)));
        v
    }

    /// Filter visible mails by a search query, checking subject, sender, recipient,
    /// preview text, and loaded body content.
    pub fn search_visible(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.visible();
        }

        let visible = self.visible();
        let store = self.store.read();
        visible
            .into_iter()
            .filter(|&i| {
                let m = &store.mails[i];
                let subject_match = m.subject.to_lowercase().contains(&query);
                let from_match = m.from.to_lowercase().contains(&query)
                    || m.from_email.to_lowercase().contains(&query);
                let to_match = m.to.to_lowercase().contains(&query)
                    || m.to_email.to_lowercase().contains(&query);
                let preview_match = m.preview.to_lowercase().contains(&query);
                let body_match = match &m.body {
                    BodyState::Loaded { text, .. } => text.to_lowercase().contains(&query),
                    _ => false,
                };
                subject_match || from_match || to_match || preview_match || body_match
            })
            .collect()
    }

    /// Return the currently visible mails, filtered by the active search query if any.
    /// Use this instead of `visible()` when the search bar might have text.
    pub fn filtered_visible(&self) -> Vec<usize> {
        let query = self.search_query.peek();
        self.search_visible(&query)
    }

    /// The selected mail, if it's in the current view (respects search filter).
    pub fn open(&self) -> Option<usize> {
        visible_selection(*self.selected.read(), &self.filtered_visible())
    }

    pub fn is_unread(&self, i: usize) -> bool {
        self.store.read().mails.get(i).is_some_and(|m| m.unread)
    }

    /// Unread mails in one folder of one account.
    pub fn account_folder_unread(&self, a: usize, f: usize) -> usize {
        self.store
            .read()
            .mails
            .iter()
            .filter(|m| !m.removed && m.account == a && m.folder == f && m.unread)
            .count()
    }

    /// Unread inbox mails for one account, or all accounts for `None`.
    pub fn inbox_unread(&self, a: Option<usize>) -> usize {
        self.store
            .read()
            .mails
            .iter()
            .filter(|m| {
                !m.removed && m.folder == INBOX && a.map_or(true, |x| m.account == x) && m.unread
            })
            .count()
    }

    pub fn sidebar_account_indices(&self) -> Vec<usize> {
        let account_ids: Vec<String> = self
            .store
            .read()
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .collect();
        self.settings
            .read()
            .ordered_account_ids(&account_ids)
            .iter()
            .filter_map(|id| account_ids.iter().position(|account_id| account_id == id))
            .collect()
    }

    pub fn reorder_sidebar_accounts(&mut self, from_id: &str, to_id: &str) {
        let account_ids: Vec<String> = self
            .store
            .read()
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .collect();
        self.update_settings(|settings| {
            settings.reorder_sidebar_accounts(&account_ids, from_id, to_id);
        });
    }

    pub fn set_account_color(&mut self, index: usize, color: crate::theme::Rgb) {
        if self.store.read().demo {
            if let Some(account) = self.store.write().accounts.get_mut(index) {
                account.color = color;
            }
            return;
        }

        let mut configs = config::load_accounts();
        let Some(cfg) = configs.get_mut(index) else {
            return;
        };
        if cfg.color == Some(color) {
            return;
        }
        cfg.color = Some(color);
        let cfg = cfg.clone();
        if let Some(account) = self.store.write().accounts.get_mut(index) {
            account.color = color;
        }
        self.update_account_config(index, cfg);
    }

    pub fn send(&self, account: usize, cmd: WorkerCmd) {
        self.engine.peek().account(account, cmd);
    }

    pub fn go(&mut self, account: Option<usize>, folder: usize) {
        self.cancel_mark_read_timer();
        self.account.set(account);
        self.folder.set(folder);
        self.set_view(View::Mail);

        // Make sure what's on screen is fresh. Workers skip folders synced in the last
        // few seconds, so this is cheap to call on every navigation.
        let accounts = self.store.peek().accounts.len();
        match account {
            Some(a) => self.send(a, WorkerCmd::SyncFolder { folder }),
            None => (0..accounts).for_each(|a| self.send(a, WorkerCmd::SyncFolder { folder })),
        }
    }

    pub fn set_view(&self, view: View) {
        if *self.view.peek() != view {
            self.cancel_mark_read_timer();
        }
        let mut current_view = self.view.clone();
        current_view.set(view);
    }

    pub fn cancel_mark_read_timer(&self) {
        let mut generation = self.mark_read_generation.clone();
        let mut generation = generation.write();
        *generation = generation.wrapping_add(1);
    }

    /// Select a mail and fetch its body if it isn't already cached. Unread messages
    /// are marked read immediately or after the configured delay.
    pub fn select(&mut self, i: usize) {
        self.set_view(View::Mail);
        self.selected.set(Some(i));
        let mark_read_generation = {
            let mut generation = self.mark_read_generation.write();
            *generation = generation.wrapping_add(1);
            *generation
        };
        let mark_read_after = self.settings.read().mark_read_after_millis;

        let (account, folder, uid, was_unread, needs_body) = {
            let store = self.store.read();
            let Some(m) = store.mails.get(i) else { return };
            (
                m.account,
                m.folder,
                m.uid,
                m.unread,
                matches!(m.body, BodyState::NotLoaded),
            )
        };

        {
            let mut store = self.store.write();
            if needs_body && uid != 0 {
                store.mails[i].body = BodyState::Loading;
            }
        }
        if uid != 0 {
            if needs_body {
                self.send(account, WorkerCmd::FetchBody { folder, uid });
            }
        }
        if was_unread {
            if mark_read_after == 0 {
                self.mark_mail_read(i, account, folder, uid);
            } else {
                let mut state = *self;
                spawn(async move {
                    futures_timer::Delay::new(Duration::from_millis(u64::from(mark_read_after)))
                        .await;
                    if *state.mark_read_generation.read() == mark_read_generation
                        && *state.selected.read() == Some(i)
                        && *state.view.read() == View::Mail
                    {
                        state.mark_mail_read(i, account, folder, uid);
                    }
                });
            }
        }
    }

    fn mark_mail_read(&mut self, index: usize, account: usize, folder: usize, uid: u32) {
        let should_mark = {
            let mut store = self.store.write();
            let Some(mail) = store.mails.get_mut(index) else {
                return;
            };
            if mail.account != account || mail.uid != uid || !mail.unread {
                false
            } else {
                mail.unread = false;
                true
            }
        };
        if should_mark && uid != 0 {
            self.send(
                account,
                WorkerCmd::SetSeen {
                    folder,
                    uid,
                    seen: true,
                },
            );
        }
    }

    pub fn toggle_expanded(&mut self, a: usize) {
        let mut expanded = self.expanded.write();
        if expanded.len() <= a {
            expanded.resize(a + 1, true);
        }
        expanded[a] = !expanded[a];
    }

    pub fn is_expanded(&self, a: usize) -> bool {
        self.expanded.read().get(a).copied().unwrap_or(true)
    }

    pub fn archive(&mut self, i: usize) {
        self.move_mail(i, ARCHIVE);
    }

    pub fn trash(&mut self, i: usize) {
        self.move_mail(i, TRASH);
    }

    fn move_mail(&mut self, i: usize, to: usize) {
        let (account, folder, uid, demo) = {
            let store = self.store.read();
            let Some(m) = store.mails.get(i) else { return };
            (m.account, m.folder, m.uid, store.demo)
        };
        if folder == to {
            return;
        }
        if demo || uid == 0 {
            // Demo mail has no server: just file it locally.
            self.store.write().mails[i].folder = to;
            return;
        }
        // Optimistic: disappear now; the worker moves it and re-syncs both folders
        // (and sends `Restore` if the server refuses).
        self.store.write().tombstone(i);
        self.send(account, WorkerCmd::MoveTo { folder, uid, to });
    }

    pub fn toggle_read(&mut self, i: usize) {
        let (account, folder, uid, now_unread) = {
            let mut store = self.store.write();
            let Some(m) = store.mails.get_mut(i) else {
                return;
            };
            m.unread = !m.unread;
            (m.account, m.folder, m.uid, m.unread)
        };
        if uid != 0 {
            self.send(
                account,
                WorkerCmd::SetSeen {
                    folder,
                    uid,
                    seen: !now_unread,
                },
            );
        }
    }

    /// Sync every folder of one account now (the "Sync now" button).
    pub fn sync_account(&self, account: usize) {
        self.send(account, WorkerCmd::SyncAll);
    }

    /// Sync every folder for every configured account.
    pub fn sync_all_accounts(&self) {
        let accounts = self.store.peek().accounts.len();
        for account in 0..accounts {
            self.sync_account(account);
        }
    }

    pub fn update_settings(&mut self, update: impl FnOnce(&mut AppSettings)) {
        let previous = self.settings.read().clone();
        let updated = {
            let mut settings = self.settings.write();
            update(&mut settings);
            settings.clone()
        };
        if updated == previous {
            return;
        }
        updated.save_to_disk();
        if updated.auto_sync_interval() != previous.auto_sync_interval() {
            self.engine
                .peek()
                .set_auto_sync_interval(updated.auto_sync_interval());
        }
    }

    /// Save a new draft (no existing UID) to the server.
    pub fn save_new_draft(&self, account: usize, raw: String) {
        self.send(account, WorkerCmd::AppendDraft { raw });
    }

    /// Update an existing draft on the server (append the new version, then delete the old).
    pub fn update_draft(&self, account: usize, uid: u32, raw: String) {
        self.send(account, WorkerCmd::UpdateDraft { uid, raw });
    }

    /// Delete a draft from an account's Drafts folder.
    pub fn delete_draft(&self, account: usize, uid: u32) {
        self.send(account, WorkerCmd::DeleteDraft { uid });
    }

    /// Move a draft to a different account: append the new version to the new account and
    /// delete the old one from the old account.
    pub fn move_draft_to_account(
        &self,
        old_account: usize,
        uid: u32,
        new_account: usize,
        raw: String,
    ) {
        self.send(new_account, WorkerCmd::AppendDraft { raw });
        self.delete_draft(old_account, uid);
    }

    /// Switching the sender account of an opened draft: remove the old copy from the account
    /// being left. The next save then creates the draft in the new account.
    pub fn update_draft_move(&self, old_account: usize, uid: u32, _new_account: usize) {
        self.delete_draft(old_account, uid);
    }

    /// Send an email via SMTP. `raw` is the RFC-2822 message. `draft_uid` is the UID of the
    /// draft (in the sending account's Drafts) this message was written from, if any; the
    /// worker deletes it once the message has gone out.
    pub fn send_email(&self, account: usize, raw: String, draft_uid: Option<u32>) {
        self.send(account, WorkerCmd::SendEmail { raw, draft_uid });
    }

    pub fn queue_send_email(
        &mut self,
        account: usize,
        raw: String,
        same_account_draft: Option<u32>,
        other_account_draft: Option<(usize, u32)>,
    ) {
        let delay_seconds = self.settings.read().undo_send_seconds;
        let id = {
            let mut next_id = self.next_pending_send_id.write();
            *next_id = next_id.wrapping_add(1);
            *next_id
        };
        let pending = PendingSend {
            id,
            delay_seconds,
            account,
            raw,
            draft_origin: same_account_draft
                .map(|uid| (account, uid))
                .or(other_account_draft),
        };

        if delay_seconds == 0 {
            self.dispatch_pending_send(pending);
        } else {
            self.pending_sends.write().push(pending);
            let mut state = *self;
            spawn(async move {
                futures_timer::Delay::new(Duration::from_secs(u64::from(delay_seconds))).await;
                state.commit_pending_send(id);
            });
        }
    }

    pub fn undo_pending_send(&mut self, id: u64) {
        let pending = {
            let mut pending_sends = self.pending_sends.write();
            pending_sends
                .iter()
                .position(|pending| pending.id == id)
                .map(|index| pending_sends.remove(index))
        };
        let Some(pending) = pending else {
            return;
        };

        if let Some((account, uid)) = pending.draft_origin {
            self.update_draft(account, uid, pending.raw);
        } else {
            self.save_new_draft(pending.account, pending.raw);
        }
        self.store.write().set_notice(
            "Send canceled; message saved to Drafts.",
            NoticeSeverity::Info,
        );
    }

    fn commit_pending_send(&mut self, id: u64) {
        let pending = {
            let mut pending_sends = self.pending_sends.write();
            pending_sends
                .iter()
                .position(|pending| pending.id == id)
                .map(|index| pending_sends.remove(index))
        };
        if let Some(pending) = pending {
            self.dispatch_pending_send(pending);
        }
    }

    fn dispatch_pending_send(&self, pending: PendingSend) {
        let draft_uid = pending.draft_origin.and_then(|(draft_account, uid)| {
            if draft_account == pending.account {
                Some(uid)
            } else {
                self.delete_draft(draft_account, uid);
                None
            }
        });
        self.send_email(pending.account, pending.raw, draft_uid);
    }

    /// Update an account's persisted config (e.g. SMTP settings) and save to disk.
    pub fn update_account_config(&self, index: usize, cfg: crate::mail::AccountConfig) {
        self.engine
            .peek()
            .send(crate::mail::Command::UpdateAccount { index, cfg });
    }
}

fn new_mail_notification(messages: &[NewMailMessage]) -> (String, String) {
    let Some(first) = messages.first() else {
        return ("New mail".to_string(), "You have new mail.".to_string());
    };

    let from = if first.from.trim().is_empty() {
        "Unknown sender"
    } else {
        first.from.trim()
    };
    let subject = if first.subject.trim().is_empty() {
        "(no subject)"
    } else {
        first.subject.trim()
    };

    if messages.len() == 1 {
        (format!("New email from {from}"), subject.to_string())
    } else {
        (
            format!("{} new emails", messages.len()),
            format!("{from}: {subject}\nand {} more", messages.len() - 1),
        )
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn single_message_notification_shows_sender_and_subject() {
        let (summary, body) = new_mail_notification(&[NewMailMessage {
            from: "Alice".to_string(),
            subject: "Project update".to_string(),
        }]);

        assert_eq!(summary, "New email from Alice");
        assert_eq!(body, "Project update");
    }

    #[test]
    fn grouped_notification_reports_the_number_of_remaining_messages() {
        let messages = vec![
            NewMailMessage {
                from: "Alice".to_string(),
                subject: "Project update".to_string(),
            },
            NewMailMessage {
                from: "Bob".to_string(),
                subject: "Schedule".to_string(),
            },
        ];
        let (summary, body) = new_mail_notification(&messages);

        assert_eq!(summary, "2 new emails");
        assert_eq!(body, "Alice: Project update\nand 1 more");
    }
}

fn visible_selection(selected: Option<usize>, visible: &[usize]) -> Option<usize> {
    selected.filter(|selected| visible.contains(selected))
}

#[cfg(test)]
mod tests {
    use super::visible_selection;

    #[test]
    fn no_selection_does_not_open_first_visible_message() {
        assert_eq!(visible_selection(None, &[0, 1]), None);
    }

    #[test]
    fn selection_opens_only_when_message_is_visible() {
        assert_eq!(visible_selection(Some(1), &[0, 1]), Some(1));
        assert_eq!(visible_selection(Some(1), &[0]), None);
    }
}
