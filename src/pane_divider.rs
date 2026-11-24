use freya::prelude::*;
use freya::winit::window::CursorIcon;

use crate::state::AppState;
use crate::theme::DARK;

/// Which pane's width this divider controls.
#[derive(Clone, Copy, PartialEq)]
pub enum DividerTarget {
    Sidebar,
    MessageList,
}

#[derive(PartialEq)]
pub struct PaneDivider {
    pub state: AppState,
    pub target: DividerTarget,
}

const DIVIDER_WIDTH: f32 = 4.;
const MIN_SIDEBAR_WIDTH: f32 = 180.;
const MAX_SIDEBAR_WIDTH: f32 = 500.;
const MIN_MESSAGE_LIST_WIDTH: f32 = 200.;
const MAX_MESSAGE_LIST_WIDTH: f32 = 700.;
const MIN_READER_WIDTH: f32 = 450.;

impl Component for PaneDivider {
    fn render(&self) -> impl IntoElement {
        let state = self.state;
        let target = self.target;
        let mut is_dragging = use_state(|| false);
        let mut start_x = use_state(|| 0.);
        let mut start_width = use_state(|| 0.);

        // Read the current width for this divider
        let current_width = match target {
            DividerTarget::Sidebar => *state.sidebar_width.read(),
            DividerTarget::MessageList => *state.message_list_width.read(),
        };

        // Min/max constraints
        let (min_w, max_w) = match target {
            DividerTarget::Sidebar => (MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH),
            DividerTarget::MessageList => (MIN_MESSAGE_LIST_WIDTH, MAX_MESSAGE_LIST_WIDTH),
        };

        let mut s_drag = state;
        let t = target;

        let mut hovering = use_state(|| false);
        rect()
            .width(Size::px(DIVIDER_WIDTH))
            .height(Size::fill())
            .background(if is_dragging() {
                Fill::from(DARK.accent)
            } else if hovering() {
                Fill::from((50, 50, 90))
            } else {
                Fill::from(DARK.divider)
            })
            .cursor(CursorIcon::ColResize)
            .on_pointer_enter(move |_| hovering.set(true))
            .on_pointer_leave(move |_| hovering.set(false))
            .on_pointer_down(move |e: Event<PointerEventData>| {
                is_dragging.set(true);
                start_x.set(e.global_location().x);
                start_width.set(current_width);
                e.stop_propagation();
            })
            .on_global_pointer_move(move |e: Event<PointerEventData>| {
                if !is_dragging() {
                    return;
                }
                let dx = (e.global_location().x - start_x()) as f32;
                let mut new_width = start_width() + dx;

                // For the MessageList divider, also enforce that the reader pane
                // doesn't shrink below its minimum width
                if t == DividerTarget::MessageList {
                    let sidebar_w = *s_drag.sidebar_width.read();
                    // Total reserved: sidebar + 2 dividers + min reader width
                    let reserved = sidebar_w + DIVIDER_WIDTH * 2. + MIN_READER_WIDTH;
                    // Window width is 1200 (from main.rs), so max message list = window - reserved
                    let dynamic_max = 1200. - reserved;
                    new_width = new_width.clamp(min_w, (max_w).min(dynamic_max));
                } else {
                    new_width = new_width.clamp(min_w, max_w);
                }

                match t {
                    DividerTarget::Sidebar => s_drag.sidebar_width.set(new_width),
                    DividerTarget::MessageList => s_drag.message_list_width.set(new_width),
                }
            })
            .on_global_pointer_up(move |_| {
                is_dragging.set(false);
            })
    }
}
