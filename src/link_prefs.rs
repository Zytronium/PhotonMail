//! Link click preference management.
//!
//! Tracks "do not show again" preferences for external link clicks.
//! Currently stores preferences in memory for the session only.
//! Designed to be extended later with file-based persistence (e.g., JSON config).

use std::collections::HashSet;

/// Where preferences would be persisted between sessions.
/// Not used yet — reserved for future file-based storage.
pub const PREFS_PATH: &str = "~/.config/PhotonMail/link_prefs.json";

/// Session-scoped link click preferences.
///
/// Stores which senders and whether globally the user has chosen
/// to skip the confirmation dialog for external links.
///
/// Future: serialize/deserialize from `prefs_path()` on app start/exit.
#[derive(Clone, Default)]
pub struct LinkPrefs {
    /// Sender email addresses that should skip the confirmation dialog.
    pub trusted_senders: HashSet<String>,
    /// If true, never show the confirmation dialog for any sender.
    pub always_open: bool,
}

impl LinkPrefs {
    /// Check whether the confirmation dialog should be shown for a given sender.
    /// Returns `true` if we should skip the dialog and open directly.
    pub fn should_skip(&self, sender_email: &str) -> bool {
        self.always_open || self.trusted_senders.contains(sender_email)
    }

    /// Mark a sender as trusted (skip dialog for this sender).
    pub fn trust_sender(&mut self, sender_email: &str) {
        self.trusted_senders.insert(sender_email.to_string());
    }

    /// Mark that the user never wants to see the dialog again.
    pub fn set_always_open(&mut self) {
        self.always_open = true;
    }

    /// Save preferences to disk for future sessions.
    /// Currently a no-op — placeholder for future persistence.
    pub fn save_to_disk(&self) {
        // TODO: serialize self to PREFS_PATH as JSON
        // For now, preferences are session-only.
        let _ = PREFS_PATH;
    }

    /// Load preferences from disk.
    /// Currently returns a fresh default — placeholder for future persistence.
    pub fn load_from_disk() -> Self {
        // TODO: deserialize from prefs_path()
        // For now, start with empty preferences each session.
        Self::default()
    }
}
