use crate::geometry::Rect;

/// Below this zoom a panel shows a snapshot instead of its live native view.
pub const PREVIEW_ZOOM_THRESHOLD: f64 = 0.5;
pub const PREVIEW_REFRESH_SECS: u64 = 2;

/// How a panel is drawn. The process keeps running in every mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelMode {
    Live,
    Preview,
    Hidden,
}

pub fn decide_mode(
    zoom: f64,
    screen_rect: Rect,
    viewport: Rect,
    minimized: bool,
    modal_open: bool,
) -> PanelMode {
    if minimized || screen_rect.intersect(&viewport).is_none() {
        PanelMode::Hidden
    } else if modal_open || zoom < PREVIEW_ZOOM_THRESHOLD {
        // A modal must not sit under a native view, which would draw on top of it.
        PanelMode::Preview
    } else {
        PanelMode::Live
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    const VIEWPORT: Rect = Rect::new(0.0, 36.0, 1280.0, 764.0);
    const ON_SCREEN: Rect = Rect::new(100.0, 100.0, 400.0, 300.0);

    #[test]
    fn below_threshold_is_preview() {
        assert_eq!(decide_mode(0.49, ON_SCREEN, VIEWPORT, false, false), PanelMode::Preview);
    }

    #[test]
    fn offscreen_is_hidden_even_when_live_zoom() {
        let offscreen = Rect::new(5000.0, 5000.0, 400.0, 300.0);
        assert_eq!(decide_mode(1.0, offscreen, VIEWPORT, false, false), PanelMode::Hidden);
        assert_eq!(decide_mode(1.0, offscreen, VIEWPORT, false, true), PanelMode::Hidden);
    }

    #[test]
    fn minimized_is_hidden() {
        assert_eq!(decide_mode(1.0, ON_SCREEN, VIEWPORT, true, false), PanelMode::Hidden);
    }

    #[test]
    fn modal_forces_preview() {
        assert_eq!(decide_mode(1.0, ON_SCREEN, VIEWPORT, false, true), PanelMode::Preview);
    }

    #[test]
    fn visible_at_threshold_is_live() {
        assert_eq!(PREVIEW_ZOOM_THRESHOLD, 0.5);
        assert_eq!(PREVIEW_REFRESH_SECS, 2);
        assert_eq!(decide_mode(PREVIEW_ZOOM_THRESHOLD, ON_SCREEN, VIEWPORT, false, false), PanelMode::Live);
    }
}
