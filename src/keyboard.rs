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
    let consumed = match focus.area {
        FocusArea::MessageList => handle_message_list(key, modifiers, state, &mut focus.area),
        FocusArea::Sidebar => handle_sidebar(key, modifiers, state, &mut focus.area),
        FocusArea::Reader => handle_reader(key, modifiers, state, &mut focus.area),
    };
    if consumed {
        focus.keyboard_active = true;
    }
    consumed
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
) -> bool {
    let visible = state.visible();
    if visible.is_empty() {
        return false;
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
                    return true;
                }
                "k" => {
                    if idx > 0 { idx -= 1; }
                    state.select(visible[idx]);
                    return true;
                }
                "o" => {
                    state.select(visible[idx]);
                    *focus = FocusArea::Reader;
                    return true;
                }
                "e" => {
                    if let Some(open) = state.open() {
                        state.archive(open);
                    }
                    return true;
                }
                "d" => {
                    if let Some(open) = state.open() {
                        state.trash(open);
                    }
                    return true;
                }
                "u" => {
                    if let Some(open) = state.open() {
                        state.toggle_read(open);
                    }
                    return true;
                }
                "r" => {
                    // TODO: open compose window for reply
                    return true;
                }
                "f" => {
                    // TODO: open compose window for forward
                    return true;
                }
                "/" => {
                    // TODO: focus search input
                    return true;
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
            true
        }
        Key::Named(NamedKey::ArrowDown) => {
            if idx < visible.len() - 1 { idx += 1; }
            state.select(visible[idx]);
            true
        }
        // Page Up / Page Down
        Key::Named(NamedKey::PageUp) => {
            idx = idx.saturating_sub(10).min(visible.len() - 1);
            state.select(visible[idx]);
            true
        }
        Key::Named(NamedKey::PageDown) => {
            idx = (idx + 10).min(visible.len() - 1);
            state.select(visible[idx]);
            true
        }
        // Home / End
        Key::Named(NamedKey::Home) => {
            state.select(visible[0]);
            true
        }
        Key::Named(NamedKey::End) => {
            state.select(visible[visible.len() - 1]);
            true
        }
        // Open selected message in reader
        Key::Named(NamedKey::Enter) => {
            state.select(visible[idx]);
            *focus = FocusArea::Reader;
            true
        }
        // Move focus to sidebar
        Key::Named(NamedKey::ArrowLeft) => {
            *focus = FocusArea::Sidebar;
            true
        }
        // Move focus to reader
        Key::Named(NamedKey::ArrowRight) => {
            if state.open().is_some() {
                *focus = FocusArea::Reader;
            }
            true
        }
        // Tab / Shift+Tab for focus switching
        Key::Named(NamedKey::Tab) if modifiers.shift() => {
            *focus = FocusArea::Sidebar;
            true
        }
        Key::Named(NamedKey::Tab) => {
            if state.open().is_some() {
                *focus = FocusArea::Reader;
            }
            true
        }
        // Delete key
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            true
        }
        // Escape — move focus back to message list
        Key::Named(NamedKey::Escape) => {
            *focus = FocusArea::MessageList;
            true
        }
        _ => false,
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
) -> bool {
    match key {
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::Tab) => {
            // Move focus to message list
            *focus = FocusArea::MessageList;
            true
        }
        Key::Named(NamedKey::Escape) => {
            *focus = FocusArea::MessageList;
            true
        }
        _ => false,
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
) -> bool {
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
                    return true;
                }
                "k" => {
                    let visible = state.visible();
                    let current = *state.selected.read();
                    if let Some(idx) = visible.iter().position(|&v| v == current) {
                        if idx > 0 {
                            state.select(visible[idx - 1]);
                        }
                    }
                    return true;
                }
                "e" => {
                    if let Some(open) = state.open() {
                        state.archive(open);
                    }
                    return true;
                }
                "d" => {
                    if let Some(open) = state.open() {
                        state.trash(open);
                    }
                    return true;
                }
                "u" => {
                    if let Some(open) = state.open() {
                        state.toggle_read(open);
                    }
                    return true;
                }
                "r" => {
                    // TODO: open compose window for reply
                    return true;
                }
                "f" => {
                    // TODO: open compose window for forward
                    return true;
                }
                _ => {}
            }
        }
    }

    // Named keys
    match key {
        // Move focus back to message list
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::Escape) => {
            *focus = FocusArea::MessageList;
            true
        }
        Key::Named(NamedKey::Tab) if modifiers.shift() => {
            *focus = FocusArea::MessageList;
            true
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
            true
        }
        Key::Named(NamedKey::ArrowUp) => {
            let visible = state.visible();
            let current = *state.selected.read();
            if let Some(idx) = visible.iter().position(|&v| v == current) {
                if idx > 0 {
                    state.select(visible[idx - 1]);
                }
            }
            true
        }
        // Delete key
        Key::Named(NamedKey::Delete) => {
            if let Some(open) = state.open() {
                state.trash(open);
            }
            true
        }
        _ => false,
    }
}
