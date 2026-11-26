macro_rules! icon {
    ($body:expr) => {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#9696aa" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">"##,
            $body,
            "</svg>"
        )
        .as_bytes()
    };
}

pub const INBOX: &[u8] = icon!(
    r#"<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#
);
pub const DRAFTS: &[u8] = icon!(
    r#"<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M9 13h6M9 17h4"/>"#
);
pub const SENT: &[u8] = icon!(r#"<path d="M22 2 11 13"/><path d="M22 2 15 22l-4-9-9-4z"/>"#);
pub const ARCHIVE: &[u8] = icon!(
    r#"<rect x="2" y="3" width="20" height="5" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/>"#
);
pub const SPAM: &[u8] = icon!(
    r#"<path d="M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86z"/><path d="M12 8v4M12 16h.01"/>"#
);
pub const TRASH: &[u8] = icon!(
    r#"<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M10 11v6M14 11v6"/>"#
);

pub const REPLY: &[u8] =
    icon!(r#"<polyline points="9 17 4 12 9 7"/><path d="M20 18v-2a4 4 0 0 0-4-4H4"/>"#);
pub const REPLY_ALL: &[u8] = icon!(
    r#"<polyline points="7 17 2 12 7 7"/><polyline points="12 17 7 12 12 7"/><path d="M22 18v-2a4 4 0 0 0-4-4H7"/>"#
);
pub const FORWARD: &[u8] =
    icon!(r#"<polyline points="15 17 20 12 15 7"/><path d="M4 18v-2a4 4 0 0 1 4-4h12"/>"#);
pub const CHEVRON_RIGHT: &[u8] = icon!(r#"<path d="m9 18 6-6-6-6"/>"#);
pub const CHEVRON_LEFT: &[u8] = icon!(r#"<path d="m15 18-6-6 6-6"/>"#);
pub const CHEVRON_DOWN: &[u8] = icon!(r#"<path d="m6 9 6 6 6-6"/>"#);
pub const PLUS: &[u8] = icon!(r#"<path d="M5 12h14M12 5v14"/>"#);
pub const SETTINGS: &[u8] =
    icon!(r#"<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6"/>"#);
pub const MAIL: &[u8] = icon!(
    r#"<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>"#
);
pub const PENCIL: &[u8] =
    icon!(r#"<path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>"#);
pub const TAG: &[u8] = icon!(
    r#"<path d="M12 2H2v10l9.29 9.29a1 1 0 0 0 1.41 0l8.59-8.59a1 1 0 0 0 0-1.41z"/><path d="M7 7h.01"/>"#
);
pub const SEARCH: &[u8] = icon!(r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>"#);
pub const STAR: &[u8] =
    icon!(r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z"/>"#);
pub const SEND: &[u8] = icon!(r#"<path d="m22 2-7 20-4-9-9-4z"/><path d="M22 2 11 13"/>"#);
pub const PAPERCLIP: &[u8] = icon!(
    r#"<path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.19 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48"/>"#
);
pub const DOWNLOAD: &[u8] = icon!(
    r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>"#
);
pub const IMAGE: &[u8] = icon!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>"#
);
pub const EYE: &[u8] = icon!(
    r#"<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>"#
);
pub const CLOSE: &[u8] =
    icon!(r#"<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>"#);
pub const MAXIMIZE: &[u8] = icon!(
    r#"<path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>"#
);
pub const MINIMIZE: &[u8] = icon!(
    r#"<path d="M8 3v3a2 2 0 0 1-2 2H3m18 0h-3a2 2 0 0 1-2-2V3m0 18v-3a2 2 0 0 1 2-2h3M3 16h3a2 2 0 0 1 2 2v3"/>"#
);
pub const DISK: &[u8] = icon!(
    r#"<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/>"#
);
pub const ELLIPSIS: &[u8] = icon!(
    r#"<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>"#
);

pub const FOLDER_ICONS: [&[u8]; 6] = [INBOX, DRAFTS, SENT, ARCHIVE, SPAM, TRASH];
