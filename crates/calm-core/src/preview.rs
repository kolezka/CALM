use std::time::{Duration, Instant};

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

/// `preview_below_zoom` comes from settings; `PREVIEW_ZOOM_THRESHOLD` is its default.
pub fn decide_mode(
    zoom: f64,
    preview_below_zoom: f64,
    screen_rect: Rect,
    viewport: Rect,
    minimized: bool,
    modal_open: bool,
) -> PanelMode {
    if minimized || screen_rect.intersect(&viewport).is_none() {
        PanelMode::Hidden
    } else if modal_open || zoom < preview_below_zoom {
        // A modal must not sit under a native view, which would draw on top of it.
        PanelMode::Preview
    } else {
        PanelMode::Live
    }
}

/// True when a preview snapshot should be taken: none yet, or the last one is
/// at least `PREVIEW_REFRESH_SECS` old.
pub fn refresh_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|at| {
        now.saturating_duration_since(at) >= Duration::from_secs(PREVIEW_REFRESH_SECS)
    })
}

/// The snapshot a preview draws. A failed refresh keeps the last good image as `Stale`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewImage<T> {
    /// No snapshot ever succeeded: draw a flat rect with the title.
    None,
    Fresh(T),
    Stale(T),
}

impl<T> PreviewImage<T> {
    /// A refresh succeeded. Returns the replaced image so the caller can free it.
    pub fn refreshed(&mut self, image: T) -> Option<T> {
        std::mem::replace(self, PreviewImage::Fresh(image)).into_image()
    }

    /// A refresh failed: keep the image, if any, and mark it stale.
    pub fn refresh_failed(&mut self) {
        *self = match std::mem::replace(self, PreviewImage::None) {
            PreviewImage::Fresh(image) | PreviewImage::Stale(image) => PreviewImage::Stale(image),
            PreviewImage::None => PreviewImage::None,
        };
    }

    /// Drops the image, returning it so the caller can free it.
    pub fn take(&mut self) -> Option<T> {
        std::mem::replace(self, PreviewImage::None).into_image()
    }

    pub fn image(&self) -> Option<&T> {
        match self {
            PreviewImage::None => None,
            PreviewImage::Fresh(image) | PreviewImage::Stale(image) => Some(image),
        }
    }

    pub fn is_stale(&self) -> bool {
        matches!(self, PreviewImage::Stale(_))
    }

    fn into_image(self) -> Option<T> {
        match self {
            PreviewImage::None => None,
            PreviewImage::Fresh(image) | PreviewImage::Stale(image) => Some(image),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    #[test]
    fn first_snapshot_is_due() {
        assert!(refresh_due(None, Instant::now()));
    }

    #[test]
    fn refresh_not_due_before_2s() {
        let last = Instant::now();
        assert!(!refresh_due(Some(last), last));
        assert!(!refresh_due(Some(last), last + Duration::from_millis(1900)));
        assert!(!refresh_due(Some(last), last + Duration::from_millis(1999)));
    }

    #[test]
    fn refresh_due_at_2s() {
        let last = Instant::now();
        assert!(refresh_due(Some(last), last + Duration::from_secs(2)));
        assert!(refresh_due(Some(last), last + Duration::from_millis(2500)));
    }

    #[test]
    fn failed_refresh_without_image_stays_none() {
        let mut preview: PreviewImage<u32> = PreviewImage::None;
        preview.refresh_failed();
        assert_eq!(preview, PreviewImage::None);
    }

    #[test]
    fn failed_refresh_marks_image_stale_and_success_clears_it() {
        let mut preview = PreviewImage::None;
        assert_eq!(preview.refreshed(1), None);
        assert_eq!(preview, PreviewImage::Fresh(1));
        preview.refresh_failed();
        assert_eq!(preview, PreviewImage::Stale(1));
        assert!(preview.is_stale());
        assert_eq!(preview.image(), Some(&1));
        preview.refresh_failed();
        assert_eq!(preview, PreviewImage::Stale(1));
        assert_eq!(preview.refreshed(2), Some(1));
        assert_eq!(preview, PreviewImage::Fresh(2));
        assert_eq!(preview.take(), Some(2));
        assert_eq!(preview, PreviewImage::None);
    }

    const VIEWPORT: Rect = Rect::new(0.0, 36.0, 1280.0, 764.0);
    const ON_SCREEN: Rect = Rect::new(100.0, 100.0, 400.0, 300.0);

    #[test]
    fn below_threshold_is_preview() {
        assert_eq!(
            decide_mode(
                0.49,
                PREVIEW_ZOOM_THRESHOLD,
                ON_SCREEN,
                VIEWPORT,
                false,
                false
            ),
            PanelMode::Preview
        );
    }

    #[test]
    fn threshold_comes_from_the_caller() {
        assert_eq!(
            decide_mode(0.7, 0.8, ON_SCREEN, VIEWPORT, false, false),
            PanelMode::Preview
        );
        assert_eq!(
            decide_mode(0.3, 0.2, ON_SCREEN, VIEWPORT, false, false),
            PanelMode::Live
        );
    }

    #[test]
    fn offscreen_is_hidden_even_when_live_zoom() {
        let offscreen = Rect::new(5000.0, 5000.0, 400.0, 300.0);
        assert_eq!(
            decide_mode(
                1.0,
                PREVIEW_ZOOM_THRESHOLD,
                offscreen,
                VIEWPORT,
                false,
                false
            ),
            PanelMode::Hidden
        );
        assert_eq!(
            decide_mode(
                1.0,
                PREVIEW_ZOOM_THRESHOLD,
                offscreen,
                VIEWPORT,
                false,
                true
            ),
            PanelMode::Hidden
        );
    }

    #[test]
    fn minimized_is_hidden() {
        assert_eq!(
            decide_mode(
                1.0,
                PREVIEW_ZOOM_THRESHOLD,
                ON_SCREEN,
                VIEWPORT,
                true,
                false
            ),
            PanelMode::Hidden
        );
    }

    #[test]
    fn modal_forces_preview() {
        assert_eq!(
            decide_mode(
                1.0,
                PREVIEW_ZOOM_THRESHOLD,
                ON_SCREEN,
                VIEWPORT,
                false,
                true
            ),
            PanelMode::Preview
        );
    }

    #[test]
    fn visible_at_threshold_is_live() {
        assert_eq!(PREVIEW_ZOOM_THRESHOLD, 0.5);
        assert_eq!(PREVIEW_REFRESH_SECS, 2);
        assert_eq!(
            decide_mode(
                PREVIEW_ZOOM_THRESHOLD,
                PREVIEW_ZOOM_THRESHOLD,
                ON_SCREEN,
                VIEWPORT,
                false,
                false
            ),
            PanelMode::Live
        );
    }
}
