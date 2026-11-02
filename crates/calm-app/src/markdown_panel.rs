//! Markdown panels: the text rendered by `calm_core::markdown` in a `MarkdownView`, placed
//! through the same clip and zoom views as plugin pages.

use calm_core::geometry::Size;
use calm_core::markdown::page_html;
use calm_core::preview::PanelMode;
use calm_core::scene::{Panel, PanelKind};
use calm_core::settings::Settings;
use calm_core::viewport::NativePlacement;
use calm_native::{MarkdownView, NativeChild};
use gpui::Window;

/// The page a markdown panel shows now, from its text, its own style and the globals.
/// `None` for another kind.
pub fn page_for(panel: &Panel, settings: &Settings) -> Option<String> {
    let PanelKind::Markdown { text } = &panel.kind else {
        return None;
    };
    let style = settings.markdown.resolve(panel.overrides.markdown.as_ref());
    Some(page_html(text, &style))
}

pub struct MarkdownPanel {
    // Declared before `host` so it drops first: the web view sits inside the host's views.
    view: MarkdownView,
    host: NativeChild,
    logical_size: Option<Size>,
    visible: bool,
    /// The page last loaded, so an unrelated settings change does not reload it.
    html: String,
}

impl MarkdownPanel {
    pub fn open(html: String, window: &mut Window) -> Result<MarkdownPanel, String> {
        let mut host = NativeChild::new(window).map_err(|err| err.to_string())?;
        host.set_visible(false);
        let mut view = MarkdownView::new(&host).map_err(|err| err.to_string())?;
        view.load_html(&html);
        Ok(MarkdownPanel {
            view,
            host,
            logical_size: None,
            visible: false,
            html,
        })
    }

    /// Loads `html` in the same web view if it differs from the page shown. True if it
    /// reloaded.
    pub fn show(&mut self, html: String) -> bool {
        if html == self.html {
            return false;
        }
        self.view.load_html(&html);
        self.html = html;
        true
    }

    /// As `PluginPanel::apply_mode`: Live places the page, Preview and Hidden hide it.
    pub fn apply_mode(
        &mut self,
        mode: PanelMode,
        placement: Option<NativePlacement>,
        window: &Window,
    ) {
        let Some(placement) = placement.filter(|_| mode == PanelMode::Live) else {
            if self.visible {
                self.visible = false;
                self.host.set_visible(false);
            }
            return;
        };
        if self.logical_size != Some(placement.logical_size) {
            self.view.set_logical_size(placement.logical_size);
            self.logical_size = Some(placement.logical_size);
        }
        let scale = f64::from(window.scale_factor());
        let viewport_height = f64::from(f32::from(window.viewport_size().height));
        self.host.apply(&placement, viewport_height, scale);
        self.visible = true;
    }

    pub fn bring_to_front(&mut self) {
        self.host.bring_to_front();
    }

    pub fn focus(&self) -> bool {
        self.view.focus()
    }

    pub fn stack_index(&self) -> Option<usize> {
        self.host.stack_index()
    }

    pub fn is_loading(&self) -> bool {
        self.view.is_loading()
    }

    pub fn draws_background(&self) -> Option<bool> {
        self.view.draws_background()
    }

    pub fn probe(&mut self, script: &str) {
        self.view.probe(script);
    }

    pub fn probe_result(&self) -> Option<String> {
        self.view.probe_result()
    }

    pub fn html(&self) -> &str {
        &self.html
    }
}
