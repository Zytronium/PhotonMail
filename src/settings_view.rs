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

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub block_tracking_pixels: bool,
    pub block_remote_images: bool,
    pub show_gravatar: bool,
    pub auto_sync_seconds: u32,
    pub notifications_enabled: bool,
    disabled_notification_accounts: HashSet<String>,
    pub notification_sound: bool,
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
            notification_sound: true,
            mark_read_after_millis: 0,
            undo_send_seconds: 5,
        }
    }
}

impl AppSettings {
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
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return Self::default();
        };
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
        }

        let mut settings: Self = serde_json::from_value(value).unwrap_or_default();
        if ![0, 5, 15, 30, 60, 300].contains(&settings.auto_sync_seconds) {
            settings.auto_sync_seconds = 300;
        }
        if ![0, 500, 1000, 2000, 3000].contains(&settings.mark_read_after_millis) {
            settings.mark_read_after_millis = 0;
        }
        if ![0, 5, 10, 15].contains(&settings.undo_send_seconds) {
            settings.undo_send_seconds = 5;
        }
        settings
    }

    fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if path.exists() {
            let existing = fs::read(path)?;
            if serde_json::from_slice::<Self>(&existing).is_err() {
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
            toggle_row(
                "Notification sound",
                "Play the bundled classic sound with desktop notifications.",
                settings.notification_sound,
                EventHandler::new({
                    let mut state = state;
                    move |_| {
                        state.update_settings(|settings| {
                            settings.notification_sound = !settings.notification_sound;
                        });
                    }
                }),
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
                ScrollView::new().height(Size::fill()).children([rect()
                    .vertical()
                    .spacing(14.)
                    .children(
                        rows.into_iter()
                            .enumerate()
                            .map(|(index, row)| row.key(index)),
                    )]),
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
