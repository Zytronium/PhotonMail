use std::path::{Path, PathBuf};

use freya::prelude::*;
use freya::text_edit::{
    EditableConfig,
    EditableEvent,
    EditorLine,
    TextEditor,
    UseEditable,
    use_editable,
};
use freya_components::input::InputColorsThemePartial;

use crate::{
    html_viewer::SecureHtmlViewer,
    icons::{CHEVRON_DOWN, CHEVRON_LEFT, DISK, SEND},
    state::{AppState, ComposePre},
    theme::DARK,
    widgets::{divider, icon, IconButton},
};

/// Threading headers extracted from ComposePre before it's consumed.
#[derive(Clone, Default)]
struct ComposePreThreading {
    original_message_id: String,
    original_in_reply_to: String,
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Format flags for a single character.
/// `link` is an index into the links table (keeps this type `Copy`).
#[derive(Clone, Copy, PartialEq, Default)]
struct FormatFlags {
    bold: bool,
    italic: bool,
    underline: bool,
    link: Option<usize>,
}

#[derive(Clone, Copy)]
enum Style {
    Bold,
    Italic,
    Underline,
}

impl Style {
    fn get(self, f: &FormatFlags) -> bool {
        match self {
            Style::Bold => f.bold,
            Style::Italic => f.italic,
            Style::Underline => f.underline,
        }
    }
    fn set(self, f: &mut FormatFlags, v: bool) {
        match self {
            Style::Bold => f.bold = v,
            Style::Italic => f.italic = v,
            Style::Underline => f.underline = v,
        }
    }
}

/// Images are stored in the text as one private-use character each.
fn is_placeholder(c: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&c)
}

/// A draft body converted for the editor: text plus per-character formatting.
#[derive(Clone, Default)]
struct DraftBody {
    text: String,
    formats: Vec<FormatFlags>,
    links: Vec<String>,
    /// Images: placeholder character -> file path (or cid: reference for drafts from server)
    images: Vec<(char, String)>,
}

type LineMouse = (usize, ParagraphHolder, Event<MouseEventData>);

#[derive(PartialEq)]
pub struct ComposePane {
    pub state: AppState,
    /// The key the parent mounted this pane with (used to pin it across draft saves).
    pub compose_key: String,
}

impl Component for ComposePane {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state.clone();

        // Check for pre-filled compose data (Reply/Reply All/Forward).
        // If present, consume it immediately so it doesn't persist across compose sessions.
        let compose_pre: Option<ComposePre> = {
            let mut pre = state.compose_pre.write();
            pre.take()
        };

        // Extract threading headers early so the send closure can use them.
        // These are needed for In-Reply-To and References headers when replying.
        let threading = compose_pre.as_ref().map(|p| ComposePreThreading {
            original_message_id: p.original_message_id.clone(),
            original_in_reply_to: p.original_in_reply_to.clone(),
        });

        // Extract forwarded HTML body so the send closure can append it and render it in the preview.
        // Stored in use_state so it persists across renders after compose_pre is consumed by .take().
        let mut forwarded_html_body: State<Option<String>> = use_state(|| {
            compose_pre.as_ref().and_then(|p| p.html_body.clone())
        });
        // Plain-text version of forwarded content for the text/plain MIME part.
        let mut forwarded_plain_body: State<Option<String>> = use_state(|| {
            compose_pre.as_ref().and_then(|p| p.forwarded_plain.clone())
        });

        // Pre-fill from draft if editing an existing draft.
        // We read the draft info from the store using (account, folder, uid).
        // Since the UID may change after saving (old draft deleted, new one appended),
        // we also check the tombstoned entries in case the UID hasn't been updated yet.
        let draft_info = *state.draft_mail.read();
        let (draft_to, draft_cc, draft_bcc, draft_subject, draft_body, _draft_uid, needs_fetch, draft_fwd_html, draft_fwd_plain) = {
            // Reply/Forward pre-fill takes priority over draft editing.
            if let Some(pre) = &compose_pre {
                // When forwarding an HTML email, the editor body is left empty so the user
                // can type their message. The forwarded HTML renders in a preview section below.
                // For plain-text forwards or replies, the body contains the quoted text.
                let body_text = if pre.html_body.is_some() {
                    String::new()
                } else {
                    pre.body.clone()
                };
                let body = DraftBody {
                    formats: body_text.chars().map(|_| FormatFlags::default()).collect(),
                    text: body_text,
                    links: Vec::new(),
                    images: Vec::new(),
                };
                (pre.to.clone(), pre.cc.clone(), String::new(), pre.subject.clone(), body, 0u32, None, None, None)
            } else if let Some((account, folder, uid)) = draft_info {
                let store = state.store.read();
                let mail_opt = store.mails.iter().enumerate().find(|(_, m)| {
                    m.account == account && m.folder == folder && m.uid == uid && !m.removed
                });
                if let Some((_, m)) = mail_opt {
                    let (body_text, draft_forwarded_html) = match &m.body {
                        crate::model::BodyState::Loaded { text, content_type, .. } => {
                            if *content_type == crate::data::MailContentType::Html {
                                // Check if this HTML draft contains a forwarded <blockquote> section.
                                // When a forward-draft is saved, the HTML body has the user's content
                                // followed by the forwarded HTML in a <blockquote>. Split them so the
                                // editor shows only the user's content and the preview shows the forwarded HTML.
                                if let Some((user_html, fwd_html)) = split_draft_forwarded_html(text) {
                                    let user_body = html_to_editor(&user_html, &m.attachments);
                                    (user_body, Some(fwd_html))
                                } else {
                                    (html_to_editor(text, &m.attachments), None)
                                }
                            } else {
                                let t = text.to_string();
                                (
                                    DraftBody {
                                        formats: t.chars().map(|_| FormatFlags::default()).collect(),
                                        text: t,
                                        links: Vec::new(),
                                        images: Vec::new(),
                                    },
                                    None,
                                )
                            }
                        }
                        _ => (DraftBody::default(), None),
                    };
                    let body_loaded = matches!(m.body, crate::model::BodyState::Loaded { .. });
                    let needs_fetch = !body_loaded && uid != 0;
                    let fetch_info = if needs_fetch {
                        Some((account, folder, uid))
                    } else {
                        None
                    };
                    // Collect draft forwarded HTML info to apply after use_state initialization.
                    let draft_fwd_plain = if let crate::model::BodyState::Loaded { text, content_type, .. } = &m.body {
                        if *content_type == crate::data::MailContentType::PlainText {
                            Some(text.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    (
                        m.to_email.clone(),
                        String::new(),
                        String::new(),
                        m.subject.clone(),
                        body_text,
                        uid,
                        fetch_info,
                        draft_forwarded_html,
                        draft_fwd_plain,
                    )
                } else {
                    // Mail not found in store (e.g. UID changed after save, or not synced yet).
                    // Try to find a tombstoned entry with the same UID to get the header info.
                    let tombstoned_opt = store.mails.iter().enumerate().find(|(_, m)| {
                        m.account == account && m.folder == folder && m.uid == uid && m.removed
                    });
                    if let Some((_, m)) = tombstoned_opt {
                        (
                            m.to_email.clone(),
                            String::new(),
                            String::new(),
                            m.subject.clone(),
                            DraftBody::default(),
                            uid,
                            None,
                            None,
                            None,
                        )
                    } else {
                        (
                            String::new(),
                            String::new(),
                            String::new(),
                            String::new(),
                            DraftBody::default(),
                            uid,
                            if uid != 0 { Some((account, folder, uid)) } else { None },
                            None,
                            None,
                        )
                    }
                }
            } else {
                (String::new(), String::new(), String::new(), String::new(), DraftBody::default(), 0u32, None, None, None)
            }
        };

        // If we used compose_pre, clear draft_mail so this is a new message, not a draft edit.
        if compose_pre.is_some() {
            state.draft_mail.set(None);
        }

        // Apply draft forwarded HTML to State if compose_pre didn't already set it.
        if draft_fwd_html.is_some() {
            let mut fwd = forwarded_html_body.write();
            if fwd.is_none() {
                *fwd = draft_fwd_html;
            }
        }
        if draft_fwd_plain.is_some() {
            let mut fwd = forwarded_plain_body.write();
            if fwd.is_none() {
                *fwd = draft_fwd_plain;
            }
        }

        // Trigger body fetch only once, using a use_state guard to avoid re-sending.
        let mut fetch_guard = use_state(|| None::<(usize, usize, u32)>);
        if let Some((account, folder, uid)) = needs_fetch {
            let already_fetched = fetch_guard.read().is_some_and(|(a, f, u)| a == account && f == folder && u == uid);
            if !already_fetched {
                fetch_guard.set(Some((account, folder, uid)));
                state.send(account, crate::mail::WorkerCmd::FetchBody { folder, uid });
            }
        } else {
            fetch_guard.set(None);
        }

        let show_cc = use_state(|| !draft_cc.is_empty());
        let show_bcc = use_state(|| false);


        // Account selector: which account to send from.
        // Priority: reply/forward from_account > draft's account > currently selected account > first account.
        let selected_account: State<usize> = use_state(|| {
            // Reply/Forward pre-fill takes priority for the from account.
            if let Some(pre) = &compose_pre {
                let store = state.store.read();
                if store.accounts.is_empty() {
                    0
                } else {
                    pre.from_account.min(store.accounts.len() - 1)
                }
            } else if let Some((account, _, _)) = draft_info {
                account
            } else {
                let store = state.store.read();
                if store.accounts.is_empty() {
                    0
                } else {
                    // Use the currently selected account, or fall back to the first
                    (*state.account.read()).unwrap_or(0).min(store.accounts.len() - 1)
                }
            }
        });
        let show_account_menu = use_state(|| false);

        // One editor for the whole message. Images are placeholder characters in its text.
        let mut editable = use_editable(|| draft_body.text.clone(), EditableConfig::new);
        let a11y_id = use_a11y();
        // One entry per character of the text.
        let formats: State<Vec<FormatFlags>> = use_state(|| {
            let mut f = draft_body.formats.clone();
            f.resize(draft_body.text.chars().count(), FormatFlags::default());
            f
        });

        let to = use_state(|| draft_to.clone());
        let cc = use_state(|| draft_cc.clone());
        let bcc = use_state(|| draft_bcc.clone());
        let subject = use_state(|| draft_subject.clone());

        // Format applied to newly typed text.
        let cursor_format = use_state(|| FormatFlags::default());

        // Links: URLs referenced by FormatFlags::link.
        let links = use_state(|| draft_body.links.clone());
        let show_link_bar = use_state(|| false);
        let link_url = use_state(|| String::new());

        // Images: placeholder character -> file path (or cid: reference for drafts from server).
        let images = use_state(|| draft_body.images.clone());
        let next_img = use_state(|| {
            // Start after the highest placeholder char used in draft images
            let max_char = draft_body.images.iter().map(|(c, _)| *c as u32).max().unwrap_or(0);
            if max_char > 0 {
                (max_char - 0xE000) + 1
            } else {
                0
            }
        });

        let close_on_press: EventHandler<Event<PressEventData>> = {
            let mut s = state.clone();
            EventHandler::new(move |_| {
                s.draft_mail.set(None);
                s.draft_pin.set(None);
                s.compose_pre.set(None);
                s.view.set(crate::state::View::Mail);
            })
        };

        let send_on_press: EventHandler<Event<PressEventData>> = {
            let s = state.clone();
            let to = to.clone();
            let cc = cc.clone();
            let bcc = bcc.clone();
            let subject = subject.clone();
            let editable = editable.clone();
            let formats = formats.clone();
            let links = links.clone();
            let images = images.clone();
            let selected_account = selected_account.clone();
            let threading = threading.clone();
            let forwarded_html = forwarded_html_body.clone();
            let forwarded_plain = forwarded_plain_body.clone();
            EventHandler::new(move |_| {
                let mut s = s.clone();
                let to_val = to.read().trim().to_string();
                if to_val.is_empty() {
                    // No recipient — don't send
                    return;
                }
                let cc_val = cc.read().trim().to_string();
                let bcc_val = bcc.read().trim().to_string();
                let subject_val = subject.read().clone();
                let body_val = editable.editor().read().to_string();
                let all_formats = formats.read().clone();
                let all_links = links.read().clone();
                let all_images = images.read().clone();
                let account = *selected_account.read();
                let forwarded_html_val = forwarded_html.read().clone();
                let forwarded_plain_val = forwarded_plain.read().clone();

                // Look up the sender's address and name from the selected account
                let from_addr = {
                    let store = s.store.read();
                    store.accounts.get(account)
                        .map(|a| {
                            if a.sender_name.is_empty() {
                                a.addr.clone()
                            } else {
                                format!("{} <{}>", a.sender_name, a.addr)
                            }
                        })
                        .unwrap_or_default()
                };

                // Build HTML body from the editor text + formatting flags (with inline images)
                let mut html_body = build_html_body_with_images(&body_val, &all_formats, &all_links, &all_images);
                let mut plain_body = body_val.chars().filter(|c| !is_placeholder(*c)).collect::<String>();

                // Append forwarded content
                if let Some(ref fwd_html) = forwarded_html_val {
                    if !html_body.is_empty() {
                        html_body.push_str(fwd_html);
                    } else {
                        html_body = fwd_html.clone();
                    }
                }
                if let Some(ref fwd_plain) = forwarded_plain_val {
                    if !plain_body.is_empty() {
                        plain_body.push('\n');
                    }
                    plain_body.push_str(fwd_plain);
                }

                // Build a multipart/alternative RFC-2822 message with embedded images
                let boundary = "000000000000abcdef1234";
                let boundary_related = "000000000000fedcba4321";
                let mut raw = String::new();
                if !from_addr.is_empty() {
                    raw.push_str(&format!("From: {}\r\n", from_addr));
                }
                raw.push_str(&format!("To: {}\r\n", to_val));
                if !cc_val.is_empty() {
                    raw.push_str(&format!("Cc: {}\r\n", cc_val));
                }
                // Bcc is stripped from the headers by the SMTP layer before transmission
                if !bcc_val.is_empty() {
                    raw.push_str(&format!("Bcc: {}\r\n", bcc_val));
                }
                raw.push_str(&format!("Subject: {}\r\n", subject_val));
                raw.push_str(&format!("Date: {}\r\n", chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S +0000")));
                // Add In-Reply-To and References headers for reply threading
                if let Some(ref thr) = threading {
                    if !thr.original_message_id.is_empty() {
                        raw.push_str(&format!("In-Reply-To: <{}>\r\n", thr.original_message_id));
                        let mut refs = Vec::new();
                        if !thr.original_in_reply_to.is_empty() {
                            refs.push(format!("<{}>", thr.original_in_reply_to));
                        }
                        refs.push(format!("<{}>", thr.original_message_id));
                        raw.push_str(&format!("References: {}\r\n", refs.join(" ")));
                    }
                }
                raw.push_str("MIME-Version: 1.0\r\n");

                // Structure: multipart/alternative { text/plain, text/html | multipart/related { text/html, images } }
                let has_images = !all_images.is_empty();
                raw.push_str(&format!("Content-Type: multipart/alternative; boundary=\"{}\"\r\n", boundary));
                raw.push_str("\r\n");

                // Plain text part
                raw.push_str(&format!("--{}\r\n", boundary));
                raw.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
                raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                raw.push_str("\r\n");
                raw.push_str(&plain_body);
                raw.push_str("\r\n");

                if has_images {
                    raw.push_str(&format!("--{}\r\n", boundary));
                    raw.push_str(&format!("Content-Type: multipart/related; type=\"text/html\"; boundary=\"{}\"\r\n", boundary_related));
                    raw.push_str("\r\n");

                    // HTML part
                    raw.push_str(&format!("--{}\r\n", boundary_related));
                    raw.push_str("Content-Type: text/html; charset=UTF-8\r\n");
                    raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                    raw.push_str("\r\n");
                    raw.push_str(&html_body);
                    raw.push_str("\r\n");

                    // Embedded images
                    for (ch, path) in &all_images {
                        let Ok(data) = std::fs::read(path) else { continue };
                        let p = std::path::Path::new(path);
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
                        let content_type = match ext.as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            "webp" => "image/webp",
                            "bmp" => "image/bmp",
                            _ => "image/png",
                        };
                        let img_id = format!("img_{}@photonmail", u32::from(*ch));
                        let filename = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("image.png")
                            .replace('"', "");
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                        raw.push_str(&format!("--{}\r\n", boundary_related));
                        raw.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", content_type, filename));
                        raw.push_str("Content-Transfer-Encoding: base64\r\n");
                        raw.push_str(&format!("Content-ID: <{}>\r\n", img_id));
                        raw.push_str(&format!("Content-Disposition: inline; filename=\"{}\"\r\n", filename));
                        raw.push_str("\r\n");
                        for chunk in encoded.as_bytes().chunks(76) {
                            raw.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                            raw.push_str("\r\n");
                        }
                    }
                    raw.push_str(&format!("--{}--\r\n", boundary_related));
                } else {
                    raw.push_str(&format!("--{}\r\n", boundary));
                    raw.push_str("Content-Type: text/html; charset=UTF-8\r\n");
                    raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                    raw.push_str("\r\n");
                    raw.push_str(&html_body);
                    raw.push_str("\r\n");
                }

                // Closing boundary
                raw.push_str(&format!("--{}--\r\n", boundary));

                // Play bald eagle screech on send
                if let Ok(sounds_guard) = crate::SOUNDS.lock() {
                    if let Some(ref sounds) = *sounds_guard {
                        sounds.play_eagle();
                    }
                }

                // Send the email via SMTP
                s.send_email(account, raw);

                // Close the compose window
                s.draft_mail.set(None);
                s.draft_pin.set(None);
                s.compose_pre.set(None);
                s.view.set(crate::state::View::Mail);
            })
        };

        let save_draft_on_press: EventHandler<Event<PressEventData>> = {
            let s = state.clone();
            let to = to.clone();
            let cc = cc.clone();
            let bcc = bcc.clone();
            let subject = subject.clone();
            let editable = editable.clone();
            let formats = formats.clone();
            let links = links.clone();
            let images = images.clone();
            let compose_key = self.compose_key.clone();
            let selected_account = selected_account.clone();
            let threading = threading.clone();
            let forwarded_html = forwarded_html_body.clone();
            let forwarded_plain = forwarded_plain_body.clone();
            EventHandler::new(move |_| {
                let mut s = s.clone();
                // Keep this pane mounted when the server hands back the saved draft's new UID.
                s.draft_pin.set(Some((compose_key.clone(), None)));
                let to_val = to.read().clone();
                let cc_val = cc.read().clone();
                let bcc_val = bcc.read().clone();
                let subject_val = subject.read().clone();
                let body_val = editable.editor().read().to_string();
                let all_formats = formats.read().clone();
                let all_links = links.read().clone();
                let all_images = images.read().clone();
                let account = *selected_account.read();
                let forwarded_html_val = forwarded_html.read().clone();
                let forwarded_plain_val = forwarded_plain.read().clone();

                // Look up the sender's address and name from the selected account
                let from_addr = {
                    let store = s.store.read();
                    store.accounts.get(account)
                        .map(|a| {
                            if a.sender_name.is_empty() {
                                a.addr.clone()
                            } else {
                                format!("{} <{}>", a.sender_name, a.addr)
                            }
                        })
                        .unwrap_or_default()
                };

                // Build HTML body from the editor text + formatting flags (with inline images)
                let mut html_body = build_html_body_with_images(&body_val, &all_formats, &all_links, &all_images);
                let mut plain_body = body_val.chars().filter(|c| !is_placeholder(*c)).collect::<String>();

                // Append forwarded content
                if let Some(ref fwd_html) = forwarded_html_val {
                    if !html_body.is_empty() {
                        html_body.push_str(fwd_html);
                    } else {
                        html_body = fwd_html.clone();
                    }
                }
                if let Some(ref fwd_plain) = forwarded_plain_val {
                    if !plain_body.is_empty() {
                        plain_body.push('\n');
                    }
                    plain_body.push_str(fwd_plain);
                }

                // Build a multipart/alternative RFC-2822 message with embedded images
                let boundary = "000000000000abcdef1234";
                let boundary_related = "000000000000fedcba4321";
                let mut raw = String::new();
                if !from_addr.is_empty() {
                    raw.push_str(&format!("From: {}\r\n", from_addr));
                }
                raw.push_str(&format!("To: {}\r\n", to_val));
                if !cc_val.is_empty() {
                    raw.push_str(&format!("Cc: {}\r\n", cc_val));
                }
                if !bcc_val.is_empty() {
                    raw.push_str(&format!("Bcc: {}\r\n", bcc_val));
                }
                raw.push_str(&format!("Subject: {}\r\n", subject_val));
                raw.push_str(&format!("Date: {}\r\n", chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S +0000")));
                // Add In-Reply-To and References headers for reply threading
                if let Some(ref thr) = threading {
                    if !thr.original_message_id.is_empty() {
                        raw.push_str(&format!("In-Reply-To: <{}>\r\n", thr.original_message_id));
                        let mut refs = Vec::new();
                        if !thr.original_in_reply_to.is_empty() {
                            refs.push(format!("<{}>", thr.original_in_reply_to));
                        }
                        refs.push(format!("<{}>", thr.original_message_id));
                        raw.push_str(&format!("References: {}\r\n", refs.join(" ")));
                    }
                }
                raw.push_str("MIME-Version: 1.0\r\n");

                // Structure: multipart/alternative { text/plain, text/html | multipart/related { text/html, images } }
                let has_images = !all_images.is_empty();
                raw.push_str(&format!("Content-Type: multipart/alternative; boundary=\"{}\"\r\n", boundary));
                raw.push_str("\r\n");

                // Plain text part
                raw.push_str(&format!("--{}\r\n", boundary));
                raw.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
                raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                raw.push_str("\r\n");
                raw.push_str(&plain_body);
                raw.push_str("\r\n");

                if has_images {
                    raw.push_str(&format!("--{}\r\n", boundary));
                    raw.push_str(&format!("Content-Type: multipart/related; type=\"text/html\"; boundary=\"{}\"\r\n", boundary_related));
                    raw.push_str("\r\n");

                    // HTML part
                    raw.push_str(&format!("--{}\r\n", boundary_related));
                    raw.push_str("Content-Type: text/html; charset=UTF-8\r\n");
                    raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                    raw.push_str("\r\n");
                    raw.push_str(&html_body);
                    raw.push_str("\r\n");

                    // Embedded images (one part per placeholder, CID matches the <img> in the HTML)
                    for (ch, path) in &all_images {
                        let Ok(data) = std::fs::read(path) else { continue };
                        let p = std::path::Path::new(path);
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
                        let content_type = match ext.as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            "webp" => "image/webp",
                            "bmp" => "image/bmp",
                            _ => "image/png",
                        };
                        let img_id = format!("img_{}@photonmail", u32::from(*ch));
                        let filename = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("image.png")
                            .replace('"', "");
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                        raw.push_str(&format!("--{}\r\n", boundary_related));
                        raw.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", content_type, filename));
                        raw.push_str("Content-Transfer-Encoding: base64\r\n");
                        raw.push_str(&format!("Content-ID: <{}>\r\n", img_id));
                        raw.push_str(&format!("Content-Disposition: inline; filename=\"{}\"\r\n", filename));
                        raw.push_str("\r\n");
                        // 76-char lines, CRLF-terminated
                        for chunk in encoded.as_bytes().chunks(76) {
                            raw.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                            raw.push_str("\r\n");
                        }
                    }
                    raw.push_str(&format!("--{}--\r\n", boundary_related));
                } else {
                    raw.push_str(&format!("--{}\r\n", boundary));
                    raw.push_str("Content-Type: text/html; charset=UTF-8\r\n");
                    raw.push_str("Content-Transfer-Encoding: 8bit\r\n");
                    raw.push_str("\r\n");
                    raw.push_str(&html_body);
                    raw.push_str("\r\n");
                }

                // Closing boundary
                raw.push_str(&format!("--{}--\r\n", boundary));

                // Check if we're updating an existing draft or creating a new one
                if let Some((old_account, _folder, uid)) = draft_info {
                    if uid != 0 {
                        // If account changed, move the draft to the new account
                        if old_account != account {
                            s.move_draft_to_account(old_account, uid, account, raw);
                        } else {
                            // Update existing draft in same account
                            s.update_draft(account, uid, raw);
                        }
                    } else {
                        // Demo mail, no server — just append
                        s.save_new_draft(account, raw);
                    }
                } else {
                    // New draft — save to selected account
                    s.save_new_draft(account, raw);
                }
            })
        };

        let toggle_cc: EventHandler<Event<PressEventData>> = {
            let mut show = show_cc.clone();
            EventHandler::new(move |_| {
                show.set(!show());
            })
        };

        let toggle_bcc: EventHandler<Event<PressEventData>> = {
            let mut show = show_bcc.clone();
            EventHandler::new(move |_| {
                show.set(!show());
            })
        };

        // --- Account selector handlers ---
        let toggle_account_menu: EventHandler<Event<PressEventData>> = {
            let mut show = show_account_menu.clone();
            EventHandler::new(move |_| show.set(!show()))
        };

        let select_account: EventHandler<(usize, Event<PressEventData>)> = {
            let mut s = state.clone();
            let mut sel = selected_account.clone();
            let mut show_menu = show_account_menu.clone();
            EventHandler::new(move |(account_idx, _)| {
                let old_account = *sel.read();
                if old_account == account_idx {
                    show_menu.set(false);
                    return;
                }
                // If editing an existing draft and switching accounts, move the draft.
                if let Some((old_acct, _folder, uid)) = draft_info {
                    if uid != 0 && old_acct == old_account {
                        // Delete the old draft from the old account
                        s.update_draft_move(old_acct, uid, account_idx);
                        // Update the draft_mail to point to the new account
                        s.draft_mail.set(Some((account_idx, crate::mail::DRAFTS, uid)));
                    }
                }
                sel.set(account_idx);
                show_menu.set(false);
            })
        };

        let toggle_bold: EventHandler<Event<PressEventData>> =
            EventHandler::new(move |_| toggle_format(editable, formats, cursor_format, Style::Bold));
        let toggle_italic: EventHandler<Event<PressEventData>> =
            EventHandler::new(move |_| toggle_format(editable, formats, cursor_format, Style::Italic));
        let toggle_underline: EventHandler<Event<PressEventData>> =
            EventHandler::new(move |_| toggle_format(editable, formats, cursor_format, Style::Underline));

        // --- Link handlers ---
        let toggle_link_bar: EventHandler<Event<PressEventData>> = {
            let mut show = show_link_bar;
            EventHandler::new(move |_| show.set(!show()))
        };
        let apply_link_press: EventHandler<Event<PressEventData>> = {
            let mut show = show_link_bar;
            let mut url_state = link_url;
            EventHandler::new(move |_| {
                let url = url_state.read().trim().to_string();
                if !url.is_empty() {
                    apply_link(editable, formats, links, Some(url));
                }
                url_state.set(String::new());
                show.set(false);
                a11y_id.request_focus();
            })
        };
        let remove_link_press: EventHandler<Event<PressEventData>> = {
            let mut show = show_link_bar;
            EventHandler::new(move |_| {
                apply_link(editable, formats, links, None);
                show.set(false);
                a11y_id.request_focus();
            })
        };

        // --- Image picker (native file dialog, multi-select) ---
        // Note: the dialog is blocking, so the window may pause while it is open.
        let pick_images_press: EventHandler<Event<PressEventData>> =
            EventHandler::new(move |_| {
                if let Some(files) = rfd::FileDialog::new()
                    .set_title("Embed images")
                    .add_filter("Images", IMAGE_EXTENSIONS)
                    .pick_files()
                {
                    for f in files {
                        insert_image_at_cursor(editable, formats, images, next_img, &f);
                    }
                }
                a11y_id.request_focus();
            });

        // --- Mouse handlers shared by all line paragraphs ---
        let on_line_down: EventHandler<LineMouse> =
            EventHandler::new(move |(row, holder, e): LineMouse| {
                a11y_id.request_focus();
                editable.process_event(EditableEvent::Down {
                    location: e.element_location,
                    editor_line: EditorLine::Paragraph(row),
                    holder: &holder,
                });
            });
        let on_line_move: EventHandler<LineMouse> =
            EventHandler::new(move |(row, holder, e): LineMouse| {
                editable.process_event(EditableEvent::Move {
                    location: e.element_location,
                    editor_line: EditorLine::Paragraph(row),
                    holder: &holder,
                });
            });

        let dark_input = InputColorsThemePartial::new()
            .focus_background(Color::TRANSPARENT)
            .focus_border_fill(Color::TRANSPARENT)
            .border_fill(Color::TRANSPARENT)
            .color(DARK.text)
            .placeholder_color(DARK.muted);

        // --- Header bar ---
        let is_editing_draft = draft_info.is_some();
        let header_title = if is_editing_draft { "Edit draft" } else { "New message" };
        let header = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .child(label().text(header_title).font_size(14.).font_weight(FontWeight::BOLD).color(DARK.text))
            .child(rect().width(Size::flex(1.)))
            .child(IconButton {
                bytes: CHEVRON_LEFT,
                color: DARK.muted,
                on_press: close_on_press,
            })
            .child(
                IconButton {
                    bytes: DISK,
                    color: DARK.muted,
                    on_press: save_draft_on_press,
                }
            )
            .child({
                // FREEDOM GLOW on Send button
                let mut send_glow = use_state(|| 0u32);
                use_hook(|| {
                    let mut sg = send_glow.clone();
                    freya::prelude::spawn(async move {
                        loop {
                            futures_timer::Delay::new(std::time::Duration::from_millis(100)).await;
                            let f = *sg.read();
                            sg.set(f.wrapping_add(1));
                        }
                    });
                });
                let frame = *send_glow.read();
                let phase = (frame % 30) as f32 / 30.0;
                let colors = [(200, 40, 60), (240, 240, 255), (60, 60, 200)];
                let glow_color = colors[(phase * 3.0) as usize % 3];
                let send_on = send_on_press.clone();
                rect()
                    .padding((6., 14., 6., 14.))
                    .corner_radius(6.)
                    .background(Fill::from(DARK.success))
                    .border(Border::new()
                        .width(2.)
                        .fill(Color::from_rgb(glow_color.0, glow_color.1, glow_color.2)))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(send_on)
                    .child(icon(SEND, 14., DARK.text))
                    .child(label().text("Send").color(DARK.text).font_size(13.).font_weight(FontWeight::BOLD))
            });

        // --- From (account selector) ---
        let store_accounts = state.store.read().accounts.clone();
        let (from_label, from_menu): (Rect, Option<Rect>) = if store_accounts.len() > 1 {
            // Show dropdown when multiple accounts exist
            let selected_idx = *selected_account.read();
            let account_name = store_accounts.get(selected_idx)
                .map(|a| a.addr.clone())
                .unwrap_or_default();

            // The From row (label + selectable input). The dropdown is returned separately and
            // added as the LAST child of the fields container so it paints over To / CC / Subject.
            let from_row =
                rect()
                    .width(Size::fill())
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(
                        label()
                            .text("From")
                            .color(DARK.muted)
                            .font_size(12.)
                            .width(Size::px(60.)),
                    )
                    .child(
                        rect()
                            .width(Size::flex(1.))
                            .padding((6., 10., 6., 10.))
                            .corner_radius(6.)
                            .background(Fill::from((20, 20, 31)))
                            .border(Border::new().fill((50, 50, 90)).width(1.))
                            .horizontal()
                            .content(Content::Flex)
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .cursor(freya::winit::window::CursorIcon::Pointer)
                            .on_press(toggle_account_menu.clone())
                            .child(
                                label()
                                    .text(account_name)
                                    .color(DARK.text)
                                    .font_size(13.)
                                    .max_lines(1),
                            )
                            .child(icon(CHEVRON_DOWN, 12., DARK.muted)),
                    );

            let from_menu = (*show_account_menu.read()).then(|| {
                // Dropdown menu — positioned below the From row, on top of other content
                let mut menu_col = rect().vertical();
                for (idx, acc) in store_accounts.iter().enumerate() {
                    let is_selected = idx == selected_idx;
                    let bg = if is_selected {
                        Fill::from((40, 40, 100))
                    } else {
                        Fill::from((25, 25, 55))
                    };
                    let text_color = if is_selected {
                        DARK.accent
                    } else {
                        DARK.text
                    };
                    let account_idx = idx;
                    let addr = acc.addr.clone();
                    let color = acc.color;
                    menu_col = menu_col.child(
                        rect()
                            .width(Size::fill())
                            .height(Size::px(34.))
                            .padding((0., 12., 0., 12.))
                            .background(bg)
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(8.)
                            .cursor(freya::winit::window::CursorIcon::Pointer)
                            .on_press({
                                let handler = select_account.clone();
                                EventHandler::new(move |e| {
                                    handler.call((account_idx, e));
                                })
                            })
                            .child(
                                rect()
                                    .width(Size::px(12.))
                                    .height(Size::px(12.))
                                    .corner_radius(6.)
                                    .background(Fill::from(color)),
                            )
                            .child(
                                label()
                                    .text(addr)
                                    .color(text_color)
                                    .font_size(13.)
                                    .max_lines(1),
                            )
                    );
                }
                // Floating overlay (Layer::Overlay paints above the inputs and gets pointer
                // events first). A 68px spacer (60px label + 8px spacing) lines the menu up
                // with the From box; the menu flexes to the rest of the width. Beyond
                // MAX_MENU_H the list scrolls.
                const ROW_H: f32 = 34.;
                const MAX_MENU_H: f32 = 204.; // ~6 rows
                let list_h = (store_accounts.len() as f32 * ROW_H).min(MAX_MENU_H);
                rect()
                    .layer(Layer::Overlay)
                    .position(Position::new_absolute().top(36.).left(0.))
                    .width(Size::fill())
                    .horizontal()
                    .content(Content::Flex)
                    .child(rect().width(Size::px(68.)))
                    .child(
                        rect()
                            .width(Size::flex(1.))
                            .corner_radius(8.)
                            .background(Fill::from((25, 25, 55)))
                            .border(Border::new().fill((50, 50, 90)).width(1.))
                            .child(
                                ScrollView::new()
                                    .width(Size::fill())
                                    .height(Size::px(list_h))
                                    .child(menu_col.width(Size::fill())),
                            ),
                    )
            });
            (from_row, from_menu)
        } else {
            // Single account — just show "From" like a regular field
            let account_name = store_accounts.first()
                .map(|a| a.addr.clone())
                .unwrap_or_default();
            let row = rect()
                .width(Size::fill())
                .horizontal()
                .content(Content::Flex)
                .cross_align(Alignment::Center)
                .spacing(8.)
                .child(
                    label()
                        .text("From")
                        .color(DARK.muted)
                        .font_size(12.)
                        .width(Size::px(60.)),
                )
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .padding((6., 10., 6., 10.))
                        .corner_radius(6.)
                        .background(Fill::from((20, 20, 31)))
                        .border(Border::new().fill((50, 50, 90)).width(1.))
                        .child(
                            label()
                                .text(account_name)
                                .color(DARK.muted)
                                .font_size(13.),
                        ),
                );
            (row, None)
        };

        // --- To / CC / BCC / Subject ---
        let fields = rect()
            .width(Size::fill())
            .vertical()
            .spacing(8.)
            .child(from_label)
            .child(input_field("To".to_string(), to.clone(), dark_input.clone()))
            .child({
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(
                        label()
                            .text("CC / BCC")
                            .color(DARK.muted)
                            .font_size(12.)
                            .padding((0., 0., 0., 4.)),
                    )
                    .child(IconButton {
                        bytes: crate::icons::PLUS,
                        color: DARK.muted,
                        on_press: toggle_cc,
                    })
                    .child(IconButton {
                        bytes: crate::icons::PLUS,
                        color: DARK.muted,
                        on_press: toggle_bcc,
                    })
            })
            .child({
                if *show_cc.read() {
                    rect().child(input_field("CC".to_string(), cc.clone(), dark_input.clone()))
                } else {
                    rect()
                }
            })
            .child({
                if *show_bcc.read() {
                    rect().child(input_field("BCC".to_string(), bcc.clone(), dark_input.clone()))
                } else {
                    rect()
                }
            })
            .child(input_field("Subject".to_string(), subject.clone(), dark_input.clone()))
            // Overlay layer: floats above the inputs below the From row instead of pushing them.
            .maybe_child(from_menu);

        // --- Snapshot of the editor for this render ---
        let text = editable.editor().read().to_string();
        let cursor = editable.editor().read().cursor_pos();
        let selection = editable
            .editor()
            .read()
            .get_selection()
            .map(|(s, e)| (s.min(e), s.max(e)))
            .filter(|(s, e)| s != e);
        let all_formats = formats.read().clone();
        let image_map = images.read().clone();

        // --- Formatting toolbar ---
        let (toolbar_fmt, link_active) = match selection {
            Some((s, e)) => {
                let slice: Vec<FormatFlags> = (s..e)
                    .map(|i| all_formats.get(i).copied().unwrap_or_default())
                    .collect();
                (
                    FormatFlags {
                        bold: slice.iter().all(|x| x.bold),
                        italic: slice.iter().all(|x| x.italic),
                        underline: slice.iter().all(|x| x.underline),
                        ..Default::default()
                    },
                    slice.iter().all(|x| x.link.is_some()),
                )
            }
            None => (*cursor_format.read(), false),
        };

        let toolbar = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(4.)
            .child(format_button("B".to_string(), FontWeight::BOLD, toolbar_fmt.bold, toggle_bold))
            .child(format_button("I".to_string(), FontWeight::NORMAL, toolbar_fmt.italic, toggle_italic))
            .child(format_button("U".to_string(), FontWeight::NORMAL, toolbar_fmt.underline, toggle_underline))
            .child(format_button("Link".to_string(), FontWeight::NORMAL, link_active, toggle_link_bar))
            .child(format_button("Image".to_string(), FontWeight::NORMAL, false, pick_images_press))
            .child(rect().width(Size::flex(1.)));

        // --- Link URL bar (shown when the Link button / Ctrl+K is used) ---
        let link_bar = if *show_link_bar.read() {
            rect()
                .width(Size::fill())
                .horizontal()
                .content(Content::Flex)
                .cross_align(Alignment::Center)
                .spacing(8.)
                .child(label().text("URL").color(DARK.muted).font_size(12.))
                .child(field_box(
                    Input::new(link_url)
                        .flat()
                        .compact()
                        .width(Size::fill())
                        .theme_colors(dark_input.clone()),
                ))
                .child(format_button("Apply".to_string(), FontWeight::NORMAL, false, apply_link_press))
                .child(format_button("Remove".to_string(), FontWeight::NORMAL, false, remove_link_press))
        } else {
            rect()
        };

        // --- Placeholder text (only while the message is empty) ---
        let placeholder = if text.is_empty() {
            rect()
                .position(Position::new_absolute().top(0.).left(0.))
                .child(
                    label()
                        .text("Write your message…")
                        .font_size(14.)
                        .color(DARK.muted)
                        .padding((1., 0., 0., 8.)),
                )
        } else {
            rect()
        };

        // --- The editor: one paragraph per line, images under the line that holds them ---
        let mut lines_column = rect()
            .width(Size::fill())
            .vertical()
            .spacing(2.)
            .a11y_id(a11y_id)
            .a11y_focusable(true)
            .a11y_auto_focus(true)
            .on_global_pointer_up(move |_: Event<PointerEventData>| {
                editable.process_event(EditableEvent::Release);
            })
            .on_key_down(move |e: Event<KeyboardEventData>| {
                let ctrl_or_meta = e.modifiers.ctrl() || e.modifiers.meta();

                if ctrl_or_meta {
                    if let Key::Character(ch) = &e.key {
                        let lower = ch.to_lowercase();

                        // Ctrl/Cmd+K opens the link bar.
                        if lower.as_str() == "k" {
                            let mut show = show_link_bar;
                            show.set(true);
                            e.prevent_default();
                            e.stop_propagation();
                            return;
                        }

                        // Our formatting shortcuts
                        let style = match lower.as_str() {
                            "b" => Some(Style::Bold),
                            "i" => Some(Style::Italic),
                            "u" => Some(Style::Underline),
                            _ => None,
                        };
                        if let Some(style) = style {
                            toggle_format(editable, formats, cursor_format, style);
                            e.prevent_default();
                            e.stop_propagation();
                            return;
                        }

                        // Shortcuts the editor handles itself. Anything else
                        // with Ctrl/Cmd does nothing. Alt is excluded because
                        // AltGr is reported as Ctrl+Alt on some layouts.
                        let editor_shortcut =
                            matches!(lower.as_str(), "a" | "c" | "x" | "v" | "z" | "y");
                        if !editor_shortcut && !e.modifiers.alt() {
                            e.prevent_default();
                            e.stop_propagation();
                            return;
                        }
                    }
                }

                // No holder: movement works on newline-separated lines of the whole text.
                let before = editable.editor().read().to_string();
                editable.process_event(EditableEvent::KeyDown {
                    key: &e.key,
                    modifiers: e.modifiers,
                    editor_line: Some(EditorLine::SingleParagraph),
                    holder: None,
                });
                let after = editable.editor().read().to_string();
                if before != after {
                    let insert_fmt = *cursor_format.read();
                    let mut f = formats;
                    reconcile_formats(&mut f.write(), &before, &after, insert_fmt);
                }

                // The editor owns navigation keys; don't let the ScrollView scroll too.
                if matches!(
                    e.key,
                    Key::Named(NamedKey::ArrowUp)
                        | Key::Named(NamedKey::ArrowDown)
                        | Key::Named(NamedKey::ArrowLeft)
                        | Key::Named(NamedKey::ArrowRight)
                        | Key::Named(NamedKey::PageUp)
                        | Key::Named(NamedKey::PageDown)
                        | Key::Named(NamedKey::Home)
                        | Key::Named(NamedKey::End)
                ) {
                    e.stop_propagation();
                }
            })
            .on_key_up(move |e: Event<KeyboardEventData>| {
                editable.process_event(EditableEvent::KeyUp { key: &e.key });
            })
            .child(placeholder);

        let mut offset = 0usize; // char index where the current line starts
        for (row, line) in text.split('\n').enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let line_start = offset;
            let line_end = offset + chars.len();
            offset = line_end + 1; // +1 for the '\n'

            let cursor_col = if cursor >= line_start && cursor <= line_end {
                Some(cursor - line_start)
            } else {
                None
            };
            let highlight = selection.and_then(|(s, e)| {
                let a = s.max(line_start);
                let b = e.min(line_end);
                if a < b { Some((a - line_start, b - line_start)) } else { None }
            });
            let flags: Vec<FormatFlags> = (line_start..line_end)
                .map(|i| all_formats.get(i).copied().unwrap_or_default())
                .collect();
            // Show placeholders as a blank space of the same width (keeps indices aligned).
            let display: String = chars
                .iter()
                .map(|c| if is_placeholder(*c) { '\u{00A0}' } else { *c })
                .collect();

            let mut row_rect = rect().width(Size::fill()).vertical().spacing(4.).child(EditorLineView {
                row,
                text: display,
                flags,
                cursor_col,
                highlight,
                on_down: on_line_down.clone(),
                on_move: on_line_move.clone(),
            });

            // Images whose placeholder sits on this line are shown right under it.
            for (col, c) in chars.iter().enumerate() {
                if !is_placeholder(*c) {
                    continue;
                }
                if let Some((_, path)) = image_map.iter().find(|(pc, _)| pc == c) {
                    let abs = line_start + col;
                    let remove: EventHandler<Event<PressEventData>> =
                        EventHandler::new(move |_| {
                            remove_image_at(editable, formats, abs);
                            a11y_id.request_focus();
                        });
                    // If the path is a cid: reference (from a server draft), show a placeholder
                    if path.starts_with("cid:") {
                        row_rect = row_rect.child(
                            rect()
                                .width(Size::px(360.))
                                .height(Size::px(120.))
                                .corner_radius(6.)
                                .background(Fill::from((20, 20, 31)))
                                .border(Border::new().fill((50, 50, 90)).width(1.))
                                .center()
                                .child(
                                    label()
                                        .text("Embedded image (cid reference)")
                                        .color(DARK.muted)
                                        .font_size(12.),
                                ),
                        );
                    } else {
                        row_rect = row_rect.child(
                            image_thumb(path.clone(), remove, 360., 240.).on_mouse_down(
                                move |e: Event<MouseEventData>| {
                                    a11y_id.request_focus();
                                    e.stop_propagation();
                                },
                            ),
                        );
                    }
                }
            }

            lines_column = lines_column.child(row_rect);
        }

        let editor_container = rect()
            .width(Size::fill())
            .height(Size::px(if forwarded_html_body.read().is_some() { 110. } else { 80. }))
            .padding(6.)
            .corner_radius(8.)
            .background(crate::grain::surface())
            // Clicking empty space (e.g. below the last line) focuses the editor and
            // moves the cursor to the end. Line paragraphs stop their clicks first.
            .on_mouse_down(move |_: Event<MouseEventData>| {
                a11y_id.request_focus();
                let len = editable.editor().read().to_string().chars().count();
                move_cursor_to(editable, len);
            })
            .child(ScrollView::new().height(Size::fill()).child(lines_column));

        // --- Forwarded message (shown inline under the editor, like a traditional client) ---
        let forwarded_html_val = forwarded_html_body.read().clone();
        let forwarded_preview = if let Some(ref fwd_html) = forwarded_html_val {
            // Preview-only cleanup (the sent message still uses the untouched HTML):
            // drop the leading "--- Forwarded message ---" paragraph (the divider below says
            // it) and zero the body margin so the content sits flush.
            let html = {
                let trimmed = fwd_html.trim_start();
                let marker = "<p>--- Forwarded message ---</p>";
                let body = trimmed.strip_prefix(marker).unwrap_or(trimmed);
                format!("<style>html,body{{margin:0;padding:0}}</style>{}", body)
            };
            let line = || {
                rect()
                    .width(Size::flex(1.))
                    .height(Size::px(1.))
                    .background(Fill::from((50, 50, 90)))
            };
            // "---- Forwarded message ----" divider
            let divider_row = rect()
                .width(Size::fill())
                .padding((10., 0., 8., 0.))
                .horizontal()
                .content(Content::Flex) // needed so the flex(1.) lines share the width
                .cross_align(Alignment::Center)
                .spacing(10.)
                .child(line())
                .child(
                    label()
                        .text("Forwarded message")
                        .color(DARK.muted)
                        .font_size(12.),
                )
                .child(line());
            // HtmlViewer paints a viewport-sized window and scrolls the document itself
            // (mouse wheel), so it must not sit inside a ScrollView.
            Some(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .vertical()
                    .child(divider_row)
                    .child(
                        rect()
                            .width(Size::fill())
                            .height(Size::fill())
                            .background(Fill::from((28, 28, 38)))
                            .key(&html)
                            .child(SecureHtmlViewer {
                                html,
                                sender_email: String::new(),
                                state: state.clone(),
                                quote_toggle_idx: None,
                            }),
                    ),
            )
        } else {
            None
        };

        let body_area = if forwarded_html_val.is_some() {
            // Forwarding: no outer ScrollView. The editor keeps its fixed height and the
            // forwarded preview fills the rest, scrolling internally.
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .vertical()
                .content(Content::Flex)
                .child(toolbar)
                .child(link_bar)
                .child(editor_container)
                .maybe_child(forwarded_preview)
        } else {
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .child(
                    ScrollView::new()
                        .height(Size::fill())
                        .child(
                            rect()
                                .width(Size::fill())
                                .min_height(Size::fill())
                                .vertical()
                                .child(toolbar)
                                .child(link_bar)
                                .child(editor_container)
                                .maybe_child(forwarded_preview),
                        ),
                )
        };

        // Main container. Dropping image files anywhere on the pane embeds them at the cursor.
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .vertical()
            .content(Content::Flex) // needed so body_area's flex(1.) resolves
            .spacing(16.)
            .padding(10.)
            .on_file_drop(move |e: Event<FileEventData>| {
                // Debug: remove this line once drag-and-drop works.
                eprintln!("file drop: {:?}", e.file_paths);
                for path in e.file_paths.iter() {
                    insert_image_at_cursor(editable, formats, images, next_img, path);
                }
            })
            .child(header)
            .child(divider())
            .child(fields)
            .child(divider())
            .child(body_area)
    }
}

/// One line of the message as its own paragraph (so images can sit between lines).
#[derive(PartialEq, Clone)]
struct EditorLineView {
    row: usize,
    /// Display text: placeholders already replaced by a blank space.
    text: String,
    flags: Vec<FormatFlags>,
    cursor_col: Option<usize>,
    highlight: Option<(usize, usize)>,
    on_down: EventHandler<LineMouse>,
    on_move: EventHandler<LineMouse>,
}

impl Component for EditorLineView {
    fn render(&self) -> impl IntoElement {
        let holder = use_state(ParagraphHolder::default);
        let row = self.row;
        let on_down = self.on_down.clone();
        let on_move = self.on_move.clone();

        // Only the line holding the cursor shows it.
        let cursor_color: Color = if self.cursor_col.is_some() {
            DARK.accent.into()
        } else {
            Color::TRANSPARENT
        };

        paragraph()
            .padding((1., 4., 1., 4.))
            .width(Size::fill())
            .cursor_color(cursor_color)
            .highlight_color(Color::from_rgb(99, 79, 189).with_a(80))
            .cursor_index(self.cursor_col.unwrap_or(0))
            .highlights(self.highlight.map(|h| vec![h]).unwrap_or_default())
            .on_mouse_down(move |e: Event<MouseEventData>| {
                // Stop here so the container's "click below the text" handler doesn't also run.
                e.stop_propagation();
                on_down.call((row, holder.read().clone(), e));
            })
            .on_mouse_move(move |e: Event<MouseEventData>| {
                on_move.call((row, holder.read().clone(), e));
            })
            .spans_iter(build_spans(self.text.clone(), self.flags.clone()))
            .holder(holder.read().clone())
    }
}

/// Move the cursor to a character index by sending arrow keys.
fn move_cursor_to(mut editable: UseEditable, target: usize) {
    let len = editable.editor().read().to_string().chars().count();
    let target = target.min(len);
    for _ in 0..(len + 2) {
        let cur = editable.editor().read().cursor_pos();
        if cur == target {
            break;
        }
        let key = Key::Named(if cur < target {
            NamedKey::ArrowRight
        } else {
            NamedKey::ArrowLeft
        });
        editable.process_event(EditableEvent::KeyDown {
            key: &key,
            modifiers: Modifiers::empty(),
            editor_line: Some(EditorLine::SingleParagraph),
            holder: None,
        });
        if editable.editor().read().cursor_pos() == cur {
            break; // no progress
        }
    }
}

/// Send one key to the editor and keep the format vector in sync with the edit.
fn send_key(mut editable: UseEditable, mut formats: State<Vec<FormatFlags>>, insert_fmt: FormatFlags, key: Key) {
    let before = editable.editor().read().to_string();
    editable.process_event(EditableEvent::KeyDown {
        key: &key,
        modifiers: Modifiers::empty(),
        editor_line: Some(EditorLine::SingleParagraph),
        holder: None,
    });
    let after = editable.editor().read().to_string();
    if before != after {
        reconcile_formats(&mut formats.write(), &before, &after, insert_fmt);
    }
}

fn alloc_placeholder(mut next_img: State<u32>) -> char {
    let n = *next_img.read();
    next_img.set(n + 1);
    char::from_u32(0xE000 + (n % 6400)).unwrap_or('\u{E000}')
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
        && path.is_file()
}

/// Insert an image at the cursor: type its placeholder character, then press Enter so
/// typing continues below the image.
fn insert_image_at_cursor(
    editable: UseEditable,
    formats: State<Vec<FormatFlags>>,
    mut images: State<Vec<(char, String)>>,
    next_img: State<u32>,
    path: &Path,
) {
    if !is_image_file(path) {
        return;
    }
    let ch = alloc_placeholder(next_img);
    images.write().push((ch, path.to_string_lossy().to_string()));
    send_key(editable, formats, FormatFlags::default(), Key::Character(ch.to_string().into()));
    send_key(editable, formats, FormatFlags::default(), Key::Named(NamedKey::Enter));
}

/// Delete the image whose placeholder character is at `index` (cursor after it, Backspace).
fn remove_image_at(editable: UseEditable, formats: State<Vec<FormatFlags>>, index: usize) {
    let is_placeholder_there = editable
        .editor()
        .read()
        .to_string()
        .chars()
        .nth(index)
        .map_or(false, is_placeholder);
    if !is_placeholder_there {
        return;
    }
    move_cursor_to(editable, index + 1);
    send_key(editable, formats, FormatFlags::default(), Key::Named(NamedKey::Backspace));
}

/// Toggle a style on the selection, or on the "next typed text" format if no selection.
fn toggle_format(
    editable: UseEditable,
    mut formats: State<Vec<FormatFlags>>,
    mut cursor_format: State<FormatFlags>,
    style: Style,
) {
    let sel = editable.editor().read().get_selection();
    match sel {
        Some((s, e)) if s != e => {
            let (s, e) = (s.min(e), s.max(e));
            let mut f = formats.write();
            if f.len() < e {
                f.resize(e, FormatFlags::default());
            }
            let all_on = f[s..e].iter().all(|x| style.get(x));
            for x in &mut f[s..e] {
                style.set(x, !all_on);
            }
        }
        _ => {
            let mut cf = *cursor_format.read();
            let cur = style.get(&cf);
            style.set(&mut cf, !cur);
            cursor_format.set(cf);
        }
    }
}

/// Set (Some(url)) or clear (None) the link on the current selection.
/// Does nothing if there is no selection.
fn apply_link(
    editable: UseEditable,
    mut formats: State<Vec<FormatFlags>>,
    mut links: State<Vec<String>>,
    url: Option<String>,
) {
    let sel = editable.editor().read().get_selection();
    if let Some((s, e)) = sel {
        if s == e {
            return;
        }
        let (s, e) = (s.min(e), s.max(e));
        let link_id = url.map(|u| {
            let mut l = links.write();
            l.push(u);
            l.len() - 1
        });
        let mut f = formats.write();
        if f.len() < e {
            f.resize(e, FormatFlags::default());
        }
        for x in &mut f[s..e] {
            x.link = link_id;
        }
    }
}

/// Keep the per-character format vector in sync with a text edit.
/// Finds the changed region via common prefix/suffix; inserted chars get `insert_fmt`.
fn reconcile_formats(formats: &mut Vec<FormatFlags>, before: &str, after: &str, insert_fmt: FormatFlags) {
    let b: Vec<char> = before.chars().collect();
    let a: Vec<char> = after.chars().collect();
    formats.resize(b.len(), FormatFlags::default());

    let mut prefix = 0;
    while prefix < b.len() && prefix < a.len() && b[prefix] == a[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < b.len() - prefix
        && suffix < a.len() - prefix
        && b[b.len() - 1 - suffix] == a[a.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let removed = b.len() - prefix - suffix;
    let inserted = a.len() - prefix - suffix;
    formats.splice(
        prefix..prefix + removed,
        std::iter::repeat(insert_fmt).take(inserted),
    );
}

/// Build styled spans from plain text and per-character formats.
fn build_spans(text: String, formats: Vec<FormatFlags>) -> impl Iterator<Item = Span<'static>> {
    if text.is_empty() {
        // Keep one empty span so the paragraph layout stays in sync with the editor rope.
        return vec![Span::new("").font_size(14.).color(DARK.text)].into_iter();
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_flags = FormatFlags::default();

    for (i, ch) in text.chars().enumerate() {
        let flags = formats.get(i).copied().unwrap_or_default();
        if current.is_empty() {
            current_flags = flags;
            current.push(ch);
        } else if flags == current_flags {
            current.push(ch);
        } else {
            spans.push(make_span(std::mem::take(&mut current), current_flags));
            current.push(ch);
            current_flags = flags;
        }
    }
    if !current.is_empty() {
        spans.push(make_span(current, current_flags));
    }
    spans.into_iter()
}

fn make_span(text: String, flags: FormatFlags) -> Span<'static> {
    let mut span = Span::new(text).font_size(14.).color(DARK.text);
    if flags.bold {
        span = span.font_weight(FontWeight::BOLD);
    }
    if flags.italic {
        span = span.font_slant(FontSlant::Italic);
    }
    if flags.underline {
        span = span.text_decoration(TextDecoration::Underline);
    }
    if flags.link.is_some() {
        span = span
            .color(Color::from_rgb(88, 166, 255))
            .text_decoration(TextDecoration::Underline);
    }
    span
}

/// An inline image with a small "×" button to remove it.
fn image_thumb(
    path: String,
    on_remove: EventHandler<Event<PressEventData>>,
    width: f32,
    height: f32,
) -> Rect {
    rect()
        .width(Size::px(width))
        .height(Size::px(height))
        .corner_radius(6.)
        .background(Fill::from((20, 20, 31)))
        .child(
            ImageViewer::new(PathBuf::from(path))
                .width(Size::px(width))
                .height(Size::px(height)),
        )
        .child(
            rect()
                .position(Position::new_absolute().top(4.).right(4.))
                .padding((0., 6., 0., 6.))
                .corner_radius(10.)
                .background(Fill::from((0, 0, 0)))
                .cursor(freya::winit::window::CursorIcon::Pointer)
                .on_press(on_remove)
                .child(label().text("×").color(DARK.text).font_size(14.)),
        )
}

/// A formatting toolbar button (B, I, U, Link, ...).
#[derive(PartialEq, Clone)]
struct FormatButton {
    label_text: String,
    font_weight: FontWeight,
    active: bool,
    on_press: EventHandler<Event<PressEventData>>,
}

impl Component for FormatButton {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let label_text = self.label_text.clone();
        let font_weight = self.font_weight;
        let active = self.active;
        let on_press = self.on_press.clone();

        let bg = if active {
            Fill::from(DARK.accent)
        } else if hovering() {
            Fill::from((40, 40, 85))
        } else {
            Fill::from((25, 25, 55))
        };

        let mut style = label()
            .text(label_text)
            .color(DARK.text)
            .font_size(13.)
            .font_weight(font_weight);

        if self.label_text == "U" && active {
            style = style.text_decoration(TextDecoration::Underline);
        }
        if self.label_text == "I" && active {
            style = style.font_slant(FontSlant::Italic);
        }

        rect()
            .padding((6., 8., 6., 8.))
            .corner_radius(4.)
            .background(bg)
            .cursor(freya::winit::window::CursorIcon::Pointer)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(on_press)
            .child(style)
    }
}

fn format_button(
    text: String,
    weight: FontWeight,
    active: bool,
    on_press: EventHandler<Event<PressEventData>>,
) -> Rect {
    rect().child(FormatButton {
        label_text: text,
        font_weight: weight,
        active,
        on_press,
    })
}

/// A darker box with a thin outline around an input, so it stands out from the background.
fn field_box(input: Input) -> Rect {
    rect()
        .width(Size::flex(1.))
        .padding((2., 8., 2., 8.))
        .corner_radius(6.)
        .background(Fill::from((20, 20, 31)))
        .border(Border::new().fill((50, 50, 90)).width(1.))
        .child(input)
}

fn input_field(
    label_text: String,
    value: State<String>,
    theme: InputColorsThemePartial,
) -> Rect {
    rect()
        .width(Size::fill())
        .horizontal()
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .spacing(8.)
        .child(
            label()
                .text(label_text)
                .color(DARK.muted)
                .font_size(12.)
                .width(Size::px(60.)),
        )
        .child(field_box(
            Input::new(value)
                .flat()
                .compact()
                .width(Size::fill())
                .theme_colors(theme),
        ))
}

/// Escape a string for safe inclusion in HTML text content.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Converts bold, italic, underline, and links to inline HTML tags.
/// Images are rendered as `<img src="cid:...">` references.
fn build_html_body_with_images(
    text: &str,
    formats: &[FormatFlags],
    links: &[String],
    images: &[(char, String)],
) -> String {
    if text.is_empty() && images.is_empty() {
        return String::new();
    }

    let mut html = String::with_capacity(text.len() + 256);
    let mut current_flags = FormatFlags::default();

    for (i, ch) in text.chars().enumerate() {
        let flags = formats.get(i).copied().unwrap_or_default();

        // Close tags that are no longer active
        if current_flags.bold && !flags.bold {
            html.push_str("</strong>");
        }
        if current_flags.italic && !flags.italic {
            html.push_str("</em>");
        }
        if current_flags.underline && !flags.underline {
            html.push_str("</u>");
        }
        if current_flags.link.is_some() && current_flags.link != flags.link {
            html.push_str("</a>");
        }

        // Open tags that are now active
        if !current_flags.bold && flags.bold {
            html.push_str("<strong>");
        }
        if !current_flags.italic && flags.italic {
            html.push_str("<em>");
        }
        if !current_flags.underline && flags.underline {
            html.push_str("<u>");
        }
        if current_flags.link != flags.link {
            if let Some(link_idx) = flags.link {
                if let Some(url) = links.get(link_idx) {
                    html.push_str("<a href=\"");
                    html.push_str(&html_escape(url));
                    html.push_str("\">");
                }
            }
        }

        // Write the character or image
        if is_placeholder(ch) {
            // Render as inline image with cid: reference
            if let Some((_, path)) = images.iter().find(|(pc, _)| *pc == ch) {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image.png");
                let img_id = format!("img_{}@photonmail", u32::from(ch));
                html.push_str("<img src=\"cid:");
                html.push_str(&img_id);
                html.push_str("\" alt=\"");
                html.push_str(&html_escape(filename));
                html.push_str("\" style=\"max-width:360px;max-height:240px;border-radius:6px;\">");
            }
        } else {
            html_escape_char(ch, &mut html);
        }

        current_flags = flags;
    }

    // Close any remaining open tags
    if current_flags.bold {
        html.push_str("</strong>");
    }
    if current_flags.italic {
        html.push_str("</em>");
    }
    if current_flags.underline {
        html.push_str("</u>");
    }
    if current_flags.link.is_some() {
        html.push_str("</a>");
    }

    // Wrap lines in <p> tags for proper HTML structure.
    // Lines starting with ">" are quoted text and wrapped in <blockquote>.
    let mut final_html = String::with_capacity(html.len() + 128);
    let mut in_paragraph = false;
    let mut in_blockquote = false;
    for line in html.split('\n') {
        let is_quoted = line.trim_start().starts_with('&') && line.trim_start().starts_with("&gt;");
        if line.is_empty() {
            if in_paragraph {
                final_html.push_str("</p>");
                in_paragraph = false;
            }
            if in_blockquote {
                final_html.push_str("</blockquote>");
                in_blockquote = false;
            }
        } else {
            // Close blockquote if this line is not quoted
            if in_blockquote && !is_quoted {
                final_html.push_str("</blockquote>");
                in_blockquote = false;
            }
            // Open blockquote if this line is quoted
            if is_quoted && !in_blockquote {
                final_html.push_str("<blockquote style=\"margin:0 0 0 0.8ex;border-left:2px solid #ccc;padding-left:1ex\">");
                in_blockquote = true;
            }
            if !in_paragraph {
                final_html.push_str("<p>");
                in_paragraph = true;
            } else {
                final_html.push_str("<br>");
            }
            // Strip the leading ">" from quoted lines for HTML display
            if is_quoted {
                // Find the first "&gt;" and skip it
                if let Some(pos) = line.find("&gt;") {
                    let rest = &line[pos + 4..];
                    final_html.push_str(rest.trim_start());
                } else {
                    final_html.push_str(line);
                }
            } else {
                final_html.push_str(line);
            }
        }
    }
    if in_paragraph {
        final_html.push_str("</p>");
    }
    if in_blockquote {
        final_html.push_str("</blockquote>");
    }

    final_html
}


/// Escape a single character for HTML, appending to the given string.
fn html_escape_char(ch: char, out: &mut String) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        c => out.push(c),
    }
}

/// Decode the HTML entities we care about.
fn decode_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "copy" => Some('©'),
        "reg" => Some('®'),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "hellip" => Some('…'),
        "bull" => Some('•'),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                num.parse::<u32>().ok()?
            };
            char::from_u32(code)
        }
    }
}

/// Extract an attribute value (`href="..."`) from the inside of a tag.
fn tag_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(attr) {
        let start = from + pos;
        let before_ok = start == 0 || lower.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = tag[start + attr.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let rest = rest[1..].trim_start();
            return Some(match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or("").to_string(),
                _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string(),
            });
        }
        from = start + attr.len();
    }
    None
}

/// Convert a draft's HTML into editor text plus per-character formatting. This is the
/// inverse of `build_html_body` (strong/b, em/i, u, a[href], br, p/div); other tags are
/// ignored and script/style content is dropped.
///
/// Inline images (`<img src="cid:...">`) are converted to placeholder characters.
/// The image paths are stored in the `images` field of the returned DraftBody.
fn html_to_editor(html: &str, attachments: &[crate::mail::AttachmentInfo]) -> DraftBody {
    let mut out = DraftBody::default();
    let mut text = String::new();
    let mut formats = Vec::new();
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut underline = 0usize;
    let mut link_stack: Vec<Option<usize>> = Vec::new();
    let mut pending_nl = 0usize;
    let mut skip_until: Option<&'static str> = None;
    let mut next_img: u32 = 0;
    // True right after an image: the line break that follows it is already implied by the
    // editor's "image + Enter" convention, so the next <br> must not add a second one.
    let mut after_img = false;

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    // Flush pending paragraph breaks (only between content, never at the very start).
    fn flush(text: &mut String, formats: &mut Vec<FormatFlags>, pending: &mut usize) {
        if *pending > 0 && !text.is_empty() {
            for _ in 0..*pending {
                text.push('\n');
                formats.push(FormatFlags::default());
            }
        }
        *pending = 0;
    }

    while i < chars.len() {
        let ch = chars[i];

        if let Some(end_tag) = skip_until {
            if ch == '<' {
                let rest: String = chars[i..].iter().take(end_tag.len() + 2).collect();
                if rest.to_ascii_lowercase().starts_with(&format!("</{}", end_tag)) {
                    skip_until = None;
                }
            }
            i += 1;
            continue;
        }

        if ch == '<' {
            // HTML comment
            if chars[i..].iter().take(4).collect::<String>() == "<!--" {
                let rest: String = chars[i..].iter().collect();
                match rest.find("-->") {
                    Some(end) => {
                        i += rest[..end + 3].chars().count();
                        continue;
                    }
                    None => break,
                }
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let tag: String = chars[i + 1..j].iter().collect();
            i = j + 1;

            let closing = tag.starts_with('/');
            let body = tag.trim_start_matches('/').trim_end_matches('/').trim();
            let name = body
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();

            match name.as_str() {
                "script" | "style" | "head" if !closing => {
                    skip_until = Some(match name.as_str() {
                        "script" => "script",
                        "style" => "style",
                        _ => "head",
                    });
                }
                "strong" | "b" => {
                    if closing { bold = bold.saturating_sub(1) } else { bold += 1 }
                }
                "em" | "i" => {
                    if closing { italic = italic.saturating_sub(1) } else { italic += 1 }
                }
                "u" => {
                    if closing { underline = underline.saturating_sub(1) } else { underline += 1 }
                }
                "a" => {
                    if closing {
                        link_stack.pop();
                    } else {
                        let idx = tag_attr(body, "href").filter(|u| !u.is_empty()).map(|u| {
                            let u = decode_entities(&u);
                            out.links.push(u);
                            out.links.len() - 1
                        });
                        link_stack.push(idx);
                    }
                }
                "br" => {
                    if after_img {
                        after_img = false;
                        pending_nl = pending_nl.max(1);
                    } else {
                        flush(&mut text, &mut formats, &mut pending_nl);
                        text.push('\n');
                        formats.push(FormatFlags::default());
                    }
                }
                "p" | "div" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "table" => {
                    pending_nl = pending_nl.max(2);
                }
                "li" | "tr" => {
                    pending_nl = pending_nl.max(1);
                }
                "img" if !closing => {
                    // Inline images: resolve `cid:` references against the mail's inline parts
                    // (or decode `data:` URIs) into temp files, then show them via a placeholder.
                    if let Some(src) = tag_attr(body, "src") {
                        let src = decode_entities(&src);
                        let path = if src.get(..4).map_or(false, |s| s.eq_ignore_ascii_case("cid:")) {
                            let cid = src[4..].trim().trim_start_matches('<').trim_end_matches('>');
                            attachments
                                .iter()
                                .find(|a| {
                                    a.content_id
                                        .as_deref()
                                        .map_or(false, |id| id.eq_ignore_ascii_case(cid))
                                })
                                .and_then(|a| {
                                    a.data
                                        .as_deref()
                                        .and_then(|d| store_draft_image(d, &a.content_type, &a.name))
                                })
                        } else if src.get(..5).map_or(false, |s| s.eq_ignore_ascii_case("data:")) {
                            decode_data_uri(&src)
                                .and_then(|(ct, d)| store_draft_image(&d, &ct, "image"))
                        } else {
                            None
                        };
                        if let Some(path) = path {
                            flush(&mut text, &mut formats, &mut pending_nl);
                            let ch = char::from_u32(0xE000 + (next_img % 6400)).unwrap_or('\u{E000}');
                            next_img += 1;
                            text.push(ch);
                            formats.push(FormatFlags::default());
                            out.images.push((ch, path));
                            // The editor puts the next text on the line below the image.
                            pending_nl = pending_nl.max(1);
                            after_img = true;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        // Character data (entities decoded, whitespace collapsed like a browser would)
        let mut c = ch;
        if ch == '&' {
            let mut j = i + 1;
            while j < chars.len() && j - i <= 10 && chars[j] != ';' && chars[j] != '&' && chars[j] != '<' {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' {
                let name: String = chars[i + 1..j].iter().collect();
                if let Some(d) = decode_entity(&name) {
                    c = d;
                    i = j;
                }
            }
        }
        i += 1;

        if is_placeholder(c) || c == '\r' {
            continue;
        }
        if c.is_whitespace() && c != '\u{a0}' {
            let prev_ws = text.chars().next_back().map_or(true, |p| p == ' ' || p == '\n');
            if prev_ws || pending_nl > 0 {
                continue;
            }
            c = ' ';
        }

        after_img = false;
        flush(&mut text, &mut formats, &mut pending_nl);
        text.push(c);
        formats.push(FormatFlags {
            bold: bold > 0,
            italic: italic > 0,
            underline: underline > 0,
            link: link_stack.iter().rev().find_map(|l| *l),
        });
    }

    // A trailing image gets its own cursor line, like in the editor.
    if text.chars().next_back().map_or(false, is_placeholder) {
        text.push('\n');
        formats.push(FormatFlags::default());
    }
    out.text = text;
    out.formats = formats;
    out
}

/// Decode entities in a whole string (used for href values).
fn decode_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(end) = chars[i..].iter().take(12).position(|&c| c == ';') {
                let name: String = chars[i + 1..i + end].iter().collect();
                if let Some(d) = decode_entity(&name) {
                    out.push(d);
                    i += end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Write image bytes from a draft to a temp file (so the editor can render it and a later
/// save can embed it again). Returns the file path.
fn store_draft_image(data: &[u8], content_type: &str, name: &str) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    let ext = match content_type.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/png" => "png",
        _ => {
            // Fall back to the attachment's own extension, else png.
            let e = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
            if IMAGE_EXTENSIONS.contains(&e.as_str()) { return store_with_ext(data, name, &e); }
            "png"
        }
    };
    store_with_ext(data, name, ext)
}

fn store_with_ext(data: &[u8], name: &str, ext: &str) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    let stem: String = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    let dir = std::env::temp_dir().join("photon-mail-draft-images");
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join(format!("{:016x}-{}.{}", h.finish(), if stem.is_empty() { "image" } else { &stem }, ext));
    if !file.exists() {
        std::fs::write(&file, data).ok()?;
    }
    Some(file.to_string_lossy().to_string())
}

/// Decode `data:image/png;base64,....` into (content type, bytes).
fn decode_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine;
    let rest = uri.get(5..)?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.to_ascii_lowercase().ends_with(";base64") {
        return None;
    }
    let ct = meta.split(';').next().unwrap_or("image/png").to_string();
    let clean: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    let data = base64::engine::general_purpose::STANDARD.decode(clean).ok()?;
    Some((ct, data))
}

/// Split a saved draft's HTML body into the user's typed content and the forwarded HTML.
///
/// When a forward-draft is saved, the HTML body contains the user's message followed by
/// the forwarded HTML wrapped in a `<blockquote>`. This function finds the first
/// `<blockquote>` tag and splits the HTML there, returning (user_html, forwarded_html).
/// Returns None if no blockquote is found (i.e., not a forwarded draft).
fn split_draft_forwarded_html(html: &str) -> Option<(String, String)> {
    // Find the first <blockquote tag (case-insensitive)
    let lower = html.to_ascii_lowercase();
    let blockquote_pos = lower.find("<blockquote")?;

    // The user's content is everything before the <blockquote>
    let user_html = html[..blockquote_pos].trim().to_string();

    // The forwarded content is the <blockquote> and everything after it
    let forwarded_html = html[blockquote_pos..].to_string();

    Some((user_html, forwarded_html))
}
