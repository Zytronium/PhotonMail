//! Sound effects for Freagle Mail (AMERICA MODE).
//!
//! MP3 sound files are in `assets/sounds/`.
//!
//! Uses rodio's `play_raw` API which spawns its own thread per sound.

use std::sync::Arc;
use rodio::Source;

/// Pre-loaded sound clip data (just bytes, no rodio types to avoid Send/Sync issues).
pub struct SoundEffects {
    /// Bald eagle screech — played on send.
    eagle: Arc<Vec<u8>>,
    /// Firework explosion — played on click / random July fireworks.
    firework: Arc<Vec<u8>>,
    /// Notification chime — played when new mail arrives.
    notification: Arc<Vec<u8>>,
    /// Startup sound — played on app launch.
    startup: Arc<Vec<u8>>,
}

impl SoundEffects {
    /// Load all sound files from the assets directory.
    pub fn new() -> Self {
        Self {
            eagle: Arc::new(include_bytes!("../assets/sounds/eagle.mp3").to_vec()),
            firework: Arc::new(include_bytes!("../assets/sounds/firework.mp3").to_vec()),
            notification: Arc::new(include_bytes!("../assets/sounds/notification.mp3").to_vec()),
            startup: Arc::new(include_bytes!("../assets/sounds/startup.mp3").to_vec()),
        }
    }

    fn play(&self, data: &[u8]) {
        // Use rodio's simple play API — it handles its own thread/stream internally
        let data = data.to_vec();
        std::thread::spawn(move || {
            if let Ok((_stream, handle)) = rodio::OutputStream::try_default() {
                let cursor = std::io::Cursor::new(data);
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    let _ = handle.play_raw(source.convert_samples());
                    // Keep the stream alive until playback finishes
                    std::mem::forget(_stream);
                }
            }
        });
    }

    /// Play the bald eagle screech (on send).
    pub fn play_eagle(&self) {
        self.play(&self.eagle);
    }

    /// Play a firework explosion.
    pub fn play_firework(&self) {
        self.play(&self.firework);
    }

    /// Play the notification chime.
    pub fn play_notification(&self) {
        self.play(&self.notification);
    }

    /// Play the startup sound.
    pub fn play_startup(&self) {
        self.play(&self.startup);
    }
}
