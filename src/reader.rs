use freya::prelude::*;
use freya::html::*;
use freya::winit::window::CursorIcon;

use crate::{
    account_view::{AccountSettings, AddAccountView, EditAccountView},
    compose::ComposePane,
    data::MailContentType,
    html_viewer::SecureHtmlViewer,
    icons::{CHEVRON_LEFT, *},
    mail::{AttachmentInfo, DRAFTS},
    model::BodyState,
    state::{AppState, View},
    theme::DARK,
    widgets::{Action, avatar, divider, icon, IconButton},
};

/// Generate a fake http URL for caching attachment image data in the ResourceCache.
/// The Blitz HTML engine only fetches http/https URLs, so we pre-populate the cache
/// with a fake URL and use it as the img src.
fn attachment_image_url(mail_idx: usize, att_idx: usize) -> String {
    format!("http://localhost/attachment/{}/{}", mail_idx, att_idx)
}

#[derive(PartialEq)]
pub struct Reader {
    pub state: AppState,
}

impl Component for Reader {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let view = *state.view.read();
        let show_back = matches!(view, View::AddAccount | View::Settings(_) | View::EditAccount(_));

        let mut col = rect().spacing(16.).padding(10.);

        // Back button when in a sub-view (not for Compose, which has its own close)
        if show_back {
            let mut s_back = state;
            col = col.child(
                rect()
                    .width(Size::px(120.))
                    .padding((6., 12., 6., 12.))
                    .corner_radius(6.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .background(crate::grain::hover())
                    .on_press(move |_| s_back.view.set(View::Mail))
                    .child(icon(CHEVRON_LEFT, 14., DARK.text))
                    .child(label().text("Back").color(DARK.text).font_size(13.)),
            );
        }

        // Compute a key for ComposePane so it remounts when the draft body loads.
        // This is essential because use_editable initializes once on mount, and we
        // need the component to remount with the new body text when the async fetch completes.
        let compose_key = {
            if matches!(view, View::Compose) {
                let draft_info = *state.draft_mail.read();
                if let Some((account, folder, uid)) = draft_info {
                    let store = state.store.read();
                    let body_loaded = store.mails.iter().any(|m| {
                        m.account == account && m.folder == folder && m.uid == uid
                            && !m.removed
                            && matches!(m.body, crate::model::BodyState::Loaded { .. })
                    });
                    format!("draft-{}-{}-{}-loaded={}", account, folder, uid, body_loaded)
                } else {
                    "new".to_string()
                }
            } else {
                String::new()
            }
        };

        // After a draft save the UID changes; keep the pinned key so the pane isn't remounted.
        let compose_key = match (state.draft_pin.read().clone(), *state.draft_mail.read()) {
            (Some((key, Some(pinned_uid))), Some((_, _, uid))) if pinned_uid == uid => key,
            _ => compose_key,
        };

        let body = match view {
            View::Mail => rect().child(MailView { state }),
            View::Compose => rect()
                .key(&compose_key)
                .child(ComposePane { state, compose_key: compose_key.clone() }),
            View::AddAccount => rect().child(AddAccountView { state }),
            View::Settings(index) => rect().child(AccountSettings { state, index }),
            View::EditAccount(index) => rect().child(EditAccountView { state, index }),
        };

        col = col.child(body.width(Size::fill()).height(Size::fill()));

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::Reader;

        rect()
            .width(Size::flex(1.))
            .height(Size::fill())
            .background(crate::grain::panel())
            .padding(26.)
            .border(freya::prelude::Border::new().width(if has_focus { 2. } else { 0. }).fill(DARK.accent))
            .child(col.width(Size::fill()).height(Size::fill()))
    }
}

#[derive(PartialEq)]
struct MailView {
    state: AppState,
}

impl Component for MailView {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let empty_text = if state.store.read().accounts.is_empty() {
            "Add an account to get started"
        } else {
            "Select a message to read"
        };
        match state.open() {
            Some(i) => {
                let (m, account) = {
                    let store = state.store.read();
                    let m = store.mails[i].clone();
                    let account = store.accounts[m.account].clone();
                    (m, account)
                };
                let mut s_archive = state;
                let mut s_trash = state;
                let mut s_unread = state;

                // For Sent/Drafts show "You → Recipient", otherwise "Sender → You"
                let direction_label = if m.folder == 1 || m.folder == 2 {
                    if m.to.is_empty() {
                        format!("{} → {}", account.addr, m.from)
                    } else {
                        format!("{} → {}", account.addr, m.to)
                    }
                } else {
                    format!("{} → {}", m.from, account.addr)
                };
                let display_from = if m.folder == 1 || m.folder == 2 {
                    if m.to.is_empty() { &m.from } else { &m.to }
                } else {
                    &m.from
                };
                let header = rect()
                    .horizontal()
                    .spacing(12.)
                    .child(avatar(display_from, account.color, 44.))
                    .child(
                        rect()
                            .spacing(2.)
                            .child(label().text(m.subject.clone()).font_size(24.).font_weight(FontWeight::BOLD).color(DARK.text))
                            .child(label().text(direction_label).color(DARK.muted))
                            .child(label().text(m.date.clone()).color(DARK.muted).font_size(12.)),
                    );

                // Hide Reply/Reply All/Forward when viewing a draft
                let is_draft = m.folder == DRAFTS;

                let mut actions = rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.);

                if !is_draft {
                    actions = actions
                        .child(Action { bytes: REPLY, text: "Reply".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) })
                        .child(Action { bytes: REPLY_ALL, text: "Reply all".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) })
                        .child(Action { bytes: FORWARD, text: "Forward".to_string(), color: DARK.text, on_press: EventHandler::new(|_| {}) });
                }

                actions = actions
                    .child(rect().width(Size::flex(1.)))
                    .child(
                        IconButton { bytes: ARCHIVE, color: DARK.muted, on_press: (move |_| s_archive.archive(i)).into() }
                    )
                    .child(
                        IconButton { bytes: TRASH, color: DARK.muted, on_press: (move |_| s_trash.trash(i)).into() }
                    )
                    .child(
                        IconButton { bytes: MAIL, color: DARK.muted, on_press: (move |_| s_unread.toggle_read(i)).into() }
                    );

                // Attachments section — clone the list so we own the data for closures
                let attachments = m.attachments.clone();
                let mail_index = i;
                let attachments_section = if !attachments.is_empty() {
                    let count = attachments.len();
                    let label_text = format!("{} attachment{}", count, if count == 1 { "" } else { "s" });

                    // Build the items container by chaining children
                    let mut items_col = rect().vertical().spacing(4.);
                    for (att_idx, att) in attachments.iter().enumerate() {
                        let name = att.name.clone();
                        let size = format_size(att.size);
                        let has_data = att.data.is_some();
                        let is_img = att.is_image();
                        let is_text = att.is_text();
                        let can_preview = is_img || is_text;
                        let att_clone = att.clone();

                        // Build preview button with state access
                        let preview_btn = if can_preview {
                            let mut s_preview = state;
                            let mi = mail_index;
                            let ai = att_idx;
                            rect()
                                .width(Size::px(28.))
                                .height(Size::px(28.))
                                .corner_radius(6.)
                                .center()
                                .cursor(CursorIcon::Pointer)
                                .background(crate::grain::hover())
                                .on_press(
                                    EventHandler::new(move |_| {
                                        s_preview.attachment_preview_mail.set(Some(mi));
                                        s_preview.attachment_preview_index.set(ai);
                                        s_preview.attachment_preview_open.set(true);
                                    })
                                )
                                .child(icon(EYE, 14., DARK.text))
                        } else {
                            rect().width(Size::px(28.))
                        };

                        items_col = items_col.child(
                            rect()
                                .horizontal()
                                .spacing(8.)
                                .cross_align(Alignment::Center)
                                .child(icon(attachment_icon(att), 16., DARK.muted))
                                .child(
                                    rect()
                                        .vertical()
                                        .child(label().text(name.clone()).color(DARK.text).font_size(12.))
                                        .child(label().text(size).color(DARK.muted).font_size(10.)),
                                )
                                .child(preview_btn)
                                .child(
                                    // Download button
                                    rect()
                                        .width(Size::px(28.))
                                        .height(Size::px(28.))
                                        .corner_radius(6.)
                                        .center()
                                        .cursor(CursorIcon::Pointer)
                                        .background(if has_data { crate::grain::hover() } else { Fill::from((30, 30, 40)) })
                                        .on_press(if has_data {
                                            EventHandler::new(move |_| {
                                                download_attachment(&att_clone);
                                            })
                                        } else {
                                            EventHandler::new(|_| {})
                                        })
                                        .child(icon(DOWNLOAD, 14., if has_data { DARK.text } else { (80, 80, 100) })),
                                ),
                        );
                    }

                    Some(
                        rect()
                            .width(Size::fill())
                            .padding(10.)
                            .corner_radius(8.)
                            .background((35, 35, 48))
                            .vertical()
                            .spacing(6.)
                            .child(
                                rect()
                                    .horizontal()
                                    .spacing(6.)
                                    .cross_align(Alignment::Center)
                                    .child(icon(PAPERCLIP, 14., DARK.muted))
                                    .child(
                                        label()
                                            .text(label_text)
                                            .color(DARK.muted)
                                            .font_size(11.),
                                    ),
                            )
                            .child(items_col),
                    )
                } else {
                    None
                };

                let body_content = match &m.body {
                    BodyState::Loaded { content_type: MailContentType::Html, text } => rect()
                        .height(Size::fill())
                        .width(Size::fill())
                        .key(i)
                        .child(SecureHtmlViewer {
                            html: text.to_string(),
                            sender_email: m.from_email.clone(),
                            state,
                        }),
                    BodyState::Loaded { text, .. } => rect()
                        .height(Size::fill())
                        .width(Size::fill())
                        .child(
                            ScrollView::new()
                                .height(Size::fill())
                                .child(paragraph().span(text.to_string()).color(DARK.text)),
                        ),
                    BodyState::Failed(err) => rect()
                        .width(Size::fill())
                        .child(label().text(format!("Couldn't load this message: {err}")).color(DARK.danger)),
                    BodyState::NotLoaded | BodyState::Loading => rect()
                        .width(Size::fill())
                        .child(label().text("Loading message…").color(DARK.muted)),
                };

                let mut col = rect()
                    .height(Size::fill())
                    .spacing(16.)
                    .child(header)
                    .child(actions)
                    .child(divider());

                if let Some(att_section) = attachments_section {
                    col = col.child(att_section);
                }

                col = col.child(body_content);

                col
            }
            None => rect()
                .expanded()
                .center()
                .spacing(12.)
                .child(icon(INBOX, 48., DARK.muted))
                .child(label().text(empty_text).color(DARK.muted)),
        }
    }
}

fn format_size(bytes: usize) -> String {
    match bytes {
        0..=1023 => format!("{} B", bytes),
        1024..=1023_999 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)),
    }
}

/// Pick an icon for an attachment based on its content type.
fn attachment_icon(att: &AttachmentInfo) -> &'static [u8] {
    if att.is_image() {
        IMAGE
    } else {
        PAPERCLIP
    }
}

/// Check if the attachment is an image type that can be previewed inline.
fn is_image_content_type(content_type: &str) -> bool {
    content_type.starts_with("image/")
}

/// Download an attachment to the user's chosen location.
fn download_attachment(att: &AttachmentInfo) {
    let data = match &att.data {
        Some(d) => d.clone(),
        None => {
            eprintln!("Attachment data not available for download: {}", att.name);
            return;
        }
    };

    // Ask the user where to save the file
    let path = rfd::FileDialog::new()
        .set_file_name(&att.name)
        .save_file();

    let Some(path) = path else {
        return;
    };

    // Write the file
    if let Err(e) = std::fs::write(&path, &data) {
        eprintln!("Failed to save attachment to {:?}: {}", path, e);
    }
}

// Extension trait methods for AttachmentInfo — defined as inherent methods below
impl AttachmentInfo {
    fn is_image(&self) -> bool {
        is_image_content_type(&self.content_type)
    }

    fn is_text(&self) -> bool {
        is_text_content_type(&self.content_type)
    }
}

/// Check if the content type is a text-based type that can be previewed.
fn is_text_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.starts_with("application/json")
        || content_type.starts_with("application/xml")
        || content_type.starts_with("application/csv")
        || content_type.starts_with("application/javascript")
        || content_type.starts_with("application/x-sh")
        || content_type.starts_with("application/x-httpd-php")
        || content_type.starts_with("application/toml")
        || content_type.starts_with("application/yaml")
        || content_type.starts_with("application/x-yaml")
        || content_type.starts_with("text/csv")
        || content_type.starts_with("text/xml")
        || content_type.starts_with("text/html")
        || content_type.starts_with("text/plain")
        || content_type.starts_with("text/markdown")
        || content_type.starts_with("text/x-")
}

/// Build the preview dialog box with header, filename, and content area.
fn preview_dialog(
    is_fullscreen: bool,
    close_preview: EventHandler<Event<PressEventData>>,
    toggle_fullscreen: EventHandler<Event<PressEventData>>,
    filename: Option<impl IntoElement>,
    content: Option<impl IntoElement>,
) -> impl IntoElement {
    let w = if is_fullscreen {
        Size::window_percent(90.)
    } else {
        Size::px(500.)
    };

    // Build the dialog body: header + separator + filename + content
    let mut body = rect()
        .width(Size::fill())
        .height(Size::fill())
        .padding(20.)
        .vertical()
        .spacing(12.);

    // Header row: title + fullscreen button + close button
    body = body.child(
        rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .spacing(8.)
            .cross_align(Alignment::Center)
            .child(label().text("Attachment Preview").font_size(16.).font_weight(FontWeight::BOLD).color(DARK.text))
            .child(rect().width(Size::flex(1.)))
            .child(
                rect()
                    .width(Size::px(28.))
                    .height(Size::px(28.))
                    .corner_radius(6.)
                    .center()
                    .cursor(CursorIcon::Pointer)
                    .background(crate::grain::hover())
                    .on_press(toggle_fullscreen)
                    .child(icon(if is_fullscreen { MINIMIZE } else { MAXIMIZE }, 14., DARK.text)),
            )
            .child(
                rect()
                    .width(Size::px(28.))
                    .height(Size::px(28.))
                    .corner_radius(6.)
                    .center()
                    .cursor(CursorIcon::Pointer)
                    .background(crate::grain::hover())
                    .on_press(close_preview)
                    .child(icon(CLOSE, 14., DARK.text)),
            ),
    );

    // Separator line
    body = body.child(
        rect()
            .width(Size::fill())
            .height(Size::px(1.))
            .background((40, 40, 54)),
    );

    // Filename label
    if let Some(fn_el) = filename {
        body = body.child(fn_el);
    }

    // Content area — fills remaining vertical space below the header
    if let Some(content_el) = content {
        body = body.child(
            rect()
                .height(Size::fill())
                .width(Size::fill())
                .child(content_el),
        );
    }

    // Root dialog rect with fixed dimensions and corner radius
    rect()
        .width(w)
        .height(if is_fullscreen { Size::window_percent(90.) } else { Size::px(400.) })
        .corner_radius(12.)
        .background(crate::grain::dialog())
        .child(body)
}

/// Attachment preview modal component.
#[derive(PartialEq)]
pub struct AttachmentPreview {
    pub state: AppState,
}

impl Component for AttachmentPreview {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let is_fullscreen = *state.attachment_preview_fullscreen.read();

        let close_preview = {
            let mut preview_open = state.attachment_preview_open.clone();
            let mut preview_fs = state.attachment_preview_fullscreen.clone();
            EventHandler::new(move |_| {
                preview_open.set(false);
                preview_fs.set(false);
            })
        };

        let toggle_fullscreen = {
            let mut preview_fs = state.attachment_preview_fullscreen.clone();
            EventHandler::new(move |_| {
                preview_fs.set(!preview_fs());
            })
        };

        let mail_idx = *state.attachment_preview_mail.read();
        let att_idx = *state.attachment_preview_index.read();

        // Look up the attachment from the store
        let attachment = {
            let store = state.store.read();
            match mail_idx {
                Some(mi) => store.mails.get(mi).and_then(|m| m.attachments.get(att_idx)).cloned(),
                None => None,
            }
        };

        // If attachment not found, show error
        let att = match attachment {
            Some(a) => a,
            None => {
                return preview_dialog(
                    is_fullscreen,
                    close_preview,
                    toggle_fullscreen,
                    Some(
                        rect()
                            .width(Size::fill())
                            .child(label().text("Attachment not found").color(DARK.danger)),
                    ),
                    None,
                );
            }
        };

        let title = att.name.clone();
        let is_img = att.is_image();
        let is_txt = att.is_text();

        // Build content area based on attachment type
        let content = if is_img {
            // Register the image bytes in the shared ResourceCache (fake http URL, since
            // Blitz only fetches http/https), then render via a component keyed by the
            // fullscreen state so its HtmlHandle is rebuilt when the mode toggles.
            let img_url = match &att.data {
                Some(data) => {
                    let url = attachment_image_url(mail_idx.unwrap_or(0), att_idx);
                    let cache = ResourceCache::get();
                    let parsed_url = url::Url::parse(&url).unwrap();
                    cache.set(parsed_url, data.clone().into());
                    Some(url)
                }
                None => None,
            };

            let key = format!("{}{}{}", att.name, img_url.clone().unwrap_or_default(), if is_fullscreen { "_fs" } else { "" });

            rect()
                .height(Size::fill())
                .width(Size::fill())
                .key(key)
                .child(AttachmentImage { url: img_url, fullscreen: is_fullscreen })
        } else if is_txt {
            // Render text content
            let text = match &att.data {
                Some(data) => String::from_utf8_lossy(data).to_string(),
                None => "Text data not available".to_string(),
            };

            rect()
                .height(Size::fill())
                .width(Size::fill())
                .key(att.name.clone())
                .child(
                    ScrollView::new()
                        .height(Size::fill())
                        .child(
                            rect()
                                .padding(10.)
                                .background((30, 30, 40))
                                .corner_radius(6.)
                                .child(
                                    label()
                                        .text(text)
                                        .color(DARK.text)
                                        .font_size(12.)
                                        .font_family("monospace"),
                                ),
                        ),
                )
        } else {
            rect()
                .expanded()
                .center()
                .child(label().text("Preview not available for this file type").color(DARK.muted))
        };

        preview_dialog(
            is_fullscreen,
            close_preview,
            toggle_fullscreen,
            Some(
                rect()
                    .width(Size::fill())
                    .padding(6.)
                    .background((40, 40, 54))
                    .corner_radius(6.)
                    .child(
                        label()
                            .text(title)
                            .color(DARK.muted)
                            .font_size(11.)
                            .max_lines(1),
                    ),
            ),
            Some(content),
        )
    }
}

/// Image body of the attachment preview. A separate component so that `use_html` runs
/// again (fresh HTML) whenever the parent re-keys it on a fullscreen toggle.
#[derive(PartialEq)]
struct AttachmentImage {
    url: Option<String>,
    fullscreen: bool,
}

impl Component for AttachmentImage {
    fn render(&self) -> impl IntoElement {
        let html = match &self.url {
            Some(url) if self.fullscreen => format!(
                r#"<html><body style="margin:0;padding:0;width:100%;height:100%;background:transparent;">
                    <div style="display:flex;align-items:center;justify-content:center;width:100%;height:100%;padding:10px;box-sizing:border-box;">
                        <img src="{}" style="width:100%;height:100%;object-fit:contain;border-radius:6px;" />
                    </div>
                </body></html>"#,
                url
            ),
            Some(url) => format!(
                r#"<html><body style="margin:0;padding:0;width:100%;height:100%;background:transparent;">
                    <div style="display:flex;align-items:center;justify-content:center;width:100%;height:100%;padding:10px;box-sizing:border-box;">
                        <img src="{}" style="max-width:100%;max-height:100%;object-fit:contain;border-radius:6px;" />
                    </div>
                </body></html>"#,
                url
            ),
            None => "<div style='color:#999;text-align:center;padding:40px;'>Image data not available</div>".to_string(),
        };

        let handle = use_html(move || HtmlSource::html(html.clone()));
        HtmlViewer::new(handle).expanded()
    }
}
