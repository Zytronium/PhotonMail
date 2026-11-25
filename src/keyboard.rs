use freya::prelude::*;

use crate::state::{AppState, View};

/// Which pane currently has keyboard focus.
#[derive(Clone, Copy, PartialEq)]
pub enum FocusArea {
    MessageList,
    Sidebar,
    Reader,
    Compose,
}
impl Default for FocusArea {
    fn default() -> Self {
        FocusArea::MessageList
    }
}

/// Keyboard focus state: which pane is focused and whether the indicator should be visible.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct FocusState {
    pub area: FocusArea,
    pub keyboard_active: bool,
    pub search_input_focused: bool,
}

/// Result of a focus-area-specific key handler.
#[derive(Clone, Copy, PartialEq)]
enum HandleResult {
    /// Key was consumed; keep keyboard focus active.
    Consumed,
    /// Key was consumed; clear keyboard focus (e.g. Escape).
    Unfocus,
    /// Key was not consumed.
    Ignored,
}

/// Handle a global key-down event and mutate app state accordingly.
///
/// Returns `true` when the key was consumed (the caller should stop propagation).
pub fn handle_key(
    event: &Event<KeyboardEventData>,
    state: &mut AppState,
    focus: &mut FocusState,
) -> bool {
    if matches!(&event.key, Key::Named(NamedKey::F5)) {
        state.sync_all_accounts();
        event.prevent_default();
        return true;
    }

    // If the search input is focused, don't handle keyboard navigation
    if focus.search_input_focused {
        return false;
    }

    let key = &event.key;
    let modifiers = &event.modifiers;

    // --- Compose is open: the editor and inputs own the keyboard. ---
    // Only Escape and Ctrl+Enter are handled globally; everything else is
    // left alone so typing, Enter, Ctrl+C/V/A, Ctrl+B/I/U etc. reach the editor.
    if matches!(*state.view.read(), View::Compose) {
        focus.area = FocusArea::Compose;
        return match handle_compose(key, modifiers, state, &mut focus.area) {
            HandleResult::Consumed => {
                focus.keyboard_active = true;
                true
            }
            HandleResult::Unfocus => {
                focus.keyboard_active = false;
                true
            }
            HandleResult::Ignored => false,
        };
    }

    // --- Ctrl/Cmd shortcuts (work from any focus area) ---
    if modifiers.ctrl() || modifiers.meta() {
        let consumed = handle_ctrl_shortcut(key, state, &mut focus.area);
        if consumed {
            // Only block the default behavior when we actually handled it,
            // so Ctrl+C / Ctrl+V / Ctrl+A keep working in text inputs.
            event.prevent_default();
            focus.keyboard_active = true;
        }
        return consumed;
    }

    // --- Focus-area-specific handling ---
    let result = match focus.area {
        FocusArea::MessageList => handle_message_list(key, modifiers, state, &mut focus.area),
        FocusArea::Sidebar => handle_sidebar(key, modifiers, state, &mut focus.area),
        FocusArea::Reader => handle_reader(key, modifiers, state, &mut focus.area),
        // Compose is handled above, but keep the arm for exhaustiveness.
        FocusArea::Compose => HandleResult::Ignored,
    };
    match result {
        HandleResult::Consumed => {
            focus.keyboard_active = true;
            true
        }
        HandleResult::Unfocus => {
            focus.keyboard_active = false;
            true
        }
        HandleResult::Ignored => false,
    }
}

// ---------------------------------------------------------------------------
// Ctrl / Cmd shortcuts (global)
// ---------------------------------------------------------------------------

fn handle_ctrl_shortcut(key: &Key, state: &mut AppState, _focus: &mut FocusArea) -> bool {
    // Ctrl+1..6 switch to folder index
    if let Key::Character(ch) = key {
        if let Ok(digit) = ch.parse::<usize>() {
            if (1..=6).contains(&digit) {
                // FOLDERS has 6 entries: Inbox, Drafts, Sent, Archive, Spam, Trash
                let account = *state.account.read();
                // Default to first account if in unified view
                let a = account.unwrap_or(0);
                state.go(Some(a), digit - 1);
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Message List focus
// ---------------------------------------------------------------------------

fn handle_message_list(
    key: &Key,
    modifiers: &Modifiers,
    state: &mut AppState,
    focus: &mut FocusArea,
) -> HandleResult {
    let visible = state.filtered_visible();
    if visible.is_empty() {
        return HandleResult::Ignored;
    }

    let current = *state.selected.read();
    let selected_idx = visible.iter().position(|&v| Some(v) == current);
    let mut idx = selected_idx.unwrap_or(0);

    // Check for character keys first (Gmail-style shortcuts)
    if let Key::Character(ch) = key {
        if !modifiers.shift() {
            match ch.as_str() {
                "j" => {
                    if selected_idx.is_some() && idx < visible.len() - 1 {
                        idx += 1;
                    }
                    state.select(visible[idx]);
                    return HandleResult::Consumed;
                }
                "k" => {
                    if selected_idx.is_some() && idx > 0 {
                        idx -= 1;
                    }
                    state.select(visible[idx]);
                    return HandleResult::Consumed;
                }
                "o" => {
                    state.select(visible[idx]);
                    *focus = FocusArea::Reader;
                    return HandleResult::Consumed;
                }
                "e" => {
                    if let Some(open) = state.open() {
                        state.archive(open);
                    }
                    return HandleResult::Consumed;
                }
                "d" => {
                    if let Some(open) = state.open() {
                        state.trash(open);
                    }
                    return HandleResult::Consumed;
                }
                "u" => {
                    if let Some(open) = state.open() {
                        state.toggle_read(open);
                    }
                    return HandleResult::Consumed;
                }
                "r" => {
                    // TODO: open compose window for reply
                    return HandleResult::Consumed;
                }
                "f" => {
                    // TODO: open compose window for forward
                    return HandleResult::Consumed;
                }
                "/" => {
                    // TODO: focus search input
                    return HandleResult::Consumed;
                }
                _ => {}
            }
        }
    }

    // Named keys
    match key {
        Key::Named(NamedKey::ArrowUp) => {
            if selected_idx.is_some() && idx > 0 {
                idx -= 1;
            }
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowDown) => {
            if selected_idx.is_some() && idx < visible.len() - 1 {
                idx += 1;
            }
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::PageUp) => {
            idx = idx.saturating_sub(10).min(visible.len() - 1);
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::PageDown) => {
            idx = (idx + 10).min(visible.len() - 1);
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Home) => {
            state.select(visible[0]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::End) => {
            state.select(visible[visible.len() - 1]);
            HandleResult::Consumed
        }
        // Open selected message in reader
        Key::Named(NamedKey::Enter) => {
            state.select(visible[idx]);
            *focus = FocusArea::Reader;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowLeft) => {
            *focus = FocusArea::Sidebar;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowRight) => {
            if state.open().is_some() {
                *focus = FocusArea::Reader;
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Tab) if modifiers.shift() => {
            *focus = FocusArea::Sidebar;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Tab) => {
            if state.open().is_some() {
                *focus = FocusArea::Reader;
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Escape) => HandleResult::Unfocus,
        _ => HandleResult::Ignored,
    }
}

// ---------------------------------------------------------------------------
// Sidebar focus
// ---------------------------------------------------------------------------

fn handle_sidebar(
    key: &Key,
    _modifiers: &Modifiers,
    _state: &mut AppState,
    focus: &mut FocusArea,
) -> HandleResult {
    match key {
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::Tab) => {
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Escape) => HandleResult::Unfocus,
        _ => HandleResult::Ignored,
    }
}

// ---------------------------------------------------------------------------
// Reader focus
// ---------------------------------------------------------------------------

fn handle_reader(
    key: &Key,
    modifiers: &Modifiers,
    state: &mut AppState,
    focus: &mut FocusArea,
) -> HandleResult {
    if let Key::Character(ch) = key {
        if !modifiers.shift() {
            match ch.as_str() {
                "j" => {
                    let visible = state.filtered_visible();
                    let current = *state.selected.read();
                    if let Some(idx) = visible.iter().position(|&v| Some(v) == current) {
                        if idx + 1 < visible.len() {
                            state.select(visible[idx + 1]);
                        }
                    } else if let Some(first) = visible.first() {
                        state.select(*first);
                    }
                    return HandleResult::Consumed;
                }
                "k" => {
                    let visible = state.filtered_visible();
                    let current = *state.selected.read();
                    if let Some(idx) = visible.iter().position(|&v| Some(v) == current) {
                        if idx > 0 {
                            state.select(visible[idx - 1]);
                        }
                    } else if let Some(first) = visible.first() {
                        state.select(*first);
                    }
                    return HandleResult::Consumed;
                }
                "e" => {
                    if let Some(open) = state.open() {
                        state.archive(open);
                    }
                    return HandleResult::Consumed;
                }
                "d" => {
                    if let Some(open) = state.open() {
                        state.trash(open);
                    }
                    return HandleResult::Consumed;
                }
                "u" => {
                    if let Some(open) = state.open() {
                        state.toggle_read(open);
                    }
                    return HandleResult::Consumed;
                }
                "r" => {
                    // TODO: open compose window for reply
                    return HandleResult::Consumed;
                }
                "f" => {
                    // TODO: open compose window for forward
                    return HandleResult::Consumed;
                }
                _ => {}
            }
        }
    }

    match key {
        Key::Named(NamedKey::ArrowLeft) => {
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Escape) => HandleResult::Unfocus,
        Key::Named(NamedKey::Tab) if modifiers.shift() => {
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowDown) => {
            let visible = state.filtered_visible();
            let current = *state.selected.read();
            if let Some(idx) = visible.iter().position(|&v| Some(v) == current) {
                if idx + 1 < visible.len() {
                    state.select(visible[idx + 1]);
                }
            } else if let Some(first) = visible.first() {
                state.select(*first);
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowUp) => {
            let visible = state.filtered_visible();
            let current = *state.selected.read();
            if let Some(idx) = visible.iter().position(|&v| Some(v) == current) {
                if idx > 0 {
                    state.select(visible[idx - 1]);
                }
            } else if let Some(first) = visible.first() {
                state.select(*first);
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            HandleResult::Consumed
        }
        _ => HandleResult::Ignored,
    }
}

// ---------------------------------------------------------------------------
// Compose (only Escape and Ctrl+Enter are global; the editor handles the rest)
// ---------------------------------------------------------------------------

fn handle_compose(
    key: &Key,
    modifiers: &Modifiers,
    state: &mut AppState,
    focus: &mut FocusArea,
) -> HandleResult {
    match key {
        // Ctrl/Cmd+Enter sends the message
        Key::Named(NamedKey::Enter) if modifiers.ctrl() || modifiers.meta() => {
            // TODO: trigger send
            state.draft_mail.set(None);
            state.draft_pin.set(None);
            state.view.set(View::Mail);
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        // Ctrl/Cmd+S saves the draft
        Key::Character(ch) if (modifiers.ctrl() || modifiers.meta()) && ch.as_str() == "s" => {
            // TODO: trigger save draft from compose pane
            // The compose pane handles this via its own button; this shortcut
            // is a placeholder for future implementation.
            HandleResult::Ignored
        }
        // Escape closes compose and returns to the mail view
        Key::Named(NamedKey::Escape) => {
            state.draft_mail.set(None);
            state.draft_pin.set(None);
            state.view.set(View::Mail);
            *focus = FocusArea::MessageList;
            HandleResult::Unfocus
        }
        // Everything else (typing, Enter, Tab, Ctrl+C/V/A/B/I/U...) goes to the editor.
        _ => HandleResult::Ignored,
    }
}
