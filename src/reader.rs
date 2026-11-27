use freya::html::*;
use freya::prelude::*;
use freya::winit::window::CursorIcon;

use crate::{
    account_view::{AccountSettings, AddAccountView, EditAccountView},
    compose::ComposePane,
    data::MailContentType,
    html_viewer::SecureHtmlViewer,
    icons::{CHEVRON_LEFT, *},
    mail::{AttachmentInfo, DRAFTS, quoting},
    model::BodyState,
    settings_view::PreferencesView,
    state::{AppState, ComposePre, View},
    theme::DARK,
    widgets::{Action, IconButton, ProfileAvatar, QuoteToggle, divider, icon},
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
        let _settings = state.settings.read();
        let view = *state.view.read();
        let show_back = matches!(
            view,
            View::AddAccount | View::Preferences | View::Settings(_) | View::EditAccount(_)
        );

        let mut col = rect().spacing(16.).padding(10.);

        // Back button when in a sub-view (not for Compose, which has its own close)
        if show_back {
            let s_back = state;
            col = col.child(
                rect()
                    .width(Size::px(120.))
                    .padding((6., 12., 6., 12.))
                    .corner_radius(6.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .background(crate::grain::hover())
                    .on_press(move |_| s_back.set_view(View::Mail))
                    .child(icon(CHEVRON_LEFT, 14., DARK.text))
                    .child(label().text("Back").color(DARK.text).font_size(13.)),
            );
        }

        // Compute a key for ComposePane so it remounts when the draft body loads.
        // This is essential because use_editable initializes once on mount, and we
        // need the component to remount with the new body text when the async fetch completes.
        // Also include compose_pre state so the pane remounts when forwarding/replying,
        // ensuring compose_pre.take() runs on a fresh mount.
        let compose_key = {
            if matches!(view, View::Compose) {
                let draft_info = *state.draft_mail.read();
                let pre_gen = *state.compose_pre_gen.read();
                if let Some((account, folder, uid)) = draft_info {
                    let store = state.store.read();
                    let body_loaded = store.mails.iter().any(|m| {
                        m.account == account
                            && m.folder == folder
                            && m.uid == uid
                            && !m.removed
                            && matches!(m.body, crate::model::BodyState::Loaded { .. })
                    });
                    format!(
                        "draft-{}-{}-{}-loaded={}-gen={}",
                        account, folder, uid, body_loaded, pre_gen
                    )
                } else {
                    format!("new-gen={}", pre_gen)
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
            View::Compose => rect().key(&compose_key).child(ComposePane {
                state,
                compose_key: compose_key.clone(),
            }),
            View::AddAccount => rect().child(AddAccountView { state }),
            View::Preferences => rect().child(PreferencesView { state }),
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
            .border(
                freya::prelude::Border::new()
                    .width(if has_focus { 2. } else { 0. })
                    .fill(DARK.accent),
            )
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
        let details_open = use_state(|| false);
        let source_mail = use_state(|| None::<usize>);
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
                let details_expanded = details_open();
                let source_visible = source_mail() == Some(i);
                let source = match &m.body {
                    BodyState::Loaded { source, .. } => Some(source.as_ref()),
                    _ => None,
                };
                let header_value =
                    |name: &str| source.and_then(|raw| crate::mail::parse::header_value(raw, name));

                let sent_like = m.folder == 1 || m.folder == 2;
                let details_from = if sent_like {
                    format_detail_address(&account.sender_name, &account.addr)
                } else {
                    format_detail_address(&m.from, &m.from_email)
                };
                let details_to = if sent_like {
                    let name = m.from.strip_prefix("To: ").unwrap_or(&m.from);
                    format_detail_address(name, &m.from_email)
                } else if !m.to.is_empty() {
                    format_detail_address(&m.to, &m.to_email)
                } else if !m.to_email.is_empty() {
                    m.to_email.clone()
                } else {
                    account.addr.clone()
                };
                let date = header_value("Date").unwrap_or_else(|| m.date.clone());
                let mailed_by = header_value("Return-Path")
                    .and_then(|value| return_path_domain(&value))
                    .unwrap_or_else(|| "Not available".to_string());
                let signed_by = header_value("DKIM-Signature")
                    .and_then(|value| dkim_signing_domain(&value))
                    .map(|domain| format!("{domain} (not verified)"))
                    .unwrap_or_else(|| "Not available".to_string());

                let mut details_toggle = details_open;
                let details_button = rect()
                    .width(Size::px(52.))
                    .cursor(CursorIcon::Pointer)
                    .on_press(move |_| details_toggle.set(!details_expanded))
                    .child(
                        label()
                            .text("Details")
                            .color(DARK.accent)
                            .font_size(12.)
                            .max_lines(1),
                    );

                let (display_from, display_email) = if m.folder == 1 || m.folder == 2 {
                    if m.to.is_empty() {
                        (&m.from, &m.from_email)
                    } else {
                        (&m.to, &m.to_email)
                    }
                } else {
                    (&m.from, &m.from_email)
                };
                let sender_row = rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(
                        label()
                            .text(display_from.clone())
                            .color(DARK.text)
                            .font_size(13.),
                    )
                    .child(
                        label()
                            .text(display_email.clone())
                            .color(DARK.muted)
                            .font_size(11.),
                    );
                // Subject on its own line, then the sender row (Proton-style).
                let header = rect()
                    .width(Size::fill())
                    .spacing(12.)
                    .child(
                        label()
                            .text(m.subject.clone())
                            .font_size(22.)
                            .font_weight(FontWeight::BOLD)
                            .color(DARK.text),
                    )
                    .child(
                        rect()
                            .width(Size::fill())
                            .horizontal()
                            .content(Content::Flex)
                            .cross_align(Alignment::Center)
                            .spacing(12.)
                            .child(ProfileAvatar {
                                name: display_from.clone(),
                                email: display_email.clone(),
                                color: account.color,
                                size: 40.,
                                state,
                            })
                            .child(
                                rect()
                                    .width(Size::flex(1.))
                                    .spacing(2.)
                                    .child(sender_row)
                                    .child(
                                        label()
                                            .text(m.date.clone())
                                            .color(DARK.muted)
                                            .font_size(12.),
                                    ),
                            )
                            .child(details_button),
                    );

                let details_panel = if details_expanded {
                    let mut details = rect()
                        .width(Size::fill())
                        .padding(12.)
                        .corner_radius(8.)
                        .background(DARK.panel)
                        .vertical()
                        .spacing(7.);
                    let mut rows = vec![
                        ("from", details_from),
                        ("to", details_to),
                        ("date", date),
                        ("subject", m.subject.clone()),
                        ("mailed-by", mailed_by),
                        ("signed-by", signed_by),
                        (
                            "security",
                            "Transport security cannot be determined from the message".to_string(),
                        ),
                    ];
                    if !m.cc_email.is_empty() {
                        rows.push(("cc", m.cc_email.clone()));
                    }
                    if !m.bcc_email.is_empty() {
                        rows.push(("bcc", m.bcc_email.clone()));
                    }
                    if !m.reply_to.is_empty() {
                        rows.push(("reply-to", m.reply_to.clone()));
                    }
                    for (key, value) in rows {
                        details =
                            details.child(
                                rect()
                                    .width(Size::fill())
                                    .horizontal()
                                    .cross_align(Alignment::Start)
                                    .spacing(10.)
                                    .child(
                                        rect().width(Size::px(86.)).child(
                                            label().text(key).color(DARK.muted).font_size(12.),
                                        ),
                                    )
                                    .child(rect().width(Size::flex(1.)).child(
                                        label().text(value).color(DARK.text).font_size(12.),
                                    )),
                            );
                    }
                    let mut s_source = source_mail;
                    details = details.child(
                        rect()
                            .cursor(CursorIcon::Pointer)
                            .on_press(move |_| {
                                s_source.set(if source_visible { None } else { Some(i) })
                            })
                            .child(
                                label()
                                    .text(if source_visible {
                                        "Hide source"
                                    } else {
                                        "Inspect source"
                                    })
                                    .color(DARK.accent)
                                    .font_size(12.),
                            ),
                    );
                    Some(details)
                } else {
                    None
                };

                // Hide Reply/Reply All/Forward when viewing a draft
                let is_draft = m.folder == DRAFTS;

                let mut actions = rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.);

                if !is_draft {
                    let reply_account = m.account;
                    let orig_message_id = m.message_id.clone();
                    let orig_in_reply_to = m.in_reply_to.clone();
                    let orig_from_name = m.from.clone();
                    let orig_date = m.date.clone();

                    // Reply button — look up body at click time so async-loaded content is available.
                    let mut s_reply = state;
                    let reply_mail_idx = i;
                    let reply_to = reply_recipient(&m);
                    let reply_subject = m.subject.clone();
                    let reply_from_name = orig_from_name.clone();
                    let reply_date = orig_date.clone();
                    let reply_msg_id = orig_message_id.clone();
                    let reply_in_reply = orig_in_reply_to.clone();
                    actions = actions.child(Action {
                        bytes: REPLY,
                        text: "Reply".to_string(),
                        color: DARK.text,
                        on_press: EventHandler::new(move |_| {
                            let reply_body = {
                                let store = s_reply.store.read();
                                if let Some(mail) = store.mails.get(reply_mail_idx) {
                                    match &mail.body {
                                        BodyState::Loaded {
                                            text, content_type, ..
                                        } => {
                                            if *content_type == MailContentType::Html {
                                                crate::mail::parse::strip_html_tags(text)
                                            } else {
                                                text.to_string()
                                            }
                                        }
                                        _ => String::new(),
                                    }
                                } else {
                                    String::new()
                                }
                            };
                            let reply_refs = s_reply
                                .store
                                .read()
                                .mails
                                .get(reply_mail_idx)
                                .map(references_chain)
                                .unwrap_or_default();
                            s_reply.compose_pre.set(Some(ComposePre {
                                to: reply_to.clone(),
                                cc: String::new(),
                                subject: subject_with_prefix("Re", &reply_subject),
                                body: build_reply_body(&reply_from_name, &reply_date, &reply_body),
                                html_body: None,
                                forwarded_plain: None,
                                from_account: reply_account,
                                original_message_id: reply_msg_id.clone(),
                                original_in_reply_to: reply_in_reply.clone(),
                                original_references: reply_refs,
                            }));
                            *s_reply.compose_pre_gen.write() += 1;
                            s_reply.draft_mail.set(None);
                            s_reply.draft_pin.set(None);
                            s_reply.set_view(View::Compose);
                        }),
                    });

                    // Reply All button — look up body at click time.
                    let mut s_reply_all = state;
                    let reply_all_mail_idx = i;
                    let reply_all_to = reply_recipient(&m);
                    let reply_all_cc = reply_all_cc_list(&m, &account.addr, &reply_all_to);
                    let reply_all_subject = m.subject.clone();
                    let reply_all_from_name = orig_from_name.clone();
                    let reply_all_date = orig_date.clone();
                    let reply_all_msg_id = orig_message_id.clone();
                    let reply_all_in_reply = orig_in_reply_to.clone();
                    actions = actions.child(Action {
                        bytes: REPLY_ALL,
                        text: "Reply all".to_string(),
                        color: DARK.text,
                        on_press: EventHandler::new(move |_| {
                            let reply_all_body = {
                                let store = s_reply_all.store.read();
                                if let Some(mail) = store.mails.get(reply_all_mail_idx) {
                                    match &mail.body {
                                        BodyState::Loaded {
                                            text, content_type, ..
                                        } => {
                                            if *content_type == MailContentType::Html {
                                                crate::mail::parse::strip_html_tags(text)
                                            } else {
                                                text.to_string()
                                            }
                                        }
                                        _ => String::new(),
                                    }
                                } else {
                                    String::new()
                                }
                            };
                            let reply_all_refs = s_reply_all
                                .store
                                .read()
                                .mails
                                .get(reply_all_mail_idx)
                                .map(references_chain)
                                .unwrap_or_default();
                            s_reply_all.compose_pre.set(Some(ComposePre {
                                to: reply_all_to.clone(),
                                cc: reply_all_cc.clone(),
                                subject: subject_with_prefix("Re", &reply_all_subject),
                                body: build_reply_body(
                                    &reply_all_from_name,
                                    &reply_all_date,
                                    &reply_all_body,
                                ),
                                html_body: None,
                                forwarded_plain: None,
                                from_account: reply_account,
                                original_message_id: reply_all_msg_id.clone(),
                                original_in_reply_to: reply_all_in_reply.clone(),
                                original_references: reply_all_refs,
                            }));
                            *s_reply_all.compose_pre_gen.write() += 1;
                            s_reply_all.draft_mail.set(None);
                            s_reply_all.draft_pin.set(None);
                            s_reply_all.set_view(View::Compose);
                        }),
                    });

                    // Forward button — look up the body at click time so async-loaded HTML is available.
                    let mut s_forward = state;
                    let forward_mail_idx = i;
                    let forward_account = m.account;
                    let forward_subject = m.subject.clone();
                    actions = actions.child(Action {
                        bytes: FORWARD,
                        text: "Forward".to_string(),
                        color: DARK.text,
                        on_press: EventHandler::new(move |_| {
                            // Look up the latest body from the store at click time
                            let (forward_body_text, forward_html_body) = {
                                let store = s_forward.store.read();
                                if let Some(mail) = store.mails.get(forward_mail_idx) {
                                    match &mail.body {
                                        BodyState::Loaded {
                                            text, content_type, ..
                                        } => {
                                            if *content_type == MailContentType::Html {
                                                (
                                                    crate::mail::parse::strip_html_tags(text),
                                                    Some(text.to_string()),
                                                )
                                            } else {
                                                (text.to_string(), None)
                                            }
                                        }
                                        _ => (String::new(), None),
                                    }
                                } else {
                                    (String::new(), None)
                                }
                            };
                            let html_body_val = forward_html_body
                                .as_ref()
                                .map(|h| build_forward_body_html(h));
                            s_forward.compose_pre.set(Some(ComposePre {
                                to: String::new(),
                                cc: String::new(),
                                subject: subject_with_prefix("Fwd", &forward_subject),
                                body: if forward_html_body.is_some() {
                                    String::new()
                                } else {
                                    build_forward_body(&forward_body_text)
                                },
                                html_body: html_body_val,
                                forwarded_plain: forward_html_body
                                    .as_ref()
                                    .map(|_| build_forward_body(&forward_body_text)),
                                from_account: forward_account,
                                original_message_id: String::new(),
                                original_in_reply_to: String::new(),
                                original_references: String::new(),
                            }));
                            *s_forward.compose_pre_gen.write() += 1;
                            s_forward.draft_mail.set(None);
                            s_forward.draft_pin.set(None);
                            s_forward.set_view(View::Compose);
                        }),
                    });
                }

                actions = actions
                    .child(rect().width(Size::flex(1.)))
                    .child(IconButton {
                        bytes: ARCHIVE,
                        color: DARK.muted,
                        on_press: (move |_| s_archive.archive(i)).into(),
                    })
                    .child(IconButton {
                        bytes: TRASH,
                        color: DARK.muted,
                        on_press: (move |_| s_trash.trash(i)).into(),
                    })
                    .child(IconButton {
                        bytes: MAIL,
                        color: DARK.muted,
                        on_press: (move |_| s_unread.toggle_read(i)).into(),
                    });

                // Attachments section — clone the list so we own the data for closures
                let attachments = m.attachments.clone();
                let mail_index = i;
                let attachments_section = if !attachments.is_empty() {
                    let count = attachments.len();
                    let label_text =
                        format!("{} attachment{}", count, if count == 1 { "" } else { "s" });

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
                                .on_press(EventHandler::new(move |_| {
                                    s_preview.attachment_preview_mail.set(Some(mi));
                                    s_preview.attachment_preview_index.set(ai);
                                    s_preview.attachment_preview_open.set(true);
                                }))
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
                                        .child(
                                            label()
                                                .text(name.clone())
                                                .color(DARK.text)
                                                .font_size(12.),
                                        )
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
                                        .background(if has_data {
                                            crate::grain::hover()
                                        } else {
                                            Fill::from(DARK.panel)
                                        })
                                        .on_press(if has_data {
                                            EventHandler::new(move |_| {
                                                download_attachment(&att_clone);
                                            })
                                        } else {
                                            EventHandler::new(|_| {})
                                        })
                                        .child(icon(
                                            DOWNLOAD,
                                            14.,
                                            if has_data { DARK.text } else { DARK.muted },
                                        )),
                                ),
                        );
                    }

                    Some(
                        rect()
                            .width(Size::fill())
                            .padding(10.)
                            .corner_radius(8.)
                            .background(DARK.panel)
                            .vertical()
                            .spacing(6.)
                            .child(
                                rect()
                                    .horizontal()
                                    .spacing(6.)
                                    .cross_align(Alignment::Center)
                                    .child(icon(PAPERCLIP, 14., DARK.muted))
                                    .child(
                                        label().text(label_text).color(DARK.muted).font_size(11.),
                                    ),
                            )
                            .child(items_col),
                    )
                } else {
                    None
                };

                // Quoted-text collapsing only applies to actual replies.
                let is_reply = quoting::is_reply(&m.in_reply_to, &m.subject);

                let body_content = if source_visible {
                    match &m.body {
                        BodyState::Loaded { source, .. } if !source.is_empty() => ScrollView::new()
                            .height(Size::fill())
                            .child(
                                rect().padding(12.).background(DARK.panel).child(
                                    label()
                                        .text(String::from_utf8_lossy(source).into_owned())
                                        .color(DARK.text)
                                        .font_size(12.)
                                        .font_family("monospace"),
                                ),
                            )
                            .into(),
                        BodyState::Loaded { .. } => rect()
                            .width(Size::fill())
                            .child(
                                label()
                                    .text("Original source is unavailable for this message.")
                                    .color(DARK.muted),
                            )
                            .into(),
                        _ => rect()
                            .width(Size::fill())
                            .child(
                                label()
                                    .text("Loading original message source…")
                                    .color(DARK.muted),
                            )
                            .into(),
                    }
                } else {
                    match &m.body {
                        BodyState::Loaded {
                            content_type: MailContentType::Html,
                            text,
                            ..
                        } => {
                            let expanded = state.quoted_expanded.read().contains(&i);
                            // For replies, put a "..." button inside the email where the quote starts
                            match is_reply
                                .then(|| quoting::prepare_html_quotes(text, expanded))
                                .flatten()
                            {
                                Some(html) => {
                                    build_collapsed_html_body(state, i, html, m.from_email.clone())
                                }
                                None => render_full_body(
                                    text.to_string(),
                                    true,
                                    state,
                                    m.from_email.clone(),
                                ),
                            }
                        }
                        BodyState::Loaded { text, .. } => {
                            // For plain-text replies, split into new content and quoted content
                            let split = if is_reply {
                                quoting::split_plain_text(text)
                            } else {
                                quoting::QuotedSplit::default()
                            };
                            if split.has_quotes {
                                build_collapsed_text_body(
                                    state,
                                    i,
                                    split.new_content,
                                    split.quoted_content,
                                )
                            } else {
                                render_full_body(text.to_string(), false, state, String::new())
                            }
                        }
                        BodyState::Failed(err) => rect()
                            .width(Size::fill())
                            .child(
                                label()
                                    .text(format!("Couldn't load this message: {err}"))
                                    .color(DARK.danger),
                            )
                            .into(),
                        BodyState::NotLoaded | BodyState::Loading => rect()
                            .width(Size::fill())
                            .child(label().text("Loading message…").color(DARK.muted))
                            .into(),
                    }
                };

                let mut col = rect()
                    .height(Size::fill())
                    .spacing(16.)
                    .child(header)
                    .child(actions);

                if let Some(details_panel) = details_panel {
                    col = col.child(details_panel);
                }
                col = col.child(divider());

                if let Some(att_section) = attachments_section {
                    col = col.child(att_section);
                }

                // The HTML viewer loads its document once, so key the body by message and
                // quote state to rebuild it when either changes.
                let body_key = format!("body-{}-{}", i, state.quoted_expanded.read().contains(&i));
                col = col.child(
                    rect()
                        .key(body_key)
                        .width(Size::fill())
                        .height(Size::fill())
                        .child(body_content),
                );

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

fn format_detail_address(name: &str, email: &str) -> String {
    let name = name.trim();
    let email = email.trim();
    match (name.is_empty(), email.is_empty()) {
        (true, true) => "Not available".to_string(),
        (false, true) => name.to_string(),
        (true, false) => email.to_string(),
        (false, false) if name.eq_ignore_ascii_case(email) => email.to_string(),
        (false, false) => format!("{name} <{email}>"),
    }
}

fn return_path_domain(value: &str) -> Option<String> {
    let address = value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>').map(|(address, _)| address))
        .unwrap_or(value)
        .trim();
    let (_, domain) = address.rsplit_once('@')?;
    let domain = domain.trim().trim_matches('"');
    (!domain.is_empty()).then(|| domain.to_string())
}

fn dkim_signing_domain(value: &str) -> Option<String> {
    value.split(';').find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("d") {
            return None;
        }
        let domain = value.trim().trim_matches('"');
        (!domain.is_empty()).then(|| domain.to_string())
    })
}

/// Build the quoted reply body with the original message below.
/// Format: "On {date}, {sender} wrote:"
fn build_reply_body(from_name: &str, date: &str, body: &str) -> String {
    if body.trim().is_empty() {
        String::new()
    } else {
        let mut quoted = String::with_capacity(body.len() + 256);
        quoted.push_str(&format!("On {}, {} wrote:\n", date, from_name));
        for line in body.lines() {
            quoted.push('>');
            quoted.push_str(line);
            quoted.push('\n');
        }
        quoted
    }
}

fn subject_with_prefix(prefix: &str, subject: &str) -> String {
    let prefixes: &[&str] = match prefix {
        "Re" => &["re"],
        "Fwd" => &["fwd", "fw"],
        _ => &[prefix],
    };
    if prefixes
        .iter()
        .any(|prefix| has_subject_prefix(subject, prefix))
    {
        subject.to_string()
    } else {
        format!("{prefix}: {subject}")
    }
}

fn has_subject_prefix(subject: &str, prefix: &str) -> bool {
    let subject = subject.trim_start().to_ascii_lowercase();
    if subject.starts_with(&format!("{prefix}:")) {
        return true;
    }
    let Some(suffix) = subject.strip_prefix(&format!("{prefix}[")) else {
        return false;
    };
    let Some((number, _)) = suffix.split_once("]:") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

/// Build the forwarded message body with the original message below.
fn build_forward_body(body: &str) -> String {
    if body.trim().is_empty() {
        String::new()
    } else {
        let mut forwarded = String::with_capacity(body.len() + 64);
        forwarded.push_str("--- Forwarded message ---\n");
        for line in body.lines() {
            forwarded.push('>');
            forwarded.push_str(line);
            forwarded.push('\n');
        }
        forwarded
    }
}

/// Build the forwarded message HTML body with the original HTML wrapped in a blockquote.
fn build_forward_body_html(html: &str) -> String {
    if html.trim().is_empty() {
        String::new()
    } else {
        let safe_html = crate::html_viewer::sanitize_forwarded_html(html);
        format!(
            "<p data-photonmail-forwarded=\"true\">--- Forwarded message ---</p><blockquote style=\"margin:0 0 0 0.8ex;border-left:2px solid #ccc;padding-left:1ex\">{}</blockquote>",
            safe_html
        )
    }
}

/// Render a reply's HTML body. `html` already contains the spacer where the "..." button goes
/// (at the end of the new content) and, when collapsed, the CSS hiding the quoted part; see
/// `quoting::prepare_html_quotes`. `SecureHtmlViewer` draws the native button over the spacer.
fn build_collapsed_html_body(
    state: AppState,
    mail_idx: usize,
    html: String,
    sender_email: String,
) -> Element {
    rect()
        .height(Size::fill())
        .width(Size::fill())
        .child(SecureHtmlViewer {
            html,
            sender_email,
            state,
            quote_toggle_idx: Some(mail_idx),
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_and_forward_subjects_do_not_get_duplicate_prefixes() {
        assert_eq!(
            subject_with_prefix("Re", "Re: Project update"),
            "Re: Project update"
        );
        assert_eq!(
            subject_with_prefix("Re", "RE[2]: Project update"),
            "RE[2]: Project update"
        );
        assert_eq!(
            subject_with_prefix("Re", "Project update"),
            "Re: Project update"
        );
        assert_eq!(
            subject_with_prefix("Fwd", "FW: Project update"),
            "FW: Project update"
        );
        assert_eq!(
            subject_with_prefix("Fwd", "Project update"),
            "Fwd: Project update"
        );
    }
}

/// Render a plain-text reply: new content → "..." button → quoted content (when expanded).
fn build_collapsed_text_body(
    state: AppState,
    mail_idx: usize,
    new_content: String,
    quoted_content: String,
) -> Element {
    let is_expanded = state.quoted_expanded.read().contains(&mail_idx);

    let mut col = rect().vertical().spacing(4.);

    // New content
    if !new_content.is_empty() {
        let text = quoting::strip_quote_chars(&new_content);
        col = col.child(
            rect()
                .width(Size::fill())
                .padding(8.)
                .child(paragraph().span(text).color(DARK.text)),
        );
    }

    // "..." button directly below the new content
    col = col.child(
        rect()
            .width(Size::fill())
            .padding((4., 0., 4., 0.))
            .horizontal()
            .child(QuoteToggle { state, mail_idx }),
    );

    // Quoted content (when expanded)
    if is_expanded && !quoted_content.is_empty() {
        let text = quoting::strip_quote_chars(&quoted_content);
        col = col.child(
            rect()
                .width(Size::fill())
                .padding((8., 12., 8., 12.))
                .background(DARK.panel)
                .corner_radius(4.)
                .horizontal()
                .child(
                    rect()
                        .width(Size::px(3.))
                        .height(Size::fill())
                        .background(DARK.divider)
                        .corner_radius(2.),
                )
                .child(
                    rect().width(Size::fill()).child(
                        label()
                            .text(text)
                            .color(DARK.muted)
                            .font_size(12.)
                            .font_family("monospace"),
                    ),
                ),
        );
    }

    rect()
        .height(Size::fill())
        .width(Size::fill())
        .child(ScrollView::new().height(Size::fill()).child(col))
        .into()
}

/// Render a full email body (HTML or plain text) without quote collapsing.
fn render_full_body(
    content: String,
    is_html: bool,
    state: AppState,
    sender_email: String,
) -> Element {
    if is_html {
        rect()
            .height(Size::fill())
            .width(Size::fill())
            .child(SecureHtmlViewer {
                html: content,
                sender_email,
                state,
                quote_toggle_idx: None,
            })
            .into()
    } else {
        let text = quoting::strip_quote_chars(&content);
        rect()
            .width(Size::fill())
            .padding(8.)
            .child(paragraph().span(text).color(DARK.text))
            .into()
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
    if att.is_image() { IMAGE } else { PAPERCLIP }
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
    let path = rfd::FileDialog::new().set_file_name(&att.name).save_file();

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
            .child(
                label()
                    .text("Attachment Preview")
                    .font_size(16.)
                    .font_weight(FontWeight::BOLD)
                    .color(DARK.text),
            )
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
                    .child(icon(
                        if is_fullscreen { MINIMIZE } else { MAXIMIZE },
                        14.,
                        DARK.text,
                    )),
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
            .background(DARK.divider),
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
        .height(if is_fullscreen {
            Size::window_percent(90.)
        } else {
            Size::px(400.)
        })
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
                Some(mi) => store
                    .mails
                    .get(mi)
                    .and_then(|m| m.attachments.get(att_idx))
                    .cloned(),
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

            let key = format!(
                "{}{}{}",
                att.name,
                img_url.clone().unwrap_or_default(),
                if is_fullscreen { "_fs" } else { "" }
            );

            rect()
                .height(Size::fill())
                .width(Size::fill())
                .key(key)
                .child(AttachmentImage {
                    url: img_url,
                    fullscreen: is_fullscreen,
                })
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
                    ScrollView::new().height(Size::fill()).child(
                        rect()
                            .padding(10.)
                            .background(DARK.panel)
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
            rect().expanded().center().child(
                label()
                    .text("Preview not available for this file type")
                    .color(DARK.muted),
            )
        };

        preview_dialog(
            is_fullscreen,
            close_preview,
            toggle_fullscreen,
            Some(
                rect()
                    .width(Size::fill())
                    .padding(6.)
                    .background(DARK.hover)
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

/// Where a reply goes: the sender's Reply-To (RFC 5322 §3.6.2) when set, otherwise the sender.
fn reply_recipient(m: &crate::model::Mail) -> String {
    if m.reply_to.is_empty() {
        m.from_email.clone()
    } else {
        m.reply_to.clone()
    }
}

/// Cc list for "Reply all": every original To and Cc recipient except the person the reply is
/// already addressed to, the user's own address, and duplicates.
fn reply_all_cc_list(m: &crate::model::Mail, own_addr: &str, primary: &str) -> String {
    let split = |list: &str| -> Vec<String> {
        list.split(',')
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect()
    };
    let own = own_addr.trim().to_lowercase();
    let mut skip: Vec<String> = split(primary).iter().map(|a| a.to_lowercase()).collect();
    skip.push(own);
    let mut out: Vec<String> = Vec::new();
    for addr in split(&m.to_email).into_iter().chain(split(&m.cc_email)) {
        let key = addr.to_lowercase();
        if skip.contains(&key) {
            continue;
        }
        skip.push(key);
        out.push(addr);
    }
    out.join(", ")
}

/// The ancestors of `m` for a reply's References header: its own References chain, or its
/// In-Reply-To when the message has none. Space-separated ids, oldest first.
fn references_chain(m: &crate::model::Mail) -> String {
    if !m.references.is_empty() {
        m.references.join(" ")
    } else {
        m.in_reply_to.clone()
    }
}
