//! Browser panels: a native `BrowserView` (address bar plus WKWebView) in a `NativeChild`,
//! placed and moded by the canvas like a terminal.

use calm_core::geometry::Size;
use calm_core::preview::PanelMode;
use calm_core::scene::PanelId;
use calm_core::viewport::NativePlacement;
use calm_native::{BrowserEvent, BrowserView, NativeChild};
use gpui::Window;

/// Page events tagged with their panel, sent from AppKit callbacks to the canvas.
pub type BrowserEvents = async_channel::Sender<(PanelId, BrowserEvent)>;

pub struct BrowserPanel {
    // Dropped before `host`, so the page leaves the zoom view before the views go.
    view: BrowserView,
    host: NativeChild,
    mode: PanelMode,
    logical_size: Option<Size>,
    visible: bool,
    /// The address bar asked for the keyboard before the views were first shown.
    address_focus_pending: bool,
}

impl BrowserPanel {
    /// Creates the views and starts loading `address` (a URL, bare host or search).
    pub fn open(
        id: PanelId,
        address: &str,
        events: BrowserEvents,
        window: &Window,
    ) -> Result<BrowserPanel, String> {
        let host = NativeChild::new(window).map_err(|err| err.to_string())?;
        let mut view = BrowserView::new(&host).map_err(|err| err.to_string())?;
        // Never blocks: the channel is unbounded. A closed channel means the canvas is gone.
        view.on_event(Box::new(move |event| {
            events.try_send((id, event)).ok();
        }));
        if !view.open(address) {
            eprintln!("calm: browser could not open {address:?}");
        }
        Ok(BrowserPanel {
            view,
            host,
            mode: PanelMode::Hidden,
            logical_size: None,
            visible: false,
            address_focus_pending: false,
        })
    }

    pub fn mode(&self) -> PanelMode {
        self.mode
    }

    /// Live shows the views at `placement`; Preview and Hidden hide them, and the canvas
    /// paints a flat rect with the title instead. The page keeps running either way.
    pub fn apply_mode(
        &mut self,
        mode: PanelMode,
        placement: Option<NativePlacement>,
        window: &Window,
    ) {
        self.mode = mode;
        let placement = placement.filter(|_| mode == PanelMode::Live);
        let Some(placement) = placement else {
            self.set_visible(false);
            return;
        };
        let size = placement.logical_size;
        // Zoom only scales the zoom view; the page relayouts on a panel resize alone.
        if self.logical_size != Some(size) {
            self.view.set_logical_size(size);
            self.logical_size = Some(size);
        }
        let scale = f64::from(window.scale_factor());
        let viewport_height = f64::from(f32::from(window.viewport_size().height));
        self.host.apply(&placement, viewport_height, scale);
        self.set_visible(true);
        if std::mem::take(&mut self.address_focus_pending) {
            self.view.focus_address();
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.host.set_visible(visible);
        }
    }

    pub fn bring_to_front(&mut self) {
        self.host.bring_to_front();
    }

    pub fn focus_page(&self) {
        self.view.focus_page();
    }

    /// Hidden views cannot take the keyboard, so a new panel focuses on its first showing.
    pub fn focus_address(&mut self) {
        if self.visible {
            self.view.focus_address();
        } else {
            self.address_focus_pending = true;
        }
    }

    pub fn is_loading(&self) -> bool {
        self.view.is_loading()
    }

    pub fn url(&self) -> Option<String> {
        self.view.url()
    }
}
