//! Link click preference management.
//!
//! Persists the user's explicit choice to bypass external-link confirmations.

use std::{
    collections::HashSet,
    fs,
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Persisted preference for whether external links skip confirmation.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct LinkPrefs {
    /// Sender addresses for which the user chose to skip link confirmations.
    pub trusted_senders: HashSet<String>,
    /// If true, never show the confirmation dialog for any sender.
    pub always_open: bool,
}

impl LinkPrefs {
    /// Check whether the confirmation dialog should be shown.
    pub fn should_skip(&self, sender_email: &str) -> bool {
        self.always_open || self.trusted_senders.contains(&normalize_sender(sender_email))
    }

    /// Remember the sender-specific choice made in the confirmation dialog.
    pub fn trust_sender(&mut self, sender_email: &str) {
        let sender = normalize_sender(sender_email);
        if !sender.is_empty() {
            self.trusted_senders.insert(sender);
        }
    }

    /// Mark that the user never wants to see the dialog again.
    pub fn set_always_open(&mut self) {
        self.always_open = true;
    }

    /// Save preferences to disk for future sessions.
    pub fn save_to_disk(&self) {
        if let Some(path) = prefs_path()
            && let Err(error) = self.save_to_path(&path)
        {
            eprintln!("Could not save link preferences: {error}");
        }
    }

    /// Load preferences from disk.
    pub fn load_from_disk() -> Self {
        prefs_path().map_or_else(Self::default, |path| Self::load_from_path(&path))
    }

    fn load_from_path(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if path.exists() {
            let existing = fs::read(path)?;
            if serde_json::from_slice::<Self>(&existing).is_err() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "existing link preferences are invalid; refusing to overwrite them"));
            }
        }

        let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "link preferences path has no parent"))?;
        fs::create_dir_all(parent)?;
        let contents = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let temporary = path.with_file_name(format!("{file_name}.tmp"));
        fs::write(&temporary, contents)?;
        fs::rename(temporary, path)
    }
}

fn prefs_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("PhotonMail").join("link_prefs.json"))
}

fn normalize_sender(sender_email: &str) -> String {
    sender_email.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn link_confirmation_preference_persists_between_loads() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("photon-mail-link-prefs-{unique}"));
        let path = directory.join("link_prefs.json");
        let mut prefs = LinkPrefs::default();
        prefs.set_always_open();
        prefs.save_to_path(&path).unwrap();

        assert!(LinkPrefs::load_from_path(&path).should_skip("person@example.com"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn sender_preference_is_persisted_and_scoped_case_insensitively() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("photon-mail-link-prefs-{unique}"));
        let path = directory.join("link_prefs.json");
        let mut prefs = LinkPrefs::default();
        prefs.trust_sender(" Person@Example.com ");
        prefs.save_to_path(&path).unwrap();

        let loaded = LinkPrefs::load_from_path(&path);
        assert!(loaded.should_skip("person@example.com"));
        assert!(!loaded.should_skip("someone-else@example.com"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_existing_preferences_are_not_overwritten() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("photon-mail-link-prefs-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("link_prefs.json");
        fs::write(&path, b"invalid").unwrap();

        let result = LinkPrefs {
            always_open: true,
            ..LinkPrefs::default()
        }
        .save_to_path(&path);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).unwrap(), b"invalid");
        fs::remove_dir_all(directory).unwrap();
    }
}
