//! Account persistence. Non-secret settings go to `<config dir>/PhotonMail/accounts.json`;
//! passwords go to the OS keyring (Secret Service / Keychain / Credential Manager) and
//! never touch disk in plain text.

use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::theme::Rgb;

const KEYRING_SERVICE: &str = "PhotonMail";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Stable id; also the keyring entry name.
    pub id: String,
    /// Display name shown in the sidebar (account label).
    pub name: String,
    /// Accent color used for this account in the UI.
    #[serde(default)]
    pub color: Option<Rgb>,
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

#[derive(Deserialize)]
struct LegacyAccount {
    id: String,
    name: String,
    email: String,
    username: String,
    host: String,
    port: u16,
}

enum AccountFile {
    Current(Vec<AccountConfig>),
    Intermediate(Vec<IntermediateAccount>),
    Legacy(Vec<LegacyAccount>),
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

pub(crate) fn email_domain(email: &str) -> Option<&str> {
    let mut quoted = false;
    let mut escaped = false;
    let mut separator = None;

    for (index, character) in email.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
        } else if character == '@' && !quoted {
            if separator.is_some() {
                return None;
            }
            separator = Some(index);
        }
    }

    if quoted || escaped {
        return None;
    }
    let domain = email.get(separator? + 1..)?.trim();
    (!domain.is_empty()).then_some(domain)
}

/// Auto-discover both IMAP and SMTP host and port from an email address.
/// Returns `Some` when the domain is recognized, `None` for unknown domains.
///
/// This is a best-effort lookup against a built-in database of common providers.
/// It does not attempt to discover the password — that must always be provided by the user.
pub fn discover_settings(email: &str) -> Option<DiscoveredSettings> {
    let d = email_domain(email)?.to_ascii_lowercase();

    // Helper to match a domain against a list of known domains/suffixes
    let matches = |domains: &[&str]| -> bool {
        domains
            .iter()
            .any(|m| d == *m || d.ends_with(&format!(".{m}")))
    };

    // Check providers in order
    if matches(&["gmail.com", "googlemail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.gmail.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.gmail.com",
                port: 465,
            },
        });
    }
    // Microsoft / Outlook / Hotmail / Live
    if matches(&[
        "outlook.com",
        "hotmail.com",
        "live.com",
        "msn.com",
        "hotmail.co.uk",
        "outlook.co.uk",
        "live.co.uk",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "outlook.office365.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp-mail.outlook.com",
                port: 587,
            },
        });
    }
    // Microsoft 365 / Exchange Online
    if matches(&["office365.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "outlook.office365.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.office365.com",
                port: 587,
            },
        });
    }
    // Yahoo
    if matches(&[
        "yahoo.com",
        "yahoo.co.uk",
        "yahoo.ca",
        "yahoo.de",
        "yahoo.fr",
        "yahoo.in",
        "yahoo.co.id",
        "yahoo.com.au",
        "ymail.com",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.yahoo.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.yahoo.com",
                port: 465,
            },
        });
    }
    // iCloud
    if matches(&["icloud.com", "me.com", "mac.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.me.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.me.com",
                port: 587,
            },
        });
    }
    // Fastmail
    if matches(&["fastmail.com", "fastmail.fm"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.fastmail.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.fastmail.com",
                port: 465,
            },
        });
    }
    // Zoho Mail
    if matches(&["zoho.com", "zohomail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.zoho.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.zoho.com",
                port: 465,
            },
        });
    }
    // AOL
    if matches(&["aol.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.aol.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.aol.com",
                port: 465,
            },
        });
    }
    // Yandex
    if matches(&[
        "yandex.ru",
        "yandex.com",
        "yandex.ua",
        "yandex.kz",
        "yandex.by",
        "ya.ru",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.yandex.ru",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.yandex.ru",
                port: 465,
            },
        });
    }
    // Mail.ru
    if matches(&["mail.ru", "inbox.ru", "list.ru"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.ru",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.ru",
                port: 465,
            },
        });
    }
    // GMX
    if matches(&[
        "gmx.com",
        "gmx.de",
        "gmx.net",
        "gmx.fr",
        "gmx.at",
        "gmx.ch",
        "gmx.es",
        "gmx.co.uk",
        "gmx.ca",
        "gmx.com.au",
        "gmx.se",
        "gmx.nl",
        "gmx.cz",
        "gmx.hr",
        "gmx.lt",
        "gmx.no",
        "gmx.pl",
        "gmx.sk",
        "webmail.gmx.com",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.gmx.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "mail.gmx.com",
                port: 465,
            },
        });
    }
    // T-Online / Telekom (Germany)
    if matches(&["t-online.de", "telekom.de"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.t-online.de",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.t-online.de",
                port: 465,
            },
        });
    }
    // Web.de / Giga (Germany)
    if matches(&["web.de", "giga.de"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.web.de",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.web.de",
                port: 465,
            },
        });
    }
    // Outlook / Hotmail international variants
    if matches(&["outlook.cz", "outlook.hu", "outlook.sa"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "outlook.office365.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp-mail.outlook.com",
                port: 587,
            },
        });
    }
    // Mailfence provides standard IMAP/SMTP access on paid plans.
    if matches(&["mailfence.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mailfence.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mailfence.com",
                port: 465,
            },
        });
    }
    // QQ Mail
    if matches(&["qq.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.qq.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.qq.com",
                port: 465,
            },
        });
    }
    // 163 / 126 NetEase
    if matches(&["163.com", "126.com", "188.com", "yeah.net", "163mail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.163.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.163.com",
                port: 465,
            },
        });
    }
    // Amazon WorkMail
    if matches(&["awsworkmail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.awsworkmail.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.awsworkmail.com",
                port: 465,
            },
        });
    }
    // Mailbox.org
    if matches(&["mailbox.org", "mailbox.de"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mailbox.org",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mailbox.org",
                port: 465,
            },
        });
    }
    // Posteo serves all supported address domains from posteo.de.
    if matches(&[
        "posteo.de",
        "posteo.com",
        "posteo.net",
        "posteo.us",
        "posteo.ca",
        "posteo.uk",
        "posteo.ie",
        "posteo.es",
        "posteo.at",
        "posteo.ch",
        "posteo.nl",
        "posteo.be",
        "posteo.lu",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "posteo.de",
                port: 993,
            },
            smtp: ServerSettings {
                host: "posteo.de",
                port: 465,
            },
        });
    }
    // Runbox
    if matches(&["runbox.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "mail.runbox.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "mail.runbox.com",
                port: 465,
            },
        });
    }
    // Mail.com / Love.com etc (Migadu group)
    if matches(&[
        "mail.com",
        "love.com",
        "iname.com",
        "briefly.com",
        "acmemail.net",
        "binkmail.com",
        "einrot.com",
        "freetime.com",
        "hockeymail.com",
        "myfamily.com",
        "starpower.com",
        "tripos.com",
        "vollomail.com",
    ]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.com",
                port: 587,
            },
        });
    }
    // Apple iCloud+ Hide My Email
    if matches(&["icloud.privateemail.com"]) {
        return Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.mail.me.com",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.mail.me.com",
                port: 587,
            },
        });
    }

    None
}

/// Recognize provider-owned MX hosts without treating an inbound mail exchanger as a
/// client-accessible IMAP or SMTP server.
pub fn discover_settings_for_mx_host(mx_host: &str) -> Option<DiscoveredSettings> {
    let host = mx_host.trim_end_matches('.').to_ascii_lowercase();
    let provider_domain = if matches!(
        host.as_str(),
        "smtp.google.com"
            | "aspmx.l.google.com"
            | "alt1.aspmx.l.google.com"
            | "alt2.aspmx.l.google.com"
            | "alt3.aspmx.l.google.com"
            | "alt4.aspmx.l.google.com"
    ) {
        "gmail.com"
    } else if host.ends_with(".mail.protection.outlook.com") {
        "office365.com"
    } else if matches!(
        host.as_str(),
        "in1-smtp.messagingengine.com" | "in2-smtp.messagingengine.com"
    ) {
        "fastmail.com"
    } else if matches!(
        host.as_str(),
        "mxext1.mailbox.org" | "mxext2.mailbox.org" | "mxext3.mailbox.org"
    ) {
        "mailbox.org"
    } else if matches!(host.as_str(), "mx01.posteo.de" | "mx02.posteo.de") {
        "posteo.de"
    } else if host == "mx.transip.email" {
        "transip.email"
    } else {
        return None;
    };

    if provider_domain == "transip.email" {
        Some(DiscoveredSettings {
            imap: ServerSettings {
                host: "imap.transip.email",
                port: 993,
            },
            smtp: ServerSettings {
                host: "smtp.transip.email",
                port: 465,
            },
        })
    } else {
        discover_settings(&format!("user@{provider_domain}"))
    }
}

pub fn unsupported_provider_message(domain: &str) -> Option<&'static str> {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if ["protonmail.com", "proton.me", "protonmail.ch", "pm.me"]
        .iter()
        .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
    {
        Some("Proton Mail requires Proton Mail Bridge; direct server discovery is not supported.")
    } else if [
        "tutanota.com",
        "tutanota.de",
        "tutamail.com",
        "tuta.com",
        "tuta.io",
        "keemail.me",
    ]
    .iter()
    .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
    {
        Some("Tuta does not provide standard IMAP or SMTP access.")
    } else if ["hey.com", "skiff.com"]
        .iter()
        .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
    {
        Some("This provider does not offer a supported standard IMAP/SMTP endpoint.")
    } else {
        None
    }
}

fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("PhotonMail"))
}

pub fn load_accounts() -> Vec<AccountConfig> {
    let Some(path) = dir().map(|d| d.join("accounts.json")) else {
        return Vec::new();
    };
    load_accounts_from_path(&path)
}

pub fn save_accounts(list: &[AccountConfig]) -> Result<(), String> {
    let dir = dir().ok_or_else(|| "no config directory on this system".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    save_accounts_at(&dir.join("accounts.json"), list)
}

fn parse_account_file(raw: &str) -> Option<AccountFile> {
    if let Ok(list) = serde_json::from_str::<Vec<AccountConfig>>(raw) {
        return Some(AccountFile::Current(list));
    }
    if let Ok(list) = serde_json::from_str::<Vec<IntermediateAccount>>(raw) {
        return Some(AccountFile::Intermediate(list));
    }
    serde_json::from_str::<Vec<LegacyAccount>>(raw)
        .ok()
        .map(AccountFile::Legacy)
}

fn load_accounts_from_path(path: &Path) -> Vec<AccountConfig> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            match preserve_corrupt_accounts(path) {
                Ok(backup) => eprintln!(
                    "Unreadable account settings preserved as {}",
                    backup.display()
                ),
                Err(backup_error) => eprintln!(
                    "Unreadable account settings were not overwritten because backup failed: {backup_error}"
                ),
            }
            return Vec::new();
        }
        Err(error) => {
            eprintln!("Could not read account settings: {error}");
            return Vec::new();
        }
    };

    match parse_account_file(&raw) {
        Some(AccountFile::Current(list)) => list,
        Some(AccountFile::Intermediate(list)) => {
            let migrated: Vec<AccountConfig> = list
                .into_iter()
                .map(|account| AccountConfig {
                    id: account.id,
                    name: account.name.clone(),
                    color: None,
                    sender_name: account.name,
                    email: account.email,
                    username: account.username,
                    host: account.host,
                    port: account.port,
                    smtp_host: account.smtp_host,
                    smtp_port: account.smtp_port,
                })
                .collect();
            let _ = save_accounts_at(path, &migrated);
            migrated
        }
        Some(AccountFile::Legacy(list)) => {
            let migrated: Vec<AccountConfig> = list
                .into_iter()
                .map(|account| {
                    let (smtp_host, smtp_port) = match discover_settings(&account.email) {
                        Some(settings) => (settings.smtp.host.to_string(), settings.smtp.port),
                        None => (String::new(), 0),
                    };
                    AccountConfig {
                        id: account.id,
                        name: account.name.clone(),
                        color: None,
                        sender_name: account.name,
                        email: account.email,
                        username: account.username,
                        host: account.host,
                        port: account.port,
                        smtp_host,
                        smtp_port,
                    }
                })
                .collect();
            let _ = save_accounts_at(path, &migrated);
            migrated
        }
        None => {
            match preserve_corrupt_accounts(path) {
                Ok(backup) => {
                    eprintln!("Invalid account settings preserved as {}", backup.display())
                }
                Err(error) => eprintln!(
                    "Invalid account settings were not overwritten because backup failed: {error}"
                ),
            }
            Vec::new()
        }
    }
}

fn save_accounts_at(path: &Path, list: &[AccountConfig]) -> Result<(), String> {
    if path.exists() {
        let current = fs::read_to_string(path).map_err(|error| error.to_string())?;
        if parse_account_file(&current).is_none() {
            return Err(
                "existing account settings are invalid; refusing to overwrite them".to_string(),
            );
        }
    }

    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash can't leave a half-written file.
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn preserve_corrupt_accounts(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "account settings path has no parent",
        )
    })?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for suffix in 0..100 {
        let backup = parent.join(format!("{file_name}.corrupt-{timestamp}-{suffix}"));
        let mut destination = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let copy_result = (|| {
            let mut source = fs::File::open(path)?;
            io::copy(&mut source, &mut destination)?;
            destination.sync_all()
        })();
        if let Err(error) = copy_result {
            drop(destination);
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        drop(destination);
        fs::remove_file(path)?;
        return Ok(backup);
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique account-settings backup name",
    ))
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

pub fn delete_password(id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_colors_are_optional_for_existing_configs_and_persist_when_set() {
        let raw = r#"{
            "id":"account-1","name":"Personal","sender_name":"Person",
            "email":"person@example.com","username":"person@example.com",
            "host":"imap.example.com","port":993,
            "smtp_host":"smtp.example.com","smtp_port":587
        }"#;
        let mut account: AccountConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(account.color, None);

        account.color = Some((10, 20, 30));
        let serialized = serde_json::to_string(&account).unwrap();
        let restored: AccountConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.color, Some((10, 20, 30)));
    }

    #[test]
    fn provider_settings_use_submission_ports_and_hosts() {
        let fastmail = discover_settings("user@fastmail.com").unwrap();
        assert_eq!(fastmail.smtp.host, "smtp.fastmail.com");
        assert_eq!(fastmail.smtp.port, 465);

        let mailbox = discover_settings("user@mailbox.org").unwrap();
        assert_eq!(mailbox.smtp.host, "smtp.mailbox.org");
        assert_eq!(mailbox.smtp.port, 465);

        let runbox = discover_settings("user@runbox.com").unwrap();
        assert_eq!(runbox.smtp.host, "mail.runbox.com");
        assert_eq!(runbox.smtp.port, 465);

        let posteo = discover_settings("user@posteo.de").unwrap();
        assert_eq!(posteo.imap.host, "posteo.de");
        assert_eq!(posteo.smtp.host, "posteo.de");
        assert_eq!(posteo.smtp.port, 465);

        let mailfence = discover_settings("user@mailfence.com").unwrap();
        assert_eq!(mailfence.imap.host, "imap.mailfence.com");
        assert_eq!(mailfence.smtp.host, "smtp.mailfence.com");
        assert_eq!(mailfence.smtp.port, 465);
    }

    #[test]
    fn provider_matching_handles_quoted_at_signs_and_normalized_domains() {
        assert!(discover_settings("\"local@part\"@fastmail.com").is_some());
        assert!(discover_settings("user@Vollomail.com").is_some());
        assert!(discover_settings("user@posteo.us").is_some());
        assert!(
            email_domain("\"local@part\"@fastmail.com")
                .is_some_and(|domain| domain == "fastmail.com")
        );
        assert!(email_domain("user@extra@example.com").is_none());
    }

    #[test]
    fn unsupported_or_unrelated_domains_are_not_mapped_to_other_providers() {
        assert!(discover_settings("user@transip.nl").is_none());
        assert!(discover_settings("user@proton.me").is_none());
        assert!(discover_settings("user@tuta.com").is_none());
        assert!(discover_settings("user@tutanota.de").is_none());
        assert!(discover_settings("user@keemail.me").is_none());
        assert!(discover_settings("user@hey.com").is_none());
        assert!(discover_settings("user@skiff.com").is_none());
        assert!(discover_settings("user@outlook_eg.com").is_none());
    }

    #[test]
    fn mx_provider_mapping_never_uses_an_inbound_relay_as_a_client_server() {
        let google = discover_settings_for_mx_host("ALT1.ASPMX.L.GOOGLE.COM.").unwrap();
        assert_eq!(google.imap.host, "imap.gmail.com");
        assert_eq!(google.smtp.host, "smtp.gmail.com");

        let workspace = discover_settings_for_mx_host("smtp.google.com.").unwrap();
        assert_eq!(workspace.imap.host, "imap.gmail.com");

        let transip = discover_settings_for_mx_host("mx.transip.email.").unwrap();
        assert_eq!(transip.imap.host, "imap.transip.email");
        assert_eq!(transip.smtp.host, "smtp.transip.email");

        assert!(discover_settings_for_mx_host("aspmx.l.google.com.attacker.test").is_none());
        assert!(discover_settings_for_mx_host("unrecognized-mx.example.test").is_none());
    }

    fn temporary_accounts_file() -> (PathBuf, PathBuf) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "photonmail-config-test-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("accounts.json");
        (directory, path)
    }

    #[test]
    fn corrupt_account_settings_are_backed_up_before_empty_state_can_be_saved() {
        let (directory, path) = temporary_accounts_file();
        let corrupt = b"{ invalid account data";
        fs::write(&path, corrupt).unwrap();

        assert!(load_accounts_from_path(&path).is_empty());
        assert!(!path.exists());
        let backup = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|candidate| candidate.to_string_lossy().contains(".corrupt-"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), corrupt);

        save_accounts_at(&path, &[]).unwrap();
        assert!(fs::read_to_string(path).unwrap().contains("[]"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saving_refuses_to_replace_unreadable_account_settings() {
        let (directory, path) = temporary_accounts_file();
        let corrupt = [0xff, 0xfe, 0xfd];
        fs::write(&path, corrupt).unwrap();

        assert!(save_accounts_at(&path, &[]).is_err());
        assert_eq!(fs::read(&path).unwrap(), corrupt);
        fs::remove_dir_all(directory).unwrap();
    }
}
