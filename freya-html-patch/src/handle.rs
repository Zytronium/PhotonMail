use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

use freya_core::prelude::*;

use crate::{
    content::HtmlContent,
    state::BlitzState,
};

/// A document for an [HtmlViewer](crate::HtmlViewer), either remote or inline.
#[derive(Clone, PartialEq)]
pub enum HtmlSource {
    Url(String),
    Html(HtmlContent),
}

impl HtmlSource {
    /// A remote document fetched from `url`.
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Inline HTML + CSS, optionally configured with an [HtmlContent].
    pub fn html(html: impl Into<HtmlContent>) -> Self {
        Self::Html(html.into())
    }
}

pub struct ElementHit {
    pub tag: String,
    pub attributes: HashMap<String, String>,
}

/// Position of a tracked element (see [`HtmlViewer::track`](crate::HtmlViewer::track)),
/// in the viewer's own coordinate space: logical pixels from its top-left corner, with the
/// document's current scroll already applied. Lets a Freya widget be drawn on top of the
/// document exactly where an element of the HTML is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AnchorRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

struct HtmlHistory {
    entries: Vec<HtmlSource>,
    index: usize,
}

/// Controls an [HtmlViewer](crate::HtmlViewer), its navigation and its browsing history.
///
/// ```rust, no_run
/// # use freya::prelude::*;
/// # use freya_html::prelude::*;
/// # fn app() -> impl IntoElement {
/// let mut handle = use_html(|| HtmlSource::url("https://example.com"));
///
/// rect()
///     .child(Button::new().child("Back").on_press(move |_| handle.back()))
///     .child(HtmlViewer::new(handle))
/// # }
/// ```
#[derive(PartialEq, Clone, Copy)]
pub struct HtmlHandle {
    history: State<HtmlHistory>,
    view: State<Option<Rc<RefCell<BlitzState>>>>,
    anchor: State<Option<AnchorRect>>,
    intercept_navigation: State<bool>,
    pending_navigation: State<Option<String>>,
}

impl HtmlHandle {
    /// A handle starting at `initial`.
    pub fn create(initial: HtmlSource) -> Self {
        let mut handle = Self::create_empty();
        handle.push(initial);
        handle
    }

    /// A handle with an empty history.
    pub fn create_empty() -> Self {
        Self {
            history: State::create(HtmlHistory {
                entries: Vec::new(),
                index: 0,
            }),
            view: State::create(None),
            anchor: State::create(None),
            intercept_navigation: State::create(false),
            pending_navigation: State::create(None),
        }
    }

    /// A handle with an empty history, backed by global states.
    pub fn create_global_empty() -> Self {
        Self {
            history: State::create_global(HtmlHistory {
                entries: Vec::new(),
                index: 0,
            }),
            view: State::create_global(None),
            anchor: State::create_global(None),
            intercept_navigation: State::create_global(false),
            pending_navigation: State::create_global(None),
        }
    }

    /// Where the element tracked with [`HtmlViewer::track`](crate::HtmlViewer::track) currently
    /// is, or `None` when it doesn't exist or is scrolled out of view. Reading this subscribes
    /// the calling component, so it re-renders when the element moves (scroll, resize, reload).
    pub fn anchor_rect(&self) -> Option<AnchorRect> {
        *self.anchor.read()
    }

    /// Used by the viewer to publish the tracked element's position.
    pub(crate) fn set_anchor_rect(&mut self, rect: Option<AnchorRect>) {
        if *self.anchor.peek() != rect {
            self.anchor.set(rect);
        }
    }

    /// Attaches the view's document state and loads the current source into it.
    pub(crate) fn attach(&mut self, view: Rc<RefCell<BlitzState>>) {
        *self.view.write() = Some(view);
        self.reload();
    }

    /// Load the URL, discarding any forward history.
    pub fn navigate(&mut self, url: impl Into<String>) {
        let url = url.into();
        if *self.intercept_navigation.peek() {
            self.pending_navigation.set(Some(url));
        } else {
            self.push(HtmlSource::Url(url));
        }
    }

    /// Intercept navigations initiated by links instead of loading their targets.
    pub fn set_navigation_interception(&mut self, intercept: bool) {
        if *self.intercept_navigation.peek() != intercept {
            self.intercept_navigation.set(intercept);
        }
    }

    /// Consume a link target intercepted by this handle without changing the displayed document.
    pub fn take_intercepted_navigation(&mut self) -> Option<String> {
        let url = self.pending_navigation.read().clone();
        if url.is_some() {
            self.pending_navigation.set(None);
        }
        url
    }

    /// Load inline HTML + CSS and its configuration, discarding any forward history.
    pub fn load_html(&mut self, html: impl Into<HtmlContent>) {
        self.push(HtmlSource::Html(html.into()));
    }

    /// Replaces the current document, configuration, and history entry, discarding forward history.
    pub fn replace_html(&mut self, html: impl Into<HtmlContent>) {
        let source = HtmlSource::Html(html.into());
        self.load(&source);
        let mut history = self.history.write();
        let index = history.index;
        history.entries.truncate(index);
        history.entries.push(source);
    }

    /// Go back one entry in the history.
    pub fn back(&mut self) {
        if self.can_go_back() {
            self.history.write().index -= 1;
            self.reload();
        }
    }

    /// Go forward one entry in the history.
    pub fn forward(&mut self) {
        if self.can_go_forward() {
            self.history.write().index += 1;
            self.reload();
        }
    }

    /// Load the current history entry again.
    pub fn reload(&self) {
        if let Some(source) = self.current_source() {
            self.load(&source);
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history.read().index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        let history = self.history.read();
        history.index + 1 < history.entries.len()
    }

    pub fn elements_at(&self, x: f32, y: f32) -> Vec<ElementHit> {
        let Some(view) = self.view.peek().clone() else {
            return Vec::new();
        };
        view.borrow_mut().elements_at(x, y)
    }

    /// Dispatch a mouse move event to the HTML document so that Blitz
    /// updates hover state and cursor information.
    pub fn mouse_move(&self, x: f32, y: f32) {
        if let Some(view) = self.view.peek().clone() {
            view.borrow_mut().mouse_move(x, y);
        }
    }

    /// Get the current cursor icon from the HTML document based on the
    /// last mouse position and CSS cursor rules.
    pub fn get_cursor(&self) -> Option<cursor_icon::CursorIcon> {
        let Some(view) = self.view.peek().clone() else {
            return None;
        };
        view.borrow().get_cursor()
    }

    /// The URL currently being displayed, `None` for inline documents.
    pub fn current_url(&self) -> Option<String> {
        let history = self.history.read();
        match history.entries.get(history.index) {
            Some(HtmlSource::Url(url)) => Some(url.clone()),
            _ => None,
        }
    }

    fn push(&mut self, source: HtmlSource) {
        self.load(&source);
        let mut history = self.history.write();
        let index = if history.entries.is_empty() {
            0
        } else {
            history.index + 1
        };
        history.entries.truncate(index);
        history.entries.push(source);
        history.index = index;
    }

    fn current_source(&self) -> Option<HtmlSource> {
        let history = self.history.peek();
        history.entries.get(history.index).cloned()
    }

    /// Loads `source` into the attached view.
    fn load(&self, source: &HtmlSource) {
        let Some(view) = self.view.peek().clone() else {
            return;
        };
        match source {
            HtmlSource::Url(url) => {
                spawn(load_url(view, url.clone()));
            }
            HtmlSource::Html(content) => view.borrow_mut().load(
                &content.html,
                content.base_url.as_ref().map(ToString::to_string),
            ),
        }
    }
}

async fn load_url(view: Rc<RefCell<BlitzState>>, url: String) {
    let fetched = match url::Url::parse(&url) {
        Ok(parsed) => {
            let Some(permit) = crate::http::try_acquire_fetch() else {
                tracing::warn!("Remote HTML fetch limit reached for {url}");
                return;
            };
            let fetched = crate::http::fetch(parsed, permit).await;
            fetched
        }
        Err(err) => Err(err.to_string()),
    };
    match fetched {
        Ok(bytes) => view
            .borrow_mut()
            .load(&String::from_utf8_lossy(&bytes), Some(url)),
        Err(err) => tracing::error!("Failed to load {url}: {err}"),
    }
}

/// Creates an [HtmlHandle] starting at the source returned by `init`.
pub fn use_html(init: impl FnOnce() -> HtmlSource) -> HtmlHandle {
    use_hook(move || HtmlHandle::create(init()))
}
