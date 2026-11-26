use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use serde::{Deserialize, Serialize};

use crate::data::MailContentType;

const CACHE_VERSION: u8 = 2;

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct EmailCache {
    version: u8,
    pub emails: Vec<CachedEmail>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CachedEmail {
    pub account_id: String,
    pub folder: usize,
    pub uid: u32,
    pub unread: bool,
    pub from: String,
    pub from_email: String,
    pub to: String,
    pub to_email: String,
    pub cc_email: String,
    pub bcc_email: String,
    pub reply_to: String,
    pub subject: String,
    pub preview: String,
    pub body: Option<CachedBody>,
    pub date: String,
    pub ts: i64,
    pub message_id: String,
    pub in_reply_to: String,
    pub references: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CachedBody {
    pub content_type: MailContentType,
    pub text: String,
    pub source: Vec<u8>,
    pub attachments: Vec<CachedAttachment>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CachedAttachment {
    pub name: String,
    pub size: usize,
    pub content_type: String,
    pub data: Option<Vec<u8>>,
    pub content_id: Option<String>,
}

impl EmailCache {
    pub fn new(emails: Vec<CachedEmail>) -> Self {
        Self {
            version: CACHE_VERSION,
            emails,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CacheWriter {
    tx: mpsc::Sender<EmailCache>,
}

impl CacheWriter {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<EmailCache>();
        thread::Builder::new()
            .name("photon-mail-cache".into())
            .spawn(move || {
                while let Ok(cache) = rx.recv() {
                    if let Err(error) = save_to_disk(&cache) {
                        eprintln!("Could not save email cache: {error}");
                    }
                }
            })
            .expect("failed to spawn the email cache writer");
        Self { tx }
    }

    pub fn save(&self, cache: EmailCache) {
        let _ = self.tx.send(cache);
    }
}

pub fn load_async() -> async_channel::Receiver<EmailCache> {
    let (tx, rx) = async_channel::bounded(1);
    thread::Builder::new()
        .name("photon-mail-cache-load".into())
        .spawn(move || {
            let _ = tx.send_blocking(load_from_disk());
        })
        .expect("failed to spawn the email cache loader");
    rx
}

fn load_from_disk() -> EmailCache {
    let Some(path) = cache_path() else {
        return EmailCache::default();
    };
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<EmailCache>(&bytes).ok())
        .filter(|cache| cache.version == CACHE_VERSION)
        .unwrap_or_default()
}

fn save_to_disk(cache: &EmailCache) -> io::Result<()> {
    let Some(path) = cache_path() else {
        return Ok(());
    };
    save_to_path(cache, &path)
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|directory| directory.join("PhotonMail").join("emails.json"))
}

fn save_to_path(cache: &EmailCache, path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "email cache path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let contents = serde_json::to_vec(cache).map_err(io::Error::other)?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)
}
