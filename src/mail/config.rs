//! Account persistence. Non-secret settings go to `<config dir>/PhotonMail/accounts.json`;
//! passwords go to the OS keyring (Secret Service / Keychain / Credential Manager) and
//! never touch disk in plain text.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "PhotonMail";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Stable id; also the keyring entry name.
    pub id: String,
    /// Display name shown in the sidebar.
    pub name: String,
    pub email: String,
    /// IMAP login (usually the email address).
    pub username: String,
    pub host: String,
    /// Implicit-TLS port (993 for nearly every provider).
    pub port: u16,
}

pub fn new_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("acct-{n:x}")
}

fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("PhotonMail"))
}

pub fn load_accounts() -> Vec<AccountConfig> {
    let Some(path) = dir().map(|d| d.join("accounts.json")) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_accounts(list: &[AccountConfig]) -> Result<(), String> {
    let dir = dir().ok_or_else(|| "no config directory on this system".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash can't leave a half-written file.
    let tmp = dir.join("accounts.json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join("accounts.json")).map_err(|e| e.to_string())
}

// These are blocking calls (D-Bus / OS APIs): run them inside `spawn_blocking`.

pub fn save_password(id: &str, password: &str) -> Result<(), String> {
    keyring::Entry::new(KEYRING_SERVICE, id)
        .and_then(|e| e.set_password(password))
        .map_err(|e| e.to_string())
}

pub fn load_password(id: &str) -> Result<String, String> {
    keyring::Entry::new(KEYRING_SERVICE, id)
        .and_then(|e| e.get_password())
        .map_err(|e| e.to_string())
}
