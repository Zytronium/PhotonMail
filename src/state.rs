use std::{borrow::Cow, cmp::Reverse, collections::HashSet, io::Cursor, time::Duration};

use freya::prelude::*;
#[cfg(not(all(unix, not(target_os = "macos"))))]
use notify_rust::Notification as DesktopNotification;
use notify_rust::NotificationResponse;
use rodio::{Decoder, OutputStreamBuilder, Sink};

use crate::{
    cache::CacheWriter,
    keyboard::FocusState,
    link_prefs::LinkPrefs,
    mail::{ARCHIVE, Engine, INBOX, NewMailMessage, TRASH, WorkerCmd, config},
    model::{BodyState, Mail, NoticeSeverity, Store},
    settings_view::{AppSettings, NotificationSound},
};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Mail,
    Compose,
    AddAccount,
    Preferences,
    Settings(usize),
    EditAccount(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NotificationTarget {
    account: usize,
    uid: u32,
}

#[derive(Clone)]
pub(crate) struct NotificationChannels {
    pub open_rx: async_channel::Receiver<Option<String>>,
    pub open_tx: async_channel::Sender<Option<String>>,
    pub target_rx: async_channel::Receiver<NotificationTarget>,
    pub target_tx: async_channel::Sender<NotificationTarget>,
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
    pub starred_only: State<bool>,
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
    cache_ready: State<bool>,
    synced_folders: State<HashSet<(usize, usize)>>,
    reset_folders: State<HashSet<(usize, usize)>>,
}

#[derive(Clone)]
pub struct PendingSend {
    pub id: u64,
    pub remaining_seconds: u32,
    account: usize,
    raw: String,
    draft_origin: Option<(usize, u32)>,
}

impl PendingSend {
    fn tick(&mut self) -> bool {
        self.remaining_seconds = self.remaining_seconds.saturating_sub(1);
        self.remaining_seconds == 0
    }
}

#[cfg(test)]
mod pending_send_tests {
    use super::PendingSend;

    #[test]
    fn pending_send_countdown_expires_at_zero() {
        let mut pending = PendingSend {
            id: 1,
            remaining_seconds: 5,
            account: 0,
            raw: String::new(),
            draft_origin: None,
        };

        for expected in (0..5).rev() {
            assert_eq!(pending.tick(), expected == 0);
            assert_eq!(pending.remaining_seconds, expected);
        }
    }
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
    let run_on_startup = settings.peek().run_on_startup;

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
    let cache_writer = use_hook(move || (!demo).then(CacheWriter::start));

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
        starred_only: use_state(|| false),
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
        cache_ready: use_state(|| false),
        synced_folders: use_state(HashSet::new),
        reset_folders: use_state(HashSet::new),
    };

    use_hook(move || {
        if run_on_startup && let Err(error) = crate::startup::set_enabled(true) {
            eprintln!("Could not update startup registration: {error}");
        }
        if let Some(writer) = cache_writer.clone() {
            start_cache_load(app, writer.clone());
        }
        start_event_pump(app, cache_writer);
    });

    app
}

fn start_cache_load(app: AppState, writer: CacheWriter) {
    let cache_rx = crate::cache::load_async();
    spawn(async move {
        let mut app = app;
        let Ok(cache) = cache_rx.recv().await else {
            return;
        };
        let synced_folders = app.synced_folders.peek().clone();
        let reset_folders = app.reset_folders.peek().clone();
        app.store
            .write()
            .restore_cache(cache, &synced_folders, &reset_folders);
        app.cache_ready.set(true);
        writer.save(app.store.read().cache_snapshot());
    });
}

/// Runs on the UI thread as a Freya task: waits for events from the IMAP workers
/// and applies them to the store, which re-renders whatever reads it.
fn start_event_pump(app: AppState, cache_writer: Option<CacheWriter>) {
    let rx = app.engine.peek().events.clone();
    let is_app_focused = Platform::get().current_window().is_app_focused;
    let notification_channels = GlobalContexts::get().get_context::<NotificationChannels>();
    let notification_target_rx = notification_channels.target_rx.clone();
    let mut navigation_app = app;
    spawn(async move {
        while let Ok(target) = notification_target_rx.recv().await {
            open_notification_mail(&mut navigation_app, target);
        }
    });

    spawn(async move {
        let mut app = app;
        while let Ok(event) = rx.recv().await {
            let should_save_cache = matches!(
                &event,
                crate::mail::Event::Headers { .. }
                    | crate::mail::Event::FolderSynced { .. }
                    | crate::mail::Event::Flags { .. }
                    | crate::mail::Event::Removed { .. }
                    | crate::mail::Event::Reset { .. }
                    | crate::mail::Event::Restore { .. }
                    | crate::mail::Event::Body { .. }
                    | crate::mail::Event::AccountAdded { .. }
            );
            let selection_removed = if let Some(selected) = *app.selected.read() {
                let store = app.store.read();
                store
                    .mails
                    .get(selected)
                    .is_some_and(|mail| event_removes_mail(mail, &event))
            } else {
                false
            };
            match &event {
                crate::mail::Event::FolderSynced {
                    account, folder, ..
                } => {
                    app.synced_folders.write().insert((*account, *folder));
                    app.reset_folders.write().remove(&(*account, *folder));
                }
                crate::mail::Event::Reset { account, folder } => {
                    app.synced_folders.write().remove(&(*account, *folder));
                    app.reset_folders.write().insert((*account, *folder));
                }
                _ => {}
            }
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
                    let sound = settings.notification_sound;
                    let target = messages.first().map(|message| NotificationTarget {
                        account: *account,
                        uid: message.uid,
                    });
                    let notification_channels = notification_channels.clone();
                    if sound != NotificationSound::Muted {
                        std::thread::spawn(move || {
                            if let Some(sound) = notification_sound_data(sound) {
                                play_notification_sound(sound);
                            }
                        });
                    }
                    std::thread::spawn(move || {
                        #[cfg(all(unix, not(target_os = "macos")))]
                        show_notification_and_wait(&summary, &body, notification_channels, target);

                        #[cfg(not(all(unix, not(target_os = "macos"))))]
                        {
                            let mut notification = DesktopNotification::new();
                            notification.summary(&summary).body(&body);
                            if let Ok(handle) = notification.show() {
                                let _ =
                                    handle.wait_for_response(|response: &NotificationResponse| {
                                        if is_notification_open_response(response) {
                                            let _ = notification_channels.open_tx.try_send(None);
                                            if let Some(target) = target {
                                                let _ = notification_channels
                                                    .target_tx
                                                    .try_send(target);
                                            }
                                        }
                                    });
                            }
                        }
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
            if let crate::mail::Event::EmailSent { account } = &event {
                let revision = {
                    let mut store = app.store.write();
                    store.send_error = None;
                    let message = store.accounts.get(*account).map_or_else(
                        || "Message sent successfully.".to_string(),
                        |account| format!("Message sent successfully for {}.", account.name),
                    );
                    store.set_notice(message, NoticeSeverity::Info)
                };
                // Clear the notice after a delay — use futures-timer instead of tokio::time::sleep
                // because Freya's spawn runs on futures-lite, not a Tokio runtime.
                let mut app_clone = app.clone();
                freya::prelude::spawn(async move {
                    futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
                    app_clone.store.write().clear_notice_if_revision(revision);
                });
            }
            if let crate::mail::Event::EmailSendFailed { account, error } = &event {
                // Keep failures visible in the compose banner until the user dismisses them.
                let mut store = app.store.write();
                let message = store.accounts.get(*account).map_or_else(
                    || format!("Message not sent: {error}"),
                    |account| format!("Message not sent for {}: {error}", account.name),
                );
                store.send_error = Some(message);
            }
            let (added, remap) = {
                let mut store = app.store.write();
                let added = store.apply(event);
                let remap = store.compact_removed();
                (added, remap)
            };
            if selection_removed {
                app.selected.set(None);
                app.cancel_mark_read_timer();
                app.attachment_preview_mail.set(None);
                app.attachment_preview_open.set(false);
            }
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
            if should_save_cache
                && *app.cache_ready.peek()
                && let Some(writer) = &cache_writer
            {
                writer.save(app.store.read().cache_snapshot());
            }
        }
    });
}

fn notification_sound_data(sound: NotificationSound) -> Option<Cow<'static, [u8]>> {
    match sound {
        NotificationSound::Muted => None,
        NotificationSound::Classic => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-classic.mp3")[..],
        )),
        NotificationSound::Center => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-center.mp3")[..],
        )),
        NotificationSound::Document => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-document.mp3")[..],
        )),
        NotificationSound::GameBonus => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-game-bonus.mp3")[..],
        )),
        NotificationSound::Good => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-good.mp3")[..],
        )),
        NotificationSound::Mars => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-mars.mp3")[..],
        )),
        NotificationSound::Mercury => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-mercury.mp3")[..],
        )),
        NotificationSound::Message => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-message.mp3")[..],
        )),
        NotificationSound::Simple => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-simple.mp3")[..],
        )),
        NotificationSound::Space => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-space.mp3")[..],
        )),
        NotificationSound::Ufo => Some(Cow::Borrowed(
            &include_bytes!("../assets/sounds/notification-ufo.mp3")[..],
        )),
        NotificationSound::WhyThePhotonWouldYouUseThisSound => Some(Cow::Borrowed(
            &include_bytes!(
                "../assets/sounds/notification-why-the-photon-would-you-use-this-sound.mp3"
            )[..],
        )),
    }
}

fn event_removes_mail(mail: &crate::model::Mail, event: &crate::mail::Event) -> bool {
    match event {
        crate::mail::Event::FolderSynced {
            account,
            folder,
            uids,
        } => {
            mail.account == *account
                && mail.folder == *folder
                && mail.uid != 0
                && !uids.contains(&mail.uid)
        }
        crate::mail::Event::Removed {
            account,
            folder,
            uids,
        } => mail.account == *account && mail.folder == *folder && uids.contains(&mail.uid),
        crate::mail::Event::Reset { account, folder } => {
            mail.account == *account && mail.folder == *folder
        }
        _ => false,
    }
}

fn play_notification_sound(sound: Cow<'static, [u8]>) {
    let Ok(stream) = OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let sink = Sink::connect_new(stream.mixer());
    let Ok(decoder) = Decoder::try_from(Cursor::new(sound)) else {
        return;
    };
    sink.append(decoder);
    sink.sleep_until_end();
}

pub fn preview_notification_sound(sound: NotificationSound) {
    if let Some(sound) = notification_sound_data(sound) {
        std::thread::spawn(move || play_notification_sound(sound));
    }
}

fn visible_mail_indices(
    mails: &[Mail],
    account: Option<usize>,
    folder: usize,
    unread_only: bool,
    starred_only: bool,
) -> Vec<usize> {
    let mut visible: Vec<usize> = mails
        .iter()
        .enumerate()
        .filter(|(_, mail)| {
            !mail.removed
                && (!starred_only || mail.starred)
                && (starred_only || mail.folder == folder)
                && account.map_or(true, |a| mail.account == a)
                && (!unread_only || mail.unread)
        })
        .map(|(index, _)| index)
        .collect();
    visible.sort_by_key(|&index| Reverse((mails[index].ts, mails[index].uid)));
    visible
}

pub(crate) fn mail_matches_search_query(mail: &Mail, query: &str) -> bool {
    let subject_match = mail.subject.to_lowercase().contains(query);
    let from_match =
        mail.from.to_lowercase().contains(query) || mail.from_email.to_lowercase().contains(query);
    let to_match =
        mail.to.to_lowercase().contains(query) || mail.to_email.to_lowercase().contains(query);
    let preview_match = mail.preview.to_lowercase().contains(query);
    let body_match = match &mail.body {
        BodyState::Loaded { text, .. } => text.to_lowercase().contains(query),
        _ => false,
    };
    subject_match || from_match || to_match || preview_match || body_match
}

impl AppState {
    /// Mails in the current account + folder, or starred mail for the selected account scope.
    pub fn visible(&self) -> Vec<usize> {
        let folder = *self.folder.read();
        let unread_only = *self.unread_only.read();
        let starred_only = *self.starred_only.read();
        let account = *self.account.read();
        let store = self.store.read();
        visible_mail_indices(&store.mails, account, folder, unread_only, starred_only)
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
            .filter(|&i| mail_matches_search_query(&store.mails[i], &query))
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

    /// Unread starred mails across all folders of one account.
    pub fn account_starred_unread(&self, account: usize) -> usize {
        self.store
            .read()
            .mails
            .iter()
            .filter(|m| !m.removed && m.account == account && m.starred && m.unread)
            .count()
    }

    /// Unread starred mails across all accounts and folders.
    pub fn starred_unread(&self) -> usize {
        self.store
            .read()
            .mails
            .iter()
            .filter(|m| !m.removed && m.starred && m.unread)
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
        self.starred_only.set(false);
        self.set_view(View::Mail);

        // Make sure what's on screen is fresh. Workers skip folders synced in the last
        // few seconds, so this is cheap to call on every navigation.
        let accounts = self.store.peek().accounts.len();
        match account {
            Some(a) => self.send(a, WorkerCmd::SyncFolder { folder }),
            None => (0..accounts).for_each(|a| self.send(a, WorkerCmd::SyncFolder { folder })),
        }
    }

    pub fn go_starred(&mut self, account: usize) {
        self.cancel_mark_read_timer();
        self.account.set(Some(account));
        self.folder.set(INBOX);
        self.starred_only.set(true);
        self.set_view(View::Mail);
    }

    pub fn go_global_starred(&mut self) {
        self.cancel_mark_read_timer();
        self.account.set(None);
        self.folder.set(INBOX);
        self.starred_only.set(true);
        self.set_view(View::Mail);
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

    pub fn toggle_star(&mut self, i: usize) {
        let (account, folder, uid, starred) = {
            let mut store = self.store.write();
            let Some(mail) = store.mails.get_mut(i) else {
                return;
            };
            mail.starred = !mail.starred;
            (mail.account, mail.folder, mail.uid, mail.starred)
        };
        if uid != 0 {
            self.send(
                account,
                WorkerCmd::SetFlagged {
                    folder,
                    uid,
                    flagged: starred,
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
        if updated.run_on_startup != previous.run_on_startup
            && let Err(error) = crate::startup::set_enabled(updated.run_on_startup)
        {
            eprintln!("Could not update startup registration: {error}");
            self.settings.set(previous);
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
            remaining_seconds: delay_seconds,
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
            spawn_in_window(async move {
                for _ in 0..delay_seconds {
                    futures_timer::Delay::new(Duration::from_secs(1)).await;
                    let expired = {
                        let mut pending_sends = state.pending_sends.write();
                        pending_sends
                            .iter_mut()
                            .find(|pending| pending.id == id)
                            .map(PendingSend::tick)
                    };

                    match expired {
                        Some(true) => {
                            state.commit_pending_send(id);
                            return;
                        }
                        Some(false) => {}
                        None => return,
                    }
                }
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

fn is_notification_open_response(response: &NotificationResponse) -> bool {
    match response {
        NotificationResponse::Default => true,
        NotificationResponse::Action(action) => action == "default",
        NotificationResponse::Reply(_) | NotificationResponse::Closed(_) => false,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_notification_and_wait(
    summary: &str,
    body: &str,
    notification_channels: NotificationChannels,
    target: Option<NotificationTarget>,
) {
    use dbus::{
        Message,
        arg::messageitem::{MessageItem, MessageItemArray},
        ffidisp::{BusType, Connection, ConnectionItem},
    };

    const NOTIFICATIONS_INTERFACE: &str = "org.freedesktop.Notifications";
    const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";

    let Ok(connection) = Connection::get_private(BusType::Session) else {
        return;
    };
    for member in ["ActivationToken", "ActionInvoked", "NotificationClosed"] {
        let rule = format!(
            "path='{NOTIFICATIONS_PATH}',interface='{NOTIFICATIONS_INTERFACE}',member='{member}'"
        );
        if connection.add_match(&rule).is_err() {
            return;
        }
    }

    let Ok(mut notification) = Message::new_method_call(
        NOTIFICATIONS_INTERFACE,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
        "Notify",
    ) else {
        return;
    };
    let actions = MessageItem::new_array(vec![
        "default".to_owned().into(),
        "Open Photon Mail".to_owned().into(),
    ])
    .expect("notification action list must have string values");
    let hints = MessageItem::Array(
        MessageItemArray::new(Vec::new(), "a{sv}".into())
            .expect("empty notification hints must be a dictionary"),
    );
    notification.append_items(&[
        "Photon Mail".to_owned().into(),
        0u32.into(),
        String::new().into(),
        summary.to_owned().into(),
        body.to_owned().into(),
        actions,
        hints,
        (-1i32).into(),
    ]);

    let Ok(reply) = connection.send_with_reply_and_block(notification, 2000) else {
        return;
    };
    let reply_items = reply.get_items();
    let Some(MessageItem::UInt32(notification_id)) = reply_items.first() else {
        return;
    };
    let notification_id = *notification_id;
    let mut activation_token = None;

    for item in connection.iter(1000) {
        let ConnectionItem::Signal(message) = item else {
            continue;
        };
        let member = message.member().map_or_else(String::new, |member| {
            member.into_cstring().to_string_lossy().into_owned()
        });
        let items = message.get_items();

        match member.as_str() {
            "ActivationToken" => {
                if let (Some(MessageItem::UInt32(signal_id)), Some(MessageItem::Str(token))) =
                    (items.first(), items.get(1))
                {
                    if let Some(token) = activation_token_for_notification(
                        notification_id,
                        *signal_id,
                        token.clone(),
                    ) {
                        activation_token = Some(token);
                    }
                }
            }
            "ActionInvoked" => {
                if let (Some(MessageItem::UInt32(signal_id)), Some(MessageItem::Str(action))) =
                    (items.first(), items.get(1))
                {
                    if *signal_id == notification_id
                        && is_notification_open_response(&NotificationResponse::Action(
                            action.clone(),
                        ))
                    {
                        let _ = notification_channels.open_tx.try_send(activation_token);
                        if let Some(target) = target {
                            let _ = notification_channels.target_tx.try_send(target);
                        }
                        return;
                    }
                }
            }
            "NotificationClosed" => {
                if let Some(MessageItem::UInt32(signal_id)) = items.first() {
                    if *signal_id == notification_id {
                        return;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn activation_token_for_notification(
    notification_id: u32,
    signal_id: u32,
    token: String,
) -> Option<String> {
    (notification_id == signal_id && !token.is_empty()).then_some(token)
}

fn open_notification_mail(app: &mut AppState, target: NotificationTarget) {
    let index = find_notification_mail_index(&app.store.read(), target);
    app.search_query.set(String::new());
    app.unread_only.set(false);
    app.go(Some(target.account), crate::mail::INBOX);
    if let Some(index) = index {
        app.select(index);
    }
}

fn find_notification_mail_index(store: &Store, target: NotificationTarget) -> Option<usize> {
    store.mails.iter().position(|mail| {
        !mail.removed
            && mail.account == target.account
            && mail.folder == crate::mail::INBOX
            && mail.uid == target.uid
    })
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
            uid: 1,
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
                uid: 1,
                from: "Alice".to_string(),
                subject: "Project update".to_string(),
            },
            NewMailMessage {
                uid: 2,
                from: "Bob".to_string(),
                subject: "Schedule".to_string(),
            },
        ];
        let (summary, body) = new_mail_notification(&messages);

        assert_eq!(summary, "2 new emails");
        assert_eq!(body, "Alice: Project update\nand 1 more");
    }

    #[test]
    fn default_notification_response_opens_the_app() {
        assert!(is_notification_open_response(
            &NotificationResponse::Default
        ));
        assert!(is_notification_open_response(
            &NotificationResponse::Action("default".to_string())
        ));
        assert!(!is_notification_open_response(
            &NotificationResponse::Action("archive".to_string())
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn activation_token_is_matched_to_its_notification() {
        assert_eq!(
            activation_token_for_notification(7, 7, "plasma-token".to_string()),
            Some("plasma-token".to_string())
        );
        assert_eq!(
            activation_token_for_notification(7, 8, "other-token".to_string()),
            None
        );
        assert_eq!(activation_token_for_notification(7, 7, String::new()), None);
    }

    #[test]
    fn notification_target_resolves_the_matching_inbox_message() {
        let mut store = Store::demo();
        for (index, mail) in store.mails.iter_mut().enumerate() {
            mail.uid = index as u32 + 1;
        }
        let index = store
            .mails
            .iter()
            .position(|mail| mail.folder == crate::mail::INBOX)
            .unwrap();
        let mail = &store.mails[index];
        let target = NotificationTarget {
            account: mail.account,
            uid: mail.uid,
        };

        assert_eq!(find_notification_mail_index(&store, target), Some(index));
        assert_eq!(
            find_notification_mail_index(
                &store,
                NotificationTarget {
                    account: usize::MAX,
                    uid: target.uid,
                }
            ),
            None
        );
    }

    #[test]
    fn sync_completion_invalidates_a_selected_cached_message_only_when_missing() {
        let mut mail = Store::demo().mails[0].clone();
        mail.uid = 42;
        let event = crate::mail::Event::FolderSynced {
            account: mail.account,
            folder: mail.folder,
            uids: vec![43],
        };
        assert!(event_removes_mail(&mail, &event));

        let event = crate::mail::Event::FolderSynced {
            account: mail.account,
            folder: mail.folder,
            uids: vec![42],
        };
        assert!(!event_removes_mail(&mail, &event));
    }
}

fn visible_selection(selected: Option<usize>, visible: &[usize]) -> Option<usize> {
    selected.filter(|selected| visible.contains(selected))
}

#[cfg(test)]
mod tests {
    use super::{visible_mail_indices, visible_selection};
    use crate::{
        mail::{ARCHIVE, INBOX, TRASH},
        model::Store,
    };

    #[test]
    fn account_starred_view_includes_other_folders_but_excludes_other_accounts_and_removed_mail() {
        let base = Store::demo().mails[0].clone();

        let mut inbox = base.clone();
        inbox.account = 0;
        inbox.folder = INBOX;
        inbox.uid = 1;
        inbox.ts = 10;
        inbox.starred = false;

        let mut archived_star = base.clone();
        archived_star.account = 1;
        archived_star.folder = ARCHIVE;
        archived_star.uid = 2;
        archived_star.ts = 20;
        archived_star.starred = true;
        archived_star.unread = false;

        let mut unread_star = base.clone();
        unread_star.account = 0;
        unread_star.folder = TRASH;
        unread_star.uid = 3;
        unread_star.ts = 30;
        unread_star.starred = true;
        unread_star.unread = true;

        let mut removed_star = base;
        removed_star.account = 0;
        removed_star.folder = INBOX;
        removed_star.uid = 4;
        removed_star.ts = 40;
        removed_star.starred = true;
        removed_star.removed = true;

        let mails = vec![inbox, archived_star, unread_star, removed_star];

        assert_eq!(
            visible_mail_indices(&mails, None, INBOX, false, true),
            vec![2, 1]
        );
        assert_eq!(
            visible_mail_indices(&mails, None, INBOX, true, true),
            vec![2]
        );
        assert_eq!(
            visible_mail_indices(&mails, Some(0), INBOX, false, true),
            vec![2]
        );
        assert_eq!(
            visible_mail_indices(&mails, Some(0), INBOX, true, true),
            vec![2]
        );
        assert_eq!(
            visible_mail_indices(&mails, Some(1), INBOX, false, true),
            vec![1]
        );
        assert_eq!(
            visible_mail_indices(&mails, Some(0), INBOX, false, false),
            vec![0]
        );
    }

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
