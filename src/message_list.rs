use std::{cell::Cell, rc::Rc, time::Duration};

use freya::prelude::*;
use freya_components::{
    input::InputColorsThemePartial,
    scrollviews::{ScrollConfig, VirtualScrollView, use_scroll_controller},
};

use crate::{
    data::FOLDERS,
    icons::*,
    mail::{DRAFTS, SENT},
    state::{AppState, mail_matches_search_query},
    theme::DARK,
    widgets::{Chip, ProfileAvatar, divider, dot, icon},
};

const EMAILS_PER_PAGE: usize = 50;
/// The loaded window moves when the first visible row is within this many rows of its top
/// (and there are newer messages)...
const NEWER_TRIGGER_TOP_ROW: usize = 6;
/// ...or within this many rows of its bottom (and there are older messages). The viewport
/// must be shorter than `EMAILS_PER_PAGE - OLDER_TRIGGER_TOP_ROWS_FROM_END` rows (~34),
/// so the window always moves well before the user can reach unloaded rows.
const OLDER_TRIGGER_TOP_ROWS_FROM_END: usize = 16;
/// When the window moves it is placed so the first visible row sits at this window row,
/// i.e. far from both triggers, which also makes large jumps (dragging the scrollbar)
/// a single move.
const WINDOW_ANCHOR_ROW: usize = 20;
const SEARCH_BATCH_SIZE: usize = 100;
const MESSAGE_ROW_HEIGHT: f32 = 90.;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct SearchScope {
    account: Option<usize>,
    folder: usize,
    unread_only: bool,
    starred_only: bool,
}

#[derive(Clone, Default)]
struct SearchProgress {
    query: String,
    scope: SearchScope,
    generation: u64,
    results: Vec<usize>,
    done: bool,
}

impl SearchProgress {
    fn append_matches(&mut self, matches: impl IntoIterator<Item = usize>) {
        self.results.extend(matches);
    }
}

#[derive(Clone, Copy)]
struct SearchCandidate {
    index: usize,
    account: usize,
    folder: usize,
    uid: u32,
}

fn search_candidates(state: AppState, indices: &[usize]) -> Vec<SearchCandidate> {
    let store = state.store.read();
    indices
        .iter()
        .filter_map(|&index| {
            store.mails.get(index).map(|mail| SearchCandidate {
                index,
                account: mail.account,
                folder: mail.folder,
                uid: mail.uid,
            })
        })
        .collect()
}

fn start_progressive_search(
    state: AppState,
    mut progress: State<SearchProgress>,
    query: String,
    generation: u64,
    candidates: Vec<SearchCandidate>,
) {
    spawn(async move {
        for batch in candidates.chunks(SEARCH_BATCH_SIZE) {
            if progress.peek().generation != generation
                || state.search_query.peek().trim().to_lowercase() != query
            {
                return;
            }

            let matches = {
                let store = state.store.read();
                batch
                    .iter()
                    .filter_map(|candidate| {
                        let index = if candidate.uid == 0 {
                            candidate.index
                        } else {
                            store.mails.iter().position(|mail| {
                                mail.account == candidate.account
                                    && mail.folder == candidate.folder
                                    && mail.uid == candidate.uid
                            })?
                        };
                        let mail = store.mails.get(index)?;
                        (!mail.removed && mail_matches_search_query(mail, &query)).then_some(index)
                    })
                    .collect::<Vec<_>>()
            };

            if !matches.is_empty() {
                if state.search_query.peek().trim().to_lowercase() != query {
                    return;
                }
                let mut progress = progress.write();
                if progress.generation != generation {
                    return;
                }
                progress.append_matches(matches);
            }

            futures_timer::Delay::new(Duration::from_millis(16)).await;
        }

        let mut progress = progress.write();
        if progress.generation == generation {
            progress.done = true;
        }
    });
}

pub struct MessageList {
    pub state: AppState,
}

fn first_email_address(email_list: &str) -> &str {
    email_list.split(',').next().unwrap_or_default().trim()
}

fn message_window(indices: &[usize], start: usize) -> &[usize] {
    let start = start.min(indices.len());
    let end = start.saturating_add(EMAILS_PER_PAGE).min(indices.len());
    &indices[start..end]
}

/// First visible row (index into the whole list) for a scroll position, which is negative
/// when scrolled down.
fn top_visible_row(position: i32) -> usize {
    (-position).max(0) as usize / MESSAGE_ROW_HEIGHT as usize
}

/// Where the loaded window should move to, if it should move, given the first visible
/// row of the whole list. The window is placed around the viewport, so this works for
/// small scrolls and for large jumps alike.
fn target_window_start(available: usize, start: usize, top_row: usize) -> Option<usize> {
    let last_start = available.saturating_sub(EMAILS_PER_PAGE);
    let has_newer = start > 0;
    let has_older = start < last_start;
    let near_top = has_newer && top_row <= start + NEWER_TRIGGER_TOP_ROW;
    let near_bottom = has_older
        && top_row + OLDER_TRIGGER_TOP_ROWS_FROM_END >= start + EMAILS_PER_PAGE;
    if !(near_top || near_bottom) {
        return None;
    }
    let target = top_row.saturating_sub(WINDOW_ANCHOR_ROW).min(last_start);
    (target != start).then_some(target)
}

/// The current window, shared with the scroll watcher task through an `Rc`. Written on
/// every render; the task only ever reads it, so it never acts on a stale window.
#[derive(Clone, Copy, Default)]
struct LiveWindow {
    start: usize,
    available: usize,
    generation: u64,
}

fn clamped_message_window_start(available: usize, start: usize) -> usize {
    start.min(available.saturating_sub(EMAILS_PER_PAGE))
}

impl PartialEq for MessageList {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

impl Component for MessageList {
    fn render(&self) -> impl IntoElement {
        let mut state = self.state;
        let unread_only = *state.unread_only.read();
        let starred_only = *state.starred_only.read();
        let acc = *state.account.read();
        let fol = *state.folder.read();
        let scope = SearchScope {
            account: acc,
            folder: fol,
            unread_only,
            starred_only,
        };
        let mut s_filter = state;
        let mut s_all = state;
        let search_query = state.search_query.clone();

        // Track whether the search input has focus so the keyboard handler can skip navigation
        let search_a11y_id = use_a11y();
        let search_focus = use_focus(search_a11y_id);
        {
            let mut fs = state.focus_state.write();
            fs.search_input_focused = search_focus.read().is_focused();
        }

        // Search the full current view independently of the 30-row list window.
        let query_text = search_query.peek().clone();
        let query = query_text.trim().to_lowercase();
        let all_visible = state.visible();
        let mut search_progress = use_state(SearchProgress::default);
        let mut message_window_start = use_state(|| 0usize);
        let mut list_generation = use_state(|| 0u64);
        let mut scroll_controller = use_scroll_controller(ScrollConfig::default);
        let live_window = use_state(|| Rc::new(Cell::new(LiveWindow::default())))
            .read()
            .clone();
        let previous_progress = search_progress.read().clone();
        if previous_progress.query != query || previous_progress.scope != scope {
            let generation = previous_progress.generation.wrapping_add(1);
            let progress = SearchProgress {
                query: query.clone(),
                scope,
                generation,
                results: Vec::new(),
                done: query.is_empty(),
            };
            search_progress.set(progress);
            message_window_start.set(0);
            scroll_controller.scroll_to_y(0);
            let next_generation = list_generation.peek().wrapping_add(1);
            list_generation.set(next_generation);
            if !query.is_empty() {
                start_progressive_search(
                    state,
                    search_progress.clone(),
                    query.clone(),
                    generation,
                    search_candidates(state, &all_visible),
                );
            }
        }
        let progress = search_progress.read().clone();
        let visible = if query.is_empty() {
            all_visible
        } else {
            progress.results.clone()
        };
        let stored_window_start = *message_window_start.read();
        let window_start = clamped_message_window_start(visible.len(), stored_window_start);
        if window_start != stored_window_start {
            // The list got shorter (e.g. a message was archived): keep the scroll position.
            message_window_start.set(window_start);
        }

        live_window.set(LiveWindow {
            start: window_start,
            available: visible.len(),
            generation: *list_generation.read(),
        });

        // Watches the scroll position and moves the loaded window before the user can reach
        // either end of it. The list always spans every message, so moving the window never
        // changes the scroll position or the layout; it only decides which rows are real
        // and which are blank placeholders. Runs once and stops when the list is dropped.
        let _scroll_watcher = use_state({
            let live = Rc::downgrade(&live_window);
            let mut window_start_state = message_window_start.clone();
            let list_generation_state = list_generation.clone();
            let mut scroll_controller = scroll_controller;
            move || {
                spawn(async move {
                    loop {
                        futures_timer::Delay::new(Duration::from_millis(16)).await;
                        let Some(live) = live.upgrade() else { break };
                        let live = live.get();
                        // Wait for the render that follows a move before acting again.
                        if *window_start_state.peek() != live.start
                            || *list_generation_state.peek() != live.generation
                        {
                            continue;
                        }
                        let top_row = top_visible_row(scroll_controller.position().y as i32);
                        if let Some(target) =
                            target_window_start(live.available, live.start, top_row)
                        {
                            window_start_state.set(target);
                        }
                    }
                });
            }
        });

        // Search bar
        let search_bar = rect()
            .width(Size::fill())
            .padding((6., 12., 6., 12.))
            .corner_radius(10.)
            .border(freya::prelude::Border::new().width(1.).fill(DARK.divider))
            .background(Color::from_rgb(DARK.bg.0, DARK.bg.1, DARK.bg.2).with_a(128))
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(icon(SEARCH, 14., DARK.muted))
            .child(
                Input::new(search_query.clone())
                    .a11y_id(search_a11y_id)
                    .placeholder("Search messages…")
                    .flat()
                    .compact()
                    .width(Size::fill())
                    .theme_colors(
                        InputColorsThemePartial::new()
                            .focus_background(Color::TRANSPARENT)
                            .focus_border_fill(Color::TRANSPARENT)
                            .border_fill(Color::TRANSPARENT)
                            .color(DARK.text)
                            .placeholder_color(DARK.muted),
                    ),
            );

        let chip_on_press: EventHandler<Event<PressEventData>> = (move |_| {
            let v = !*s_filter.unread_only.read();
            s_filter.unread_only.set(v);
        })
            .into();

        let all_on_press: EventHandler<Event<PressEventData>> = (move |_| {
            s_all.unread_only.set(false);
        })
            .into();

        let title = if starred_only {
            "Starred".to_string()
        } else if acc.is_none() {
            "All inboxes".to_string()
        } else {
            FOLDERS.get(fol).copied().unwrap_or("Inbox").to_string()
        };
        let shown = visible.len();

        let heading = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .padding((6., 4., 2., 4.))
            .child(
                label()
                    .text(title)
                    .color(DARK.text)
                    .font_size(18.)
                    .font_weight(FontWeight::BOLD),
            )
            .child(rect().width(Size::flex(1.)))
            .child(
                label()
                    .text(if !query.is_empty() && !progress.done {
                        format!("{shown} found · searching…")
                    } else {
                        format!("{shown} shown")
                    })
                    .color(DARK.muted)
                    .font_size(12.),
            );

        let toolbar = rect()
            .width(Size::fill())
            .vertical()
            .spacing(8.)
            .padding((6., 12., 10., 12.))
            .child(heading)
            .child(search_bar)
            .child(
                rect()
                    .horizontal()
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(Chip {
                        text: "All".to_string(),
                        active: !unread_only,
                        on_press: all_on_press,
                    })
                    .child(Chip {
                        text: "Unread".to_string(),
                        active: unread_only,
                        on_press: chip_on_press,
                    }),
            );

        let body = if visible.is_empty() {
            if query_text.trim().is_empty() {
                rect()
                    .padding(24.)
                    .child(label().text("No messages").color(DARK.muted))
            } else if !progress.done {
                rect()
                    .padding(24.)
                    .child(label().text("Searching…").color(DARK.muted))
            } else {
                rect()
                    .padding(24.)
                    .child(label().text("No matching messages").color(DARK.muted))
            }
        } else {
            let total = visible.len();
            let window = (window_start, message_window(&visible, window_start).to_vec());
            rect().child(
                VirtualScrollView::new_with_data_controlled(
                    window,
                    move |item, window: &(usize, Vec<usize>)| {
                        let (start, rows) = window;
                        match item.index.checked_sub(*start).and_then(|row| rows.get(row)) {
                            Some(&i) => rect()
                                .key(i)
                                .padding((1., 6., 1., 6.))
                                .child(MessageRow { state, i })
                                .into(),
                            // Not loaded: keeps the layout identical without building a row.
                            None => rect()
                                .key(usize::MAX - item.index)
                                .height(Size::px(MESSAGE_ROW_HEIGHT))
                                .into(),
                        }
                    },
                    scroll_controller,
                )
                    // Only reset the view when the list itself changes (search, filters).
                    // Moving the loaded window keeps the same view and scroll state.
                    .key(*list_generation.read())
                    .width(Size::fill())
                    .height(Size::fill())
                    .item_size(MESSAGE_ROW_HEIGHT)
                    .length(total),
            )
        };

        let fs = *state.focus_state.read();
        let has_focus = fs.keyboard_active && fs.area == crate::keyboard::FocusArea::MessageList;

        rect()
            .width(Size::px(*state.message_list_width.read()))
            .height(Size::fill())
            .background(crate::grain::surface())
            .padding(2.)
            .border(
                freya::prelude::Border::new()
                    .width(if has_focus { 2. } else { 0. })
                    .fill(DARK.accent),
            )
            .child(toolbar)
            .child(divider())
            .child(body)
    }
}

struct MessageRow {
    state: AppState,
    i: usize,
}

impl PartialEq for MessageRow {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

impl Component for MessageRow {
    fn render(&self) -> impl IntoElement {
        let mut hovering = use_state(|| false);
        let state = self.state;
        let i = self.i;
        let (m, account) = {
            let store = state.store.read();
            let Some(m) = store.mails.get(i).cloned() else {
                return rect();
            };
            let account = store.accounts[m.account].clone();
            (m, account)
        };
        let selected = *state.selected.read() == Some(i);
        let unread = state.is_unread(i);
        let unified = state.account.read().is_none();
        let weight = if unread {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let bg = if selected {
            Some(crate::grain::selected())
        } else if hovering() {
            Some(crate::grain::hover())
        } else {
            None
        };

        let mut s_select = state;
        let mut s_archive = state;
        let mut s_trash = state;
        let mut s_read = state;
        let mut s_star = state;

        // Date, or quick actions while hovering. Same fixed height in both states.
        // Use inline buttons without pointer handlers to avoid flicker — IconButton's
        // on_pointer_enter/leave conflicts with the row's hover detection.
        let date = rect().height(Size::px(28.)).center().child(
            label()
                .text(m.date.clone())
                .color(if unread { DARK.text } else { DARK.muted })
                .font_size(12.),
        );
        let trailing = if hovering() {
            rect()
                .horizontal()
                .content(Content::Flex)
                .spacing(4.)
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_star.toggle_star(i);
                        })
                        .child(icon(
                            STAR,
                            16.,
                            if m.starred { DARK.accent } else { DARK.muted },
                        )),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_archive.archive(i);
                        })
                        .child(icon(ARCHIVE, 16., DARK.muted)),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_trash.trash(i);
                        })
                        .child(icon(TRASH, 16., DARK.muted)),
                )
                .child(
                    rect()
                        .width(Size::px(28.))
                        .height(Size::px(28.))
                        .corner_radius(6.)
                        .center()
                        .cursor(freya::winit::window::CursorIcon::Pointer)
                        .on_press(move |e: Event<PressEventData>| {
                            e.stop_propagation();
                            s_read.toggle_read(i);
                        })
                        .child(icon(MAIL, 16., DARK.muted)),
                )
        } else {
            if m.starred {
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(icon(STAR, 14., DARK.accent))
                    .child(date)
            } else {
                date
            }
        };

        // For Sent (folder 2) and Drafts (folder 1), show the recipient instead of the sender
        let (display_name, display_email) = if m.folder == DRAFTS || m.folder == SENT {
            (m.to.clone(), m.to_email.clone())
        } else {
            (m.from.clone(), m.from_email.clone())
        };
        let mut name_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(
                rect().width(Size::flex(1.)).child(
                    label()
                        .text(display_name.clone())
                        .color(DARK.text)
                        .font_size(15.)
                        .font_weight(weight)
                        .max_lines(1)
                        .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                ),
            );
        if unified {
            name_row = name_row.child(
                label()
                    .text(account.name.clone())
                    .color(account.color)
                    .font_size(11.),
            );
        }
        let name_row = name_row.child(trailing);

        let email_row = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .child(
                label()
                    .text(display_email.clone())
                    .color(DARK.muted)
                    .font_size(12.)
                    .max_lines(1)
                    .text_overflow(freya::prelude::TextOverflow::Ellipsis),
            )
            .child(rect().width(Size::flex(1.)));

        let top = rect()
            .width(Size::fill())
            .spacing(1.)
            .child(name_row)
            .child(email_row);

        let row = rect()
            .width(Size::fill())
            .padding((10., 10., 10., 8.))
            .corner_radius(10.)
            .horizontal()
            .content(Content::Flex)
            .spacing(10.);
        let row = match bg {
            Some(c) => row.background(c),
            None => row,
        };
        row.on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_press(move |_e: Event<PressEventData>| {
                if m.folder == DRAFTS {
                    let mut s_compose = state.clone();
                    s_compose.draft_mail.set(Some((m.account, m.folder, m.uid)));
                    s_compose.draft_pin.set(None);
                    s_compose.set_view(crate::state::View::Compose);
                } else {
                    s_select.select(i);
                }
            })
            .child(
                // Fixed-width gutter so rows line up whether read or not.
                rect()
                    .width(Size::px(8.))
                    .margin((14., 0., 0., 0.))
                    .child(if unread {
                        dot(8., DARK.accent)
                    } else {
                        rect().width(Size::px(8.)).height(Size::px(8.))
                    }),
            )
            .child(ProfileAvatar {
                name: display_name,
                email: first_email_address(&display_email).to_string(),
                color: account.color,
                size: 36.,
                state,
            })
            .child(
                rect()
                    .width(Size::flex(1.))
                    .spacing(2.)
                    .child(top)
                    .child(
                        label()
                            .text(m.subject.clone())
                            .color(DARK.text)
                            .font_weight(weight)
                            .max_lines(1)
                            .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                    )
                    .child(
                        label()
                            .text(m.preview.clone())
                            .color(DARK.muted)
                            .font_size(12.)
                            .max_lines(1)
                            .text_overflow(freya::prelude::TextOverflow::Ellipsis),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SearchProgress, clamped_message_window_start, first_email_address, message_window, target_window_start, top_visible_row,
    };

    #[test]
    fn the_loaded_window_holds_fifty_messages() {
        let messages = (0..225).collect::<Vec<_>>();
        assert_eq!(message_window(&messages, 0), (0..50).collect::<Vec<_>>());
        assert_eq!(message_window(&messages, 100), (100..150).collect::<Vec<_>>());
        assert_eq!(message_window(&messages, 175), (175..225).collect::<Vec<_>>());
        assert_eq!(clamped_message_window_start(75, 100), 25);
    }

    #[test]
    fn top_row_comes_from_the_scroll_position() {
        assert_eq!(top_visible_row(0), 0);
        assert_eq!(top_visible_row(-89), 0);
        assert_eq!(top_visible_row(-90), 1);
        assert_eq!(top_visible_row(-3060), 34);
        assert_eq!(top_visible_row(40), 0);
    }

    #[test]
    fn the_window_moves_down_before_the_viewport_reaches_its_end() {
        // Window 0..50: nothing happens until the first visible row reaches 34.
        assert_eq!(target_window_start(225, 0, 0), None);
        assert_eq!(target_window_start(225, 0, 33), None);
        assert_eq!(target_window_start(225, 0, 34), Some(14));
        // Last window: nothing older to load.
        assert_eq!(target_window_start(225, 175, 200), None);
        assert_eq!(target_window_start(225, 175, 224), None);
    }

    #[test]
    fn the_window_moves_up_before_the_viewport_reaches_its_top() {
        assert_eq!(target_window_start(225, 100, 107), None);
        assert_eq!(target_window_start(225, 100, 106), Some(86));
        // First window: nothing newer to load.
        assert_eq!(target_window_start(225, 0, 3), None);
        // Close to the top of the list the window snaps to the start.
        assert_eq!(target_window_start(225, 14, 20), Some(0));
    }

    #[test]
    fn large_jumps_move_the_window_in_one_step() {
        assert_eq!(target_window_start(1000, 0, 700), Some(680));
        assert_eq!(target_window_start(1000, 680, 5), Some(0));
        assert_eq!(target_window_start(1000, 0, 990), Some(950));
    }

    #[test]
    fn a_move_leaves_a_gap_before_either_trigger() {
        for (available, start, top) in [(225, 0, 34), (225, 100, 134), (225, 100, 106)] {
            let target = target_window_start(available, start, top).unwrap();
            assert_eq!(target_window_start(available, target, top), None);
        }
    }

    #[test]
    fn search_batches_append_results_for_the_same_query() {
        let mut progress = SearchProgress {
            query: "invoice".to_string(),
            ..SearchProgress::default()
        };

        progress.append_matches([2, 4]);
        progress.append_matches([7]);

        assert_eq!(progress.query, "invoice");
        assert_eq!(progress.results, vec![2, 4, 7]);
    }

    #[test]
    fn avatar_uses_only_the_first_recipient_email() {
        assert_eq!(
            first_email_address("alice@example.test, bob@example.test"),
            "alice@example.test"
        );
        assert_eq!(
            first_email_address(" alice@example.test "),
            "alice@example.test"
        );
        assert_eq!(first_email_address(""), "");
    }
}

