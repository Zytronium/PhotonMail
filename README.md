# Photon Mail

Browse and send email at light speed with Photon Mail. A fast, native desktop 
email client built with pure Rust.

## Features

- **Amazing lightweight UI** - polished dark theme with resizable panes, keyboard navigation, search, and unread filtering
- **View and send emails** - real-time sync with IDLE, secure HTML rendering, rich compose editor, auto-discovery for popular providers, and credential storage in the OS keyring
- **IMAP accounts** - sign in to your IMAP account
- **Gmail accounts** - sign in with Google using OAuth 2.0; access tokens are refreshed automatically and never stored in the account settings file
- **Multiple accounts** - manage several email accounts in one window with a unified inbox view
- **New mail notifications and automatic sync** - desktop alerts for new unread messages, F5 resync, five-minute background refresh, and close-to-tray support on Linux, Windows, and macOS
- **More coming soon** - Photon Mail is early in development and is getting new features frequently. Stay tuned!

## Screenshots

**Main View**
![Photon Mail main view](readme-assets/main.png)

**Email Composer**
![Compose window](readme-assets/compose.png)

## Building

### Build Requirements

- Rust 1.90 or later
- A C compiler (for Skia/freya build dependencies)
- On Linux: `dbus` package (for keyring password storage via Secret Service)

```bash
git clone https://github.com/Zytronium/PhotonMail.git
cd PhotonMail
cargo run --release
```

## Configuration

Account settings are stored in `<config dir>/PhotonMail/accounts.json`. Passwords are stored in the OS keyring under the `PhotonMail` service and are never persisted in plain text.

Gmail sign-in uses a Google OAuth client ID for a **Desktop app**, bundled directly with Photon Mail; no environment-variable or build-time configuration is needed. The OAuth consent screen must include the `openid`, `email`, and `https://mail.google.com/` scopes. Google classifies full-mail access as restricted and requires OAuth app verification for public use; this scope is needed because Photon Mail supports permanently deleting messages without moving them to Trash. Photon Mail uses the loopback redirect flow with PKCE, stores the refresh token in the OS keyring, and requests short-lived access tokens for Gmail's IMAP/SMTP XOAUTH2 authentication. See Google's [desktop-app OAuth guide](https://developers.google.com/identity/protocols/oauth2/native-app) and [Gmail XOAUTH2 protocol](https://developers.google.com/workspace/gmail/imap/xoauth2-protocol).

## Architecture

Photon Mail uses a two-thread architecture: the UI runs on Freya's main thread, while all IMAP I/O runs on a dedicated Tokio runtime. The UI communicates with the backend through an `async-channel` command/event pipeline, one worker per account.

```text
  UI thread (Freya)                       "photon-mail-io" thread (tokio)
  --------------------------------        --------------------------------
  AppState -- Command ----------------->  manager --> Worker (1 per account)
     ^                                                   |  sync, bodies, flags, moves
     +---------------- Event (async_channel) <--         |
                                                         + idle task: IMAP IDLE on INBOX
```

## Security

HTML emails are sanitized before rendering:

- `<script>` tags and `on*` event attributes are removed
- `javascript:` URLs are blocked
- Tracking pixels (1x1 images, invisible iframes, known tracking URLs) are stripped
- External link clicks show a confirmation dialog with a "do not show again" option per sender

## License

MIT License. Copyright © 2026 Zytronium. See [LICENSE](LICENSE) for details.
