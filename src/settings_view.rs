use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use freya::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{state::AppState, theme::DARK, widgets::divider};

const AUTO_SYNC_OPTIONS: &[(u32, &str)] = &[
    (0, "Off"),
    (5, "5 sec"),
    (15, "15 sec"),
    (30, "30 sec"),
    (60, "1 min"),
    (300, "5 min"),
];
const MARK_READ_OPTIONS: &[(u32, &str)] = &[
    (0, "Instant"),
    (500, "0.5 sec"),
    (1000, "1 sec"),
    (2000, "2 sec"),
    (3000, "3 sec"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum NotificationSound {
    #[serde(rename = "muted")]
    Muted,
    #[default]
    #[serde(rename = "notification-classic")]
    Classic,
    #[serde(rename = "notification-center")]
    Center,
    #[serde(rename = "notification-document")]
    Document,
    #[serde(rename = "notification-game-bonus")]
    GameBonus,
    #[serde(rename = "notification-good")]
    Good,
    #[serde(rename = "notification-mars")]
    Mars,
    #[serde(rename = "notification-mercury")]
    Mercury,
    #[serde(rename = "notification-message")]
    Message,
    #[serde(rename = "notification-simple")]
    Simple,
    #[serde(rename = "notification-space")]
    Space,
    #[serde(rename = "notification-ufo")]
    Ufo,
    #[serde(rename = "notification-why-the-photon-would-you-use-this-sound")]
    WhyThePhotonWouldYouUseThisSound,
}

impl NotificationSound {
    fn id(self) -> &'static str {
        match self {
            Self::Muted => "muted",
            Self::Classic => "notification-classic",
            Self::Center => "notification-center",
            Self::Document => "notification-document",
            Self::GameBonus => "notification-game-bonus",
            Self::Good => "notification-good",
            Self::Mars => "notification-mars",
            Self::Mercury => "notification-mercury",
            Self::Message => "notification-message",
            Self::Simple => "notification-simple",
            Self::Space => "notification-space",
            Self::Ufo => "notification-ufo",
            Self::WhyThePhotonWouldYouUseThisSound => {
                "notification-why-the-photon-would-you-use-this-sound"
            }
        }
    }
}

pub const NOTIFICATION_SOUND_OPTIONS: &[(NotificationSound, &str)] = &[
    (NotificationSound::Muted, "Muted"),
    (NotificationSound::Classic, "Classic"),
    (NotificationSound::Center, "Center"),
    (NotificationSound::Document, "Document"),
    (NotificationSound::GameBonus, "Game Bonus"),
    (NotificationSound::Good, "Good"),
    (NotificationSound::Mars, "Mars"),
    (NotificationSound::Mercury, "Mercury"),
    (NotificationSound::Message, "Message"),
    (NotificationSound::Simple, "Simple"),
    (NotificationSound::Space, "Space"),
    (NotificationSound::Ufo, "UFO"),
    (
        NotificationSound::WhyThePhotonWouldYouUseThisSound,
        "Why the photon would you use this sound",
    ),
];

const NOTIFICATION_SOUND_OPTION_HEIGHT: f32 = 34.;
const LONG_NOTIFICATION_SOUND_OPTION_HEIGHT: f32 = 48.;

fn notification_sound_option_height(sound: NotificationSound) -> f32 {
    if sound == NotificationSound::WhyThePhotonWouldYouUseThisSound {
        LONG_NOTIFICATION_SOUND_OPTION_HEIGHT
    } else {
        NOTIFICATION_SOUND_OPTION_HEIGHT
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub block_tracking_pixels: bool,
    pub block_remote_images: bool,
    pub show_gravatar: bool,
    pub auto_sync_seconds: u32,
    pub notifications_enabled: bool,
    disabled_notification_accounts: HashSet<String>,
    pub sidebar_account_order: Vec<String>,
    pub notification_sound: NotificationSound,
    pub mark_read_after_millis: u32,
    pub undo_send_seconds: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            block_tracking_pixels: true,
            block_remote_images: true,
            show_gravatar: false,
            auto_sync_seconds: 300,
            notifications_enabled: true,
            disabled_notification_accounts: HashSet::new(),
            sidebar_account_order: Vec::new(),
            notification_sound: NotificationSound::default(),
            mark_read_after_millis: 0,
            undo_send_seconds: 5,
        }
    }
}

impl AppSettings {
    pub fn ordered_account_ids(&self, account_ids: &[String]) -> Vec<String> {
        let mut ordered: Vec<String> = self
            .sidebar_account_order
            .iter()
            .filter(|id| account_ids.contains(id))
            .cloned()
            .collect();
        for id in account_ids {
            if !ordered.contains(id) {
                ordered.push(id.clone());
            }
        }
        ordered
    }

    pub fn reorder_sidebar_accounts(&mut self, account_ids: &[String], from_id: &str, to_id: &str) {
        if from_id == to_id {
            return;
        }
        let mut order = self.ordered_account_ids(account_ids);
        let Some(from_index) = order.iter().position(|id| id == from_id) else {
            return;
        };
        let Some(target_index) = order.iter().position(|id| id == to_id) else {
            return;
        };
        let account_id = order.remove(from_index);
        order.insert(target_index, account_id);
        self.sidebar_account_order = order;
    }

    pub fn auto_sync_interval(&self) -> Option<Duration> {
        match self.auto_sync_seconds {
            0 => None,
            5 | 15 | 30 | 60 | 300 => Some(Duration::from_secs(u64::from(self.auto_sync_seconds))),
            _ => Some(Duration::from_secs(300)),
        }
    }

    pub fn notifications_for_account(&self, account_id: &str) -> bool {
        self.notifications_enabled && self.account_notifications_enabled(account_id)
    }

    pub fn gravatar_enabled(&self) -> bool {
        self.show_gravatar
    }

    pub fn account_notifications_enabled(&self, account_id: &str) -> bool {
        !self.disabled_notification_accounts.contains(account_id)
    }

    pub fn set_account_notifications_enabled(&mut self, account_id: &str, enabled: bool) {
        if enabled {
            self.disabled_notification_accounts.remove(account_id);
        } else {
            self.disabled_notification_accounts
                .insert(account_id.to_string());
        }
    }

    pub fn save_to_disk(&self) {
        if let Some(path) = settings_path()
            && let Err(error) = self.save_to_path(&path)
        {
            eprintln!("Could not save settings: {error}");
        }
    }

    pub fn load_from_disk() -> Self {
        settings_path().map_or_else(Self::default, |path| Self::load_from_path(&path))
    }

    fn load_from_path(path: &Path) -> Self {
        fs::read(path).map_or_else(|_| Self::default(), |bytes| Self::parse(&bytes))
    }

    fn parse(bytes: &[u8]) -> Self {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return Self::default();
        };
        Self::parse_value(value).unwrap_or_default()
    }

    fn parse_value(mut value: serde_json::Value) -> Result<Self, serde_json::Error> {
        if let Some(object) = value.as_object_mut() {
            if !object.contains_key("auto_sync_seconds") {
                if let Some(minutes) = object
                    .remove("auto_sync_minutes")
                    .and_then(|value| value.as_u64())
                {
                    let seconds = match minutes {
                        0 => 0,
                        1 => 60,
                        5 | 15 | 30 => 300,
                        _ => 300,
                    };
                    object.insert("auto_sync_seconds".to_string(), seconds.into());
                }
            }
            if !object.contains_key("mark_read_after_millis") {
                if let Some(seconds) = object
                    .remove("mark_read_after_seconds")
                    .and_then(|value| value.as_u64())
                {
                    let millis = if seconds == 0 { 0 } else { 3000 };
                    object.insert("mark_read_after_millis".to_string(), millis.into());
                }
            }
            if let Some(value) = object.get("notification_sound").cloned() {
                let sound = match value {
                    serde_json::Value::Bool(true) => "notification-classic".to_string(),
                    serde_json::Value::Bool(false) => "muted".to_string(),
                    serde_json::Value::String(value)
                        if NOTIFICATION_SOUND_OPTIONS
                            .iter()
                            .any(|(sound, _)| sound.id() == value) =>
                    {
                        value
                    }
                    _ => "notification-classic".to_string(),
                };
                object.insert("notification_sound".to_string(), sound.into());
            }
        }

        let mut settings: Self = serde_json::from_value(value)?;
        if ![0, 5, 15, 30, 60, 300].contains(&settings.auto_sync_seconds) {
            settings.auto_sync_seconds = 300;
        }
        if ![0, 500, 1000, 2000, 3000].contains(&settings.mark_read_after_millis) {
            settings.mark_read_after_millis = 0;
        }
        if ![0, 5, 10, 15].contains(&settings.undo_send_seconds) {
            settings.undo_send_seconds = 5;
        }
        Ok(settings)
    }

    fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if path.exists() {
            let existing = fs::read(path)?;
            let is_valid = serde_json::from_slice::<Self>(&existing).is_ok()
                || serde_json::from_slice::<serde_json::Value>(&existing)
                    .ok()
                    .is_some_and(|value| Self::parse_value(value).is_ok());
            if !is_valid {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "existing settings are invalid; refusing to overwrite them",
                ));
            }
        }

        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let contents = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let temporary = path.with_file_name(format!("{file_name}.tmp"));
        fs::write(&temporary, contents)?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(temporary, path)
    }
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("PhotonMail").join("settings.json"))
}

#[derive(PartialEq)]
pub struct PreferencesView {
    pub state: AppState,
}

impl Component for PreferencesView {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let settings = state.settings.read().clone();
        let accounts = state.store.read().accounts.clone();
        let show_notification_sound_menu = use_state(|| false);

        let mut rows = vec![
            section_title("Privacy"),
            toggle_row(
                "Block tracking pixels",
                "Keep likely tracking images blocked, even when other remote images are allowed.",
                settings.block_tracking_pixels,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.block_tracking_pixels = !settings.block_tracking_pixels;
                        });
                    }
                }),
            ),
            toggle_row(
                "Block remote images",
                "Prevent email images from contacting external servers; individual messages can still load them on request.",
                settings.block_remote_images,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.block_remote_images = !settings.block_remote_images;
                        });
                    }
                }),
            ),
            toggle_row(
                "Show Gravatar profile pictures",
                "Fetch sender avatars from Gravatar without allowing images embedded in messages.",
                settings.show_gravatar,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.show_gravatar = !settings.show_gravatar
                        });
                    }
                }),
            ),
            section_title("Sync"),
            setting_label(
                "Automatically sync",
                "Choose how often Photon Mail checks every account.",
            ),
            option_picker(
                state,
                settings.auto_sync_seconds,
                AUTO_SYNC_OPTIONS,
                set_auto_sync,
            ),
            section_title("Notifications"),
            toggle_row(
                "Send desktop notifications",
                "Show an alert when unread messages arrive while the window is unfocused.",
                settings.notifications_enabled,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.notifications_enabled = !settings.notifications_enabled;
                        });
                    }
                }),
            ),
            setting_label(
                "Per-account notifications",
                "Disable alerts for selected accounts.",
            ),
        ];

        for account in accounts {
            let account_id = account.id.clone();
            let enabled = settings.account_notifications_enabled(&account_id);
            let title = account.name.clone();
            let account_id_for_update = account_id.clone();
            rows.push(toggle_row(
                &title,
                &account.addr,
                enabled,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.set_account_notifications_enabled(
                                &account_id_for_update,
                                !enabled,
                            );
                        });
                    }
                }),
            ));
        }

        rows.extend([
            notification_sound_row(
                state,
                settings.notification_sound,
                show_notification_sound_menu,
            ),
            section_title("Reading and sending"),
            setting_label(
                "Mark as read after opening",
                "Wait before marking a message as read; leaving it cancels the timer.",
            ),
            option_picker(
                state,
                settings.mark_read_after_millis,
                MARK_READ_OPTIONS,
                set_mark_read_after,
            ),
            setting_label(
                "Undo send timer",
                "Messages are held locally until this timer expires.",
            ),
            option_picker(
                state,
                settings.undo_send_seconds,
                &[(0, "Off"), (5, "5 sec"), (10, "10 sec"), (15, "15 sec")],
                set_undo_send,
            ),
        ]);

        rect()
            .width(Size::fill())
            .height(Size::fill())
            .spacing(16.)
            .child(
                rect()
                    .spacing(4.)
                    .child(
                        label()
                            .text("Settings")
                            .font_size(24.)
                            .font_weight(FontWeight::BOLD)
                            .color(DARK.text),
                    )
                    .child(
                        label()
                            .text("Preferences are saved automatically.")
                            .color(DARK.muted),
                    ),
            )
            .child(divider())
            .child(
                rect()
                    .margin((0., -36., 0., 0.))
                    .width(Size::fill())
                    .height(Size::fill())
                    .child(
                        ScrollView::new().height(Size::fill()).child(
                            rect()
                                .width(Size::fill())
                                .padding((0., 36., 0., 0.))
                                .vertical()
                                .spacing(14.)
                                .children(
                                    rows.into_iter()
                                        .enumerate()
                                        .map(|(index, row)| row.key(index)),
                                ),
                        ),
                    ),
            )
    }
}

fn set_auto_sync(settings: &mut AppSettings, value: u32) {
    settings.auto_sync_seconds = value;
}

fn set_mark_read_after(settings: &mut AppSettings, value: u32) {
    settings.mark_read_after_millis = value;
}

fn set_undo_send(settings: &mut AppSettings, value: u32) {
    settings.undo_send_seconds = value;
}

fn section_title(title: &'static str) -> Rect {
    rect().padding((8., 0., 0., 0.)).child(
        label()
            .text(title)
            .font_size(17.)
            .font_weight(FontWeight::BOLD)
            .color(DARK.text),
    )
}

fn setting_label(title: &'static str, description: &'static str) -> Rect {
    rect()
        .spacing(3.)
        .child(label().text(title).color(DARK.text).font_size(14.))
        .child(label().text(description).color(DARK.muted).font_size(12.))
}

fn notification_sound_row(
    state: AppState,
    current: NotificationSound,
    show_menu: State<bool>,
) -> Rect {
    let current_label = NOTIFICATION_SOUND_OPTIONS
        .iter()
        .find(|(sound, _)| *sound == current)
        .map(|(_, label)| *label)
        .unwrap_or("Classic");
    let mut toggle_show_menu = show_menu.clone();
    let toggle_menu = EventHandler::new(move |_| toggle_show_menu.set(!toggle_show_menu()));
    let mut selector = rect()
        .width(Size::flex(0.5))
        .height(Size::px(36.))
        .padding((0., 8., 0., 10.))
        .corner_radius(6.)
        .background(Fill::from((20, 20, 31)))
        .border(Border::new().fill((58, 58, 78)).width(1.))
        .horizontal()
        .spacing(8.)
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .cursor(freya::winit::window::CursorIcon::Pointer)
        .on_press(toggle_menu)
        .child(
            rect()
                .width(Size::flex(1.))
                .horizontal()
                .main_align(Alignment::End)
                .child(
                    label()
                        .text(current_label)
                        .color(DARK.text)
                        .font_size(13.)
                        .max_lines(1),
                ),
        )
        .child(label().text("▾").color(DARK.muted).font_size(13.));

    if *show_menu.read() {
        let mut menu = rect().vertical().padding((6., 0., 0., 0.));
        for &(sound, title) in NOTIFICATION_SOUND_OPTIONS {
            let selected = sound == current;
            menu = menu.child(NotificationSoundOption {
                state,
                sound,
                title,
                selected,
                show_menu: show_menu.clone(),
            });
        }
        let list_height = NOTIFICATION_SOUND_OPTIONS
            .iter()
            .map(|(sound, _)| notification_sound_option_height(*sound))
            .sum::<f32>()
            .min(204.);
        selector = selector.child(
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_absolute().top(36.).left(0.))
                .width(Size::percent(100.))
                .corner_radius(8.)
                .background(Fill::from((30, 30, 46)))
                .border(Border::new().fill((58, 58, 78)).width(1.))
                .child(
                    ScrollView::new()
                        .width(Size::percent(100.))
                        .height(Size::px(list_height))
                        .child(menu.width(Size::percent(100.))),
                ),
        );
    }

    rect()
        .width(Size::fill())
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .spacing(12.)
        .child(
            rect()
                .width(Size::flex(1.))
                .spacing(3.)
                .child(
                    label()
                        .text("Notification sound")
                        .color(DARK.text)
                        .font_size(14.),
                )
                .child(
                    label()
                        .text("Choose a sound for desktop notifications, or mute them.")
                        .color(DARK.muted)
                        .font_size(12.),
                ),
        )
        .child(selector)
}

#[derive(PartialEq)]
struct NotificationSoundOption {
    state: AppState,
    sound: NotificationSound,
    title: &'static str,
    selected: bool,
    show_menu: State<bool>,
}

impl Component for NotificationSoundOption {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let mut select_state = self.state;
        let mut show_menu = self.show_menu.clone();
        let sound = self.sound;
        let selected = self.selected;
        let background = if selected {
            crate::grain::selected()
        } else if hovering() {
            crate::grain::hover()
        } else {
            Fill::from((30, 30, 46))
        };

        let mut option = rect()
            .width(Size::percent(100.))
            .height(Size::px(notification_sound_option_height(self.sound)))
            .padding((0., 12., 0., 0.))
            .corner_radius(4.)
            .background(background)
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .cursor(freya::winit::window::CursorIcon::Pointer)
            .on_press(EventHandler::new(move |_| {
                select_state.update_settings(|settings| settings.notification_sound = sound);
                show_menu.set(false);
            }));

        if self.sound != NotificationSound::Muted {
            option = option.child(
                rect()
                    .width(Size::px(26.))
                    .height(Size::px(26.))
                    .margin((0., 6., 0., 0.))
                    .corner_radius(5.)
                    .center()
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(move |event: Event<PressEventData>| {
                        event.stop_propagation();
                        crate::state::preview_notification_sound(sound);
                    })
                    .child(
                        label()
                            .text("▶")
                            .color(if selected { DARK.accent } else { DARK.muted })
                            .font_size(11.),
                    ),
            );
        }

        option.child(
            rect()
                .width(Size::flex(1.))
                .horizontal()
                .main_align(Alignment::End)
                .child(
                    label()
                        .text(self.title)
                        .color(if selected { DARK.accent } else { DARK.text })
                        .font_size(13.),
                ),
        )
    }
}

fn toggle_row(
    title: &str,
    description: &str,
    enabled: bool,
    on_press: EventHandler<Event<PressEventData>>,
) -> Rect {
    rect()
        .width(Size::fill())
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .spacing(12.)
        .child(
            rect()
                .width(Size::flex(1.))
                .spacing(3.)
                .child(
                    label()
                        .text(title.to_string())
                        .color(DARK.text)
                        .font_size(14.),
                )
                .child(
                    label()
                        .text(description.to_string())
                        .color(DARK.muted)
                        .font_size(12.),
                ),
        )
        .child(toggle_control(enabled, on_press))
}

fn toggle_control(enabled: bool, on_press: EventHandler<Event<PressEventData>>) -> Rect {
    let thumb = || {
        rect()
            .width(Size::px(18.))
            .height(Size::px(18.))
            .corner_radius(9.)
            .background(DARK.text)
    };
    let spacer = || rect().width(Size::px(22.)).height(Size::px(1.));
    let control = rect()
        .width(Size::px(46.))
        .height(Size::px(26.))
        .horizontal()
        .cross_align(Alignment::Center)
        .padding(3.)
        .corner_radius(13.)
        .background(if enabled {
            Fill::from(DARK.accent)
        } else {
            crate::grain::hover()
        })
        .border(
            Border::new()
                .width(1.)
                .fill(if enabled { DARK.accent } else { DARK.divider }),
        )
        .cursor(freya::winit::window::CursorIcon::Pointer)
        .on_press(on_press);
    if enabled {
        control.child(spacer()).child(thumb())
    } else {
        control.child(thumb()).child(spacer())
    }
}

fn option_picker(
    state: AppState,
    current: u32,
    choices: &[(u32, &'static str)],
    set: fn(&mut AppSettings, u32),
) -> Rect {
    rect()
        .horizontal()
        .spacing(8.)
        .children(choices.iter().map(|(value, title)| {
            let value = *value;
            let selected = current == value;
            let mut state = state;
            option_button(
                title,
                selected,
                EventHandler::new(move |_| {
                    state.update_settings(|settings| set(settings, value));
                }),
            )
        }))
}

fn option_button(
    title: &str,
    selected: bool,
    on_press: EventHandler<Event<PressEventData>>,
) -> Rect {
    rect()
        .padding((6., 12., 6., 12.))
        .corner_radius(7.)
        .background(if selected {
            Fill::from(DARK.accent)
        } else {
            crate::grain::hover()
        })
        .border(
            Border::new()
                .width(1.)
                .fill(if selected { DARK.accent } else { DARK.divider }),
        )
        .cursor(freya::winit::window::CursorIcon::Pointer)
        .on_press(on_press)
        .child(
            label()
                .text(title.to_string())
                .color(DARK.text)
                .font_size(12.),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_notification_overrides_respect_global_switch() {
        let mut settings = AppSettings::default();
        settings.set_account_notifications_enabled("personal", false);
        assert!(!settings.notifications_for_account("personal"));
        assert!(settings.notifications_for_account("work"));

        settings.notifications_enabled = false;
        settings.set_account_notifications_enabled("personal", true);
        assert!(!settings.notifications_for_account("personal"));
    }

    #[test]
    fn sidebar_account_order_handles_additions_removals_and_reordering() {
        let account_ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut settings = AppSettings {
            sidebar_account_order: vec!["b".to_string(), "removed".to_string()],
            ..AppSettings::default()
        };

        assert_eq!(
            settings.ordered_account_ids(&account_ids),
            vec!["b", "a", "c"]
        );

        settings.reorder_sidebar_accounts(&account_ids, "a", "c");
        assert_eq!(settings.sidebar_account_order, vec!["b", "c", "a"]);

        settings.reorder_sidebar_accounts(&account_ids, "c", "b");
        assert_eq!(settings.sidebar_account_order, vec!["c", "b", "a"]);
    }

    #[test]
    fn gravatar_can_load_while_message_images_are_blocked() {
        let mut settings = AppSettings::default();
        settings.block_remote_images = true;
        settings.show_gravatar = true;
        assert!(settings.gravatar_enabled());
    }

    #[test]
    fn sync_and_read_delay_options_use_the_supported_intervals() {
        assert_eq!(
            AUTO_SYNC_OPTIONS,
            &[
                (0, "Off"),
                (5, "5 sec"),
                (15, "15 sec"),
                (30, "30 sec"),
                (60, "1 min"),
                (300, "5 min"),
            ]
        );
        assert_eq!(
            MARK_READ_OPTIONS,
            &[
                (0, "Instant"),
                (500, "0.5 sec"),
                (1000, "1 sec"),
                (2000, "2 sec"),
                (3000, "3 sec"),
            ]
        );
        assert_eq!(
            AppSettings::default().auto_sync_interval(),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            AppSettings {
                auto_sync_seconds: 15,
                ..AppSettings::default()
            }
            .auto_sync_interval(),
            Some(Duration::from_secs(15))
        );
        assert_eq!(
            AppSettings {
                auto_sync_seconds: 0,
                ..AppSettings::default()
            }
            .auto_sync_interval(),
            None
        );
    }

    #[test]
    fn legacy_timing_preferences_are_migrated() {
        let settings =
            AppSettings::parse(br#"{"auto_sync_minutes":1,"mark_read_after_seconds":5}"#);
        assert_eq!(settings.auto_sync_seconds, 60);
        assert_eq!(settings.mark_read_after_millis, 3000);
    }

    #[test]
    fn legacy_notification_sound_boolean_and_unknown_choices_are_normalized() {
        assert_eq!(
            AppSettings::parse(br#"{"notification_sound":true}"#).notification_sound,
            NotificationSound::Classic
        );
        assert_eq!(
            AppSettings::parse(br#"{"notification_sound":false}"#).notification_sound,
            NotificationSound::Muted
        );
        assert_eq!(
            AppSettings::parse(br#"{"notification_sound":"missing"}"#).notification_sound,
            NotificationSound::Classic
        );
    }

    #[test]
    fn notification_sound_choices_round_trip_and_legacy_settings_remain_writable() {
        for &(sound, _) in NOTIFICATION_SOUND_OPTIONS {
            let json = serde_json::to_string(&sound).unwrap();
            assert_eq!(
                serde_json::from_str::<NotificationSound>(&json).unwrap(),
                sound
            );
        }

        let path = std::env::temp_dir().join(format!(
            "photon-mail-settings-legacy-sound-{}.json",
            std::process::id()
        ));
        fs::write(&path, br#"{"notification_sound":true}"#).unwrap();
        let settings = AppSettings {
            notification_sound: NotificationSound::Space,
            ..AppSettings::default()
        };
        settings.save_to_path(&path).unwrap();
        assert_eq!(AppSettings::load_from_path(&path), settings);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn settings_round_trip_and_refuse_to_overwrite_invalid_data() {
        let path =
            std::env::temp_dir().join(format!("photon-mail-settings-{}.json", std::process::id()));
        let mut settings = AppSettings::default();
        settings.auto_sync_seconds = 15;
        settings.show_gravatar = true;
        settings.save_to_path(&path).unwrap();
        assert_eq!(AppSettings::load_from_path(&path), settings);

        settings.auto_sync_seconds = 30;
        settings.save_to_path(&path).unwrap();
        assert_eq!(AppSettings::load_from_path(&path), settings);

        fs::write(&path, b"invalid").unwrap();
        assert_eq!(
            settings.save_to_path(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_frequencies_are_normalized_when_loaded() {
        let path = std::env::temp_dir().join(format!(
            "photon-mail-settings-invalid-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            br#"{"auto_sync_seconds":999,"mark_read_after_millis":999,"undo_send_seconds":999}"#,
        )
        .unwrap();
        let settings = AppSettings::load_from_path(&path);
        assert_eq!(settings.auto_sync_seconds, 300);
        assert_eq!(settings.mark_read_after_millis, 0);
        assert_eq!(settings.undo_send_seconds, 5);
        fs::remove_file(path).unwrap();
    }
}
