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
    icons::{CHEVRON_RIGHT, SEND},
    state::AppState,
    theme::DARK,
    widgets::{divider, icon, IconButton},
};

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

type LineMouse = (usize, ParagraphHolder, Event<MouseEventData>);

#[derive(PartialEq)]
pub struct ComposePane {
    pub state: AppState,
}

impl Component for ComposePane {
    fn render(&self) -> impl IntoElement {
        let state = self.state.clone();

        let to = use_state(|| String::new());
        let cc = use_state(|| String::new());
        let bcc = use_state(|| String::new());
        let subject = use_state(|| String::new());

        let show_cc = use_state(|| false);
        let show_bcc = use_state(|| false);

        // One editor for the whole message. Images are placeholder characters in its text.
        let mut editable = use_editable(|| String::new(), EditableConfig::new);
        let a11y_id = use_a11y();
        // One entry per character of the text.
        let formats = use_state(|| Vec::<FormatFlags>::new());
        // Format applied to newly typed text.
        let cursor_format = use_state(|| FormatFlags::default());

        // Links: URLs referenced by FormatFlags::link.
        let links = use_state(|| Vec::<String>::new());
        let show_link_bar = use_state(|| false);
        let link_url = use_state(|| String::new());

        // Images: placeholder character -> file path.
        let images = use_state(|| Vec::<(char, String)>::new());
        let next_img = use_state(|| 0u32);

        let close_on_press: EventHandler<Event<PressEventData>> = {
            let mut s = state.clone();
            EventHandler::new(move |_| {
                s.view.set(crate::state::View::Mail);
            })
        };

        let send_on_press: EventHandler<Event<PressEventData>> = {
            let mut s = state.clone();
            EventHandler::new(move |_| {
                // TODO: actually send the message
                s.view.set(crate::state::View::Mail);
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
        let header = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .child(label().text("New message").font_size(14.).font_weight(FontWeight::BOLD).color(DARK.text))
            .child(rect().width(Size::flex(1.)))
            .child(IconButton {
                bytes: CHEVRON_RIGHT,
                color: DARK.muted,
                on_press: close_on_press,
            })
            .child(
                rect()
                    .padding((6., 14., 6., 14.))
                    .corner_radius(6.)
                    .background(Fill::from(DARK.success))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(6.)
                    .cursor(freya::winit::window::CursorIcon::Pointer)
                    .on_press(send_on_press)
                    .child(icon(SEND, 14., DARK.text))
                    .child(label().text("Send").color(DARK.text).font_size(13.).font_weight(FontWeight::BOLD)),
            );

        // --- To / CC / BCC / Subject ---
        let fields = rect()
            .width(Size::fill())
            .vertical()
            .spacing(8.)
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
            .child(input_field("Subject".to_string(), subject.clone(), dark_input.clone()));

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

            lines_column = lines_column.child(row_rect);
        }

        let editor_container = rect()
            .width(Size::fill())
            .height(Size::flex(1.))
            .padding(10.)
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

        let body_area = rect()
            .width(Size::fill())
            .height(Size::flex(1.))
            .vertical()
            .content(Content::Flex) // needed so flex(1.) children resolve
            .spacing(6.)
            .child(toolbar)
            .child(link_bar)
            .child(editor_container);

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
            Fill::from((45, 45, 62))
        } else {
            Fill::from((30, 30, 46))
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
        .border(Border::new().fill((58, 58, 78)).width(1.))
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
