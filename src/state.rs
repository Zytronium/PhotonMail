use std::cmp::Reverse;

use freya::prelude::*;

use crate::{
    keyboard::FocusState,
    link_prefs::LinkPrefs,
    mail::{config, Engine, WorkerCmd, ARCHIVE, INBOX, TRASH},
    model::{BodyState, Store},
};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Mail,
    Compose,
    AddAccount,
    Settings(usize),
    EditAccount(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub struct AppState {
    pub account: State<Option<usize>>, // None = all accounts
    pub folder: State<usize>,
    pub selected: State<usize>,
    pub store: State<Store>,   // accounts + message headers/bodies
    pub engine: State<Engine>, // channel to the background IMAP workers
    pub expanded: State<Vec<bool>>, // per-account sidebar group (missing = open)
    pub view: State<View>,
    pub unread_only: State<bool>,
    pub sidebar_width: State<f32>,
    pub message_list_width: State<f32>,
    pub focus_state: State<FocusState>, // keyboard focus state
    pub link_prefs: State<LinkPrefs>,   // "do not show again" link preferences
    pub link_dialog_url: State<String>,  // URL for the link confirmation dialog
    pub link_dialog_sender: State<String>, // Sender email for the link confirmation dialog
    pub link_dialog_open: State<bool>,    // Whether the link confirmation dialog is visible
    pub search_query: State<String>,     // Search query for filtering messages
    pub attachment_preview_mail: State<Option<usize>>,  // Mail index of the attachment being previewed
    pub attachment_preview_index: State<usize>,         // Index within the mail's attachments list
    pub attachment_preview_open: State<bool>,           // Whether the preview modal is open
    pub attachment_preview_fullscreen: State<bool>,     // Whether the preview is fullscreen
    pub draft_mail: State<Option<(usize, usize, u32)>>, // (account, folder, uid) of the draft being edited (None = new message)
    /// Pins the compose pane's mount key across saves. `(key, None)` = a save is pending;
    /// `(key, Some(uid))` = the save finished and `uid` is the draft's new UID. While the
    /// pin matches `draft_mail`, the compose pane keeps its key and is NOT remounted.
    pub draft_pin: State<Option<(String, Option<u32>)>>,
}

pub fn use_app_state() -> AppState {
    let demo = crate::is_demo();

    // Saved accounts are read from disk exactly once.
    let configs = use_hook(move || if demo { Vec::new() } else { config::load_accounts() });
    let first_run = !demo && configs.is_empty();

    let store_configs = configs.clone();
    let store = use_state(move || {
        if demo { Store::demo() } else { Store::from_configs(&store_configs) }
    });
    // Starting the engine spawns the I/O thread, which connects every saved account.
    let engine = use_state(move || {
        if demo { Engine::offline() } else { Engine::start(configs.clone()) }
    });

    let app = AppState {
        account: use_state(|| None),
        folder: use_state(|| 0),
        selected: use_state(|| 0),
        store,
        engine,
        expanded: use_state(Vec::new),
        // No accounts yet: land on the Add account screen instead of an empty inbox.
        view: use_state(move || if first_run { View::AddAccount } else { View::Mail }),
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
    };

    // Drain worker events into the store, once, for the life of the app.
    use_hook(move || start_event_pump(app.clone()));

    app
}

/// Runs on the UI thread as a Freya task: waits for events from the IMAP workers
/// and applies them to the store, which re-renders whatever reads it.
fn start_event_pump(app: AppState) {
    let rx = app.engine.peek().events.clone();
    spawn(async move {
        let mut app = app;
        while let Ok(event) = rx.recv().await {
            // Handle DraftSaved before applying to store. The server gives the saved draft a
            // new UID; track it (also for brand-new drafts) and pin the compose pane's key so
            // the pane keeps its contents instead of remounting.
            if let crate::mail::Event::DraftSaved { account, uid } = &event {
                let pin = app.draft_pin.read().clone();
                if let Some((key, _)) = pin {
                    let in_compose = matches!(*app.view.read(), View::Compose);
                    let current = *app.draft_mail.read();
                    if in_compose && current.map_or(true, |(a, _, _)| a == *account) {
                        app.draft_pin.set(Some((key, Some(*uid))));
                        app.draft_mail.set(Some((*account, crate::mail::DRAFTS, *uid)));
                    }
                }
            }
            let added = app.store.write().apply(event);
            if added {
                // A new account just finished signing in: show its inbox.
                let last = app.store.peek().accounts.len().saturating_sub(1);
                app.account.set(Some(last));
                app.folder.set(INBOX);
                app.view.set(View::Mail);
            }
        }
    });
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
        let sel = *self.selected.read();
        self.filtered_visible().contains(&sel).then_some(sel)
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

    pub fn send(&self, account: usize, cmd: WorkerCmd) {
        self.engine.peek().account(account, cmd);
    }

    pub fn go(&mut self, account: Option<usize>, folder: usize) {
        self.account.set(account);
        self.folder.set(folder);
        self.view.set(View::Mail);

        // Make sure what's on screen is fresh. Workers skip folders synced in the last
        // few seconds, so this is cheap to call on every navigation.
        let accounts = self.store.peek().accounts.len();
        match account {
            Some(a) => self.send(a, WorkerCmd::SyncFolder { folder }),
            None => (0..accounts).for_each(|a| self.send(a, WorkerCmd::SyncFolder { folder })),
        }
    }

    /// Select a mail: mark it read (locally and on the server) and fetch its body if
    /// it isn't already cached.
    pub fn select(&mut self, i: usize) {
        self.view.set(View::Mail);
        self.selected.set(i);

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
            if was_unread {
                store.mails[i].unread = false;
            }
            if needs_body && uid != 0 {
                store.mails[i].body = BodyState::Loading;
            }
        }
        if uid != 0 {
            if needs_body {
                self.send(account, WorkerCmd::FetchBody { folder, uid });
            }
            if was_unread {
                self.send(account, WorkerCmd::SetSeen { folder, uid, seen: true });
            }
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
            let Some(m) = store.mails.get_mut(i) else { return };
            m.unread = !m.unread;
            (m.account, m.folder, m.uid, m.unread)
        };
        if uid != 0 {
            self.send(account, WorkerCmd::SetSeen { folder, uid, seen: !now_unread });
        }
    }

    /// Sync every folder of one account now (the "Sync now" button).
    pub fn sync_account(&self, account: usize) {
        self.send(account, WorkerCmd::SyncAll);
    }

    /// Save a new draft (no existing UID) to the server.
    pub fn save_new_draft(&self, account: usize, raw: String) {
        self.send(account, WorkerCmd::AppendDraft { raw });
    }

    /// Update an existing draft on the server (delete old, append new).
    pub fn update_draft(&self, account: usize, uid: u32, raw: String) {
        self.send(account, WorkerCmd::UpdateDraft { uid, raw });
    }
}
