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
    /// Display name shown in the sidebar (account label).
    pub name: String,
    /// Sender display name — the name recipients see in the From header.
    pub sender_name: String,
    pub email: String,
    /// IMAP login (usually the email address).
    pub username: String,
    /// IMAP host.
    pub host: String,
    /// IMAP port: implicit TLS on 993, or STARTTLS on 143.
    pub port: u16,
    /// SMTP host (auto-discovered from email domain for popular providers).
    pub smtp_host: String,
    /// SMTP submission port: implicit TLS on 465, or STARTTLS on 587.
    pub smtp_port: u16,
}

pub fn new_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("acct-{n:x}")
}

/// Server settings for a single email protocol.
#[derive(Clone, Copy, Debug)]
pub struct ServerSettings {
    pub host: &'static str,
    pub port: u16,
}

/// Auto-discovered server settings for both IMAP and SMTP.
#[derive(Clone, Debug)]
pub struct DiscoveredSettings {
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
}

/// Auto-discover both IMAP and SMTP host and port from an email address.
/// Returns `Some` when the domain is recognized, `None` for unknown domains.
///
/// This is a best-effort lookup against a built-in database of common providers.
/// It does not attempt to discover the password — that must always be provided by the user.
pub fn discover_settings(email: &str) -> Option<DiscoveredSettings> {
    let domain = email.split('@').nth(1)?;
    let d = domain.to_lowercase();

    // Helper to match a domain against a list of known domains/suffixes
    let matches = |domains: &[&str]| -> bool {
        domains.iter().any(|m| d == *m || d.ends_with(&format!(".{m}")))
    };

    // Check providers in order
    if matches(&["gmail.com", "googlemail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.gmail.com", port: 993 },
            smtp: ServerSettings { host: "smtp.gmail.com", port: 465 },
        });
    }
    // Microsoft / Outlook / Hotmail / Live
    if matches(&["outlook.com", "hotmail.com", "live.com", "msn.com", "hotmail.co.uk", "outlook.co.uk", "live.co.uk"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "outlook.office365.com", port: 993 },
            smtp: ServerSettings { host: "smtp.office365.com", port: 587 },
        });
    }
    // Microsoft 365 / Exchange Online
    if matches(&["office365.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "outlook.office365.com", port: 993 },
            smtp: ServerSettings { host: "smtp.office365.com", port: 587 },
        });
    }
    // Yahoo
    if matches(&["yahoo.com", "yahoo.co.uk", "yahoo.ca", "yahoo.de", "yahoo.fr", "yahoo.in", "yahoo.co.id", "yahoo.com.au", "ymail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.yahoo.com", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.yahoo.com", port: 465 },
        });
    }
    // iCloud
    if matches(&["icloud.com", "me.com", "mac.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.me.com", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.me.com", port: 587 },
        });
    }
    // Proton Mail
    if matches(&["protonmail.com", "proton.me"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "mail.proton.me", port: 993 },
            smtp: ServerSettings { host: "smtp.proton.me", port: 465 },
        });
    }
    // Fastmail
    if matches(&["fastmail.com", "fastmail.fm", "gmox.com", "gotdns.ch", "gotmail.com", "gotmail.fr", "mailfence.com", "pcpostal.com", "premium-mail.com", "sendme.space", "telesoft.md", "telfort.nl", "transip.nl"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.fastmail.com", port: 993 },
            smtp: ServerSettings { host: "smtp.fastmail.com", port: 993 },
        });
    }
    // Zoho Mail
    if matches(&["zoho.com", "zohomail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.zoho.com", port: 993 },
            smtp: ServerSettings { host: "smtp.zoho.com", port: 465 },
        });
    }
    // AOL
    if matches(&["aol.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.aol.com", port: 993 },
            smtp: ServerSettings { host: "smtp.aol.com", port: 465 },
        });
    }
    // Yandex
    if matches(&["yandex.ru", "yandex.com", "yandex.ua", "yandex.kz", "yandex.by", "ya.ru"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.yandex.ru", port: 993 },
            smtp: ServerSettings { host: "smtp.yandex.ru", port: 465 },
        });
    }
    // Mail.ru
    if matches(&["mail.ru", "inbox.ru", "list.ru"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.ru", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.ru", port: 465 },
        });
    }
    // GMX
    if matches(&["gmx.com", "gmx.de", "gmx.net", "gmx.fr", "gmx.at", "gmx.ch", "gmx.es", "gmx.co.uk", "gmx.ca", "gmx.com.au", "gmx.se", "gmx.nl", "gmx.cz", "gmx.hr", "gmx.lt", "gmx.no", "gmx.pl", "gmx.sk", "webmail.gmx.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.gmx.com", port: 993 },
            smtp: ServerSettings { host: "mail.gmx.com", port: 465 },
        });
    }
    // T-Online / Telekom (Germany)
    if matches(&["t-online.de", "telekom.de"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.t-online.de", port: 993 },
            smtp: ServerSettings { host: "smtp.t-online.de", port: 465 },
        });
    }
    // Web.de / Giga (Germany)
    if matches(&["web.de", "giga.de"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.web.de", port: 993 },
            smtp: ServerSettings { host: "smtp.web.de", port: 465 },
        });
    }
    // Outlook / Hotmail international variants
    if matches(&["outlook.cz", "outlook.hu", "outlook.sa", "outlook_eg.com", "outlook_ph.com", "outlook_ps.com", "outlook_qa.com", "outlook_th.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "outlook.office365.com", port: 993 },
            smtp: ServerSettings { host: "smtp.office365.com", port: 587 },
        });
    }
    // QQ Mail
    if matches(&["qq.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.qq.com", port: 993 },
            smtp: ServerSettings { host: "smtp.qq.com", port: 465 },
        });
    }
    // 163 / 126 NetEase
    if matches(&["163.com", "126.com", "188.com", "yeah.net", "163mail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.163.com", port: 993 },
            smtp: ServerSettings { host: "smtp.163.com", port: 465 },
        });
    }
    // Amazon WorkMail
    if matches(&["awsworkmail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.awsworkmail.com", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.awsworkmail.com", port: 465 },
        });
    }
    // Mailbox.org / Posteo
    if matches(&["mailbox.org", "mailbox.de", "posteo.de", "posteo.eu"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mailbox.org", port: 993 },
            smtp: ServerSettings { host: "smtp.mailbox.org", port: 993 },
        });
    }
    // Runbox
    if matches(&["runbox.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.runbox.com", port: 993 },
            smtp: ServerSettings { host: "smtp.runbox.com", port: 993 },
        });
    }
    // Hey.com
    if matches(&["hey.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.hey.com", port: 993 },
            smtp: ServerSettings { host: "smtp.hey.com", port: 465 },
        });
    }
    // Tutanota / Tuta
    if matches(&["tutanota.com", "tuta.com", "tuta.io"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "mail.tuta.com", port: 993 },
            smtp: ServerSettings { host: "mail.tuta.com", port: 465 },
        });
    }
    // Skiff
    if matches(&["skiff.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "mail.skiff.com", port: 993 },
            smtp: ServerSettings { host: "smtp.skiff.com", port: 465 },
        });
    }
    // Mail.com / Love.com etc (Migadu group)
    if matches(&["mail.com", "love.com", "iname.com", "briefly.com", "acmemail.net", "binkmail.com", "einrot.com", "freetime.com", "hockeymail.com", "myfamily.com", "starpower.com", "tripos.com", "volloMail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.com", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.com", port: 587 },
        });
    }
    // Apple iCloud+ Hide My Email
    if matches(&["icloud.privateemail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings { host: "imap.mail.me.com", port: 993 },
            smtp: ServerSettings { host: "smtp.mail.me.com", port: 587 },
        });
    }

    None
}


fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("PhotonMail"))
}

pub fn load_accounts() -> Vec<AccountConfig> {
    let Some(path) = dir().map(|d| d.join("accounts.json")) else {
        return Vec::new();
    };
    let raw = fs::read_to_string(path).ok();
    // Legacy format: array of objects without smtp_host/smtp_port or sender_name.
    // Migrate by auto-discovering SMTP settings from the email domain.
    if let Some(s) = &raw {
        // Try the current format first
        if let Ok(list) = serde_json::from_str::<Vec<AccountConfig>>(s) {
            return list;
        }
        // Fall back to intermediate format: has smtp_host/smtp_port but no sender_name
        #[derive(Deserialize)]
        struct IntermediateAccount {
            id: String,
            name: String,
            email: String,
            username: String,
            host: String,
            port: u16,
            smtp_host: String,
            smtp_port: u16,
        }
        if let Ok(list) = serde_json::from_str::<Vec<IntermediateAccount>>(s) {
            let migrated: Vec<AccountConfig> = list
                .into_iter()
                .map(|a| AccountConfig {
                    id: a.id,
                    name: a.name.clone(),
                    sender_name: a.name,
                    email: a.email,
                    username: a.username,
                    host: a.host,
                    port: a.port,
                    smtp_host: a.smtp_host,
                    smtp_port: a.smtp_port,
                })
                .collect();
            // Save the migrated format so we don't do this again
            let _ = save_accounts(&migrated);
            return migrated;
        }
        // Fall back to oldest legacy format (no SMTP fields, no sender_name)
        #[derive(Deserialize)]
        struct LegacyAccount {
            id: String,
            name: String,
            email: String,
            username: String,
            host: String,
            port: u16,
        }
        if let Ok(list) = serde_json::from_str::<Vec<LegacyAccount>>(s) {
            let migrated: Vec<AccountConfig> = list
                .into_iter()
                .map(|a| {
                    let (smtp_host, smtp_port) = match discover_settings(&a.email) {
                        Some(s) => (s.smtp.host.to_string(), s.smtp.port),
                        None => (String::new(), 0),
                    };
                    AccountConfig {
                        id: a.id,
                        name: a.name.clone(),
                        sender_name: a.name,
                        email: a.email.clone(),
                        username: a.username,
                        host: a.host,
                        port: a.port,
                        smtp_host,
                        smtp_port,
                    }
                })
                .collect();
            // Save the migrated format so we don't do this again
            let _ = save_accounts(&migrated);
            return migrated;
        }
    }
    Vec::new()
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
