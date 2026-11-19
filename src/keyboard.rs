use freya::prelude::*;

use crate::state::AppState;

/// Which pane currently has keyboard focus.
#[derive(Clone, Copy, PartialEq)]
pub enum FocusArea {
    MessageList,
    Sidebar,
    Reader,
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
pub fn handle_key(event: &Event<KeyboardEventData>, state: &mut AppState, focus: &mut FocusState) -> bool {
    let key = &event.key;
    let modifiers = &event.modifiers;

    // --- Ctrl/Cmd shortcuts (work from any focus area) ---
    if modifiers.ctrl() || modifiers.meta() {
        let consumed = handle_ctrl_shortcut(key, state, &mut focus.area);
        if consumed {
            focus.keyboard_active = true;
        }
        return consumed;
    }

    // --- Focus-area-specific handling ---
    let result = match focus.area {
        FocusArea::MessageList => handle_message_list(key, modifiers, state, &mut focus.area),
        FocusArea::Sidebar => handle_sidebar(key, modifiers, state, &mut focus.area),
        FocusArea::Reader => handle_reader(key, modifiers, state, &mut focus.area),
    };
    match result {
        HandleResult::Consumed => { focus.keyboard_active = true; return true; }
        HandleResult::Unfocus => { focus.keyboard_active = false; return true; }
        HandleResult::Ignored => false,
    }
}

// ---------------------------------------------------------------------------
// Ctrl / Cmd shortcuts (global)
// ---------------------------------------------------------------------------

fn handle_ctrl_shortcut(key: &Key, state: &mut AppState, _focus: &mut FocusArea) -> bool {
    // Ctrl+1..9 switch to folder index
    if let Key::Character(ch) = key {
        if let Some(digit) = ch.parse::<usize>().ok() {
            if digit >= 1 && digit <= 6 {
                // FOLDERS has 6 entries: Inbox, Drafts, Sent, Archive, Spam, Trash
                let account = *state.account.read();
                let acc = account.or_else(|| {
                    // Default to first account if in unified view
                    Some(0)
                });
                if let Some(a) = acc {
                    state.go(Some(a), digit - 1);
                    return true;
                }
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
    let visible = state.visible();
    if visible.is_empty() {
        return HandleResult::Ignored;
    }

    let current = *state.selected.read();
    let idx = visible.iter().position(|&v| v == current);
    let mut idx = idx.unwrap_or(0);

    // Check for character keys first (Gmail-style shortcuts)
    if let Key::Character(ch) = key {
        if !modifiers.shift() {
            match ch.as_str() {
                "j" => {
                    if idx < visible.len() - 1 { idx += 1; }
                    state.select(visible[idx]);
                    return HandleResult::Consumed;
                }
                "k" => {
                    if idx > 0 { idx -= 1; }
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
        // Arrow keys
        Key::Named(NamedKey::ArrowUp) => {
            if idx > 0 { idx -= 1; }
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowDown) => {
            if idx < visible.len() - 1 { idx += 1; }
            state.select(visible[idx]);
            HandleResult::Consumed
        }
        // Page Up / Page Down
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
        // Home / End
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
        // Move focus to sidebar
        Key::Named(NamedKey::ArrowLeft) => {
            *focus = FocusArea::Sidebar;
            HandleResult::Consumed
        }
        // Move focus to reader
        Key::Named(NamedKey::ArrowRight) => {
            if state.open().is_some() {
                *focus = FocusArea::Reader;
            }
            HandleResult::Consumed
        }
        // Tab / Shift+Tab for focus switching
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
        // Delete key
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            HandleResult::Consumed
        }
        // Escape — unfocus
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
            // Move focus to message list
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
    // Character shortcuts
    if let Key::Character(ch) = key {
        if !modifiers.shift() {
            match ch.as_str() {
                "j" => {
                    let visible = state.visible();
                    let current = *state.selected.read();
                    if let Some(idx) = visible.iter().position(|&v| v == current) {
                        if idx + 1 < visible.len() {
                            state.select(visible[idx + 1]);
                        }
                    }
                    return HandleResult::Consumed;
                }
                "k" => {
                    let visible = state.visible();
                    let current = *state.selected.read();
                    if let Some(idx) = visible.iter().position(|&v| v == current) {
                        if idx > 0 {
                            state.select(visible[idx - 1]);
                        }
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

    // Named keys
    match key {
        // Move focus back to message list
        Key::Named(NamedKey::ArrowLeft) => {
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        // Escape — unfocus
        Key::Named(NamedKey::Escape) => HandleResult::Unfocus,
        Key::Named(NamedKey::Tab) if modifiers.shift() => {
            *focus = FocusArea::MessageList;
            HandleResult::Consumed
        }
        // Navigate to next/prev message while staying in reader
        Key::Named(NamedKey::ArrowDown) => {
            let visible = state.visible();
            let current = *state.selected.read();
            if let Some(idx) = visible.iter().position(|&v| v == current) {
                if idx + 1 < visible.len() {
                    state.select(visible[idx + 1]);
                }
            }
            HandleResult::Consumed
        }
        Key::Named(NamedKey::ArrowUp) => {
            let visible = state.visible();
            let current = *state.selected.read();
            if let Some(idx) = visible.iter().position(|&v| v == current) {
                if idx > 0 {
                    state.select(visible[idx - 1]);
                }
            }
            HandleResult::Consumed
        }
        // Delete key
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            HandleResult::Consumed
        }
        _ => HandleResult::Ignored,
    }
}
