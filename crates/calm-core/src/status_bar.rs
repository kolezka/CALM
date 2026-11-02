//! Where the floating status bar sits: a rounded pill centered near the bottom of the canvas.
//! The bar itself is a native view (it has to draw above terminals); this is only its rect.

use crate::geometry::Rect;

pub const STATUS_BAR_HEIGHT: f64 = 36.0;
/// Gap between the bar and the canvas bottom edge.
pub const STATUS_BAR_BOTTOM_MARGIN: f64 = 14.0;
/// Smallest gap kept between the bar and the canvas side edges.
pub const STATUS_BAR_SIDE_MARGIN: f64 = 12.0;
/// Space between the text and the bar's rounded ends.
pub const STATUS_BAR_PADDING_X: f64 = 18.0;
pub const STATUS_BAR_MIN_WIDTH: f64 = 96.0;
pub const STATUS_BAR_CORNER_RADIUS: f64 = STATUS_BAR_HEIGHT / 2.0;
/// Separator between the bar's text items.
pub const STATUS_BAR_SEPARATOR: &str = "  ·  ";

/// The bar's rect in the same top-left coordinates as `canvas`, sized to fit
/// `content_width` of text. `None` when the canvas is too small to hold it.
pub fn status_bar_rect(canvas: Rect, content_width: f64) -> Option<Rect> {
    let max_width = canvas.size.width - 2.0 * STATUS_BAR_SIDE_MARGIN;
    let needed_height = STATUS_BAR_HEIGHT + 2.0 * STATUS_BAR_BOTTOM_MARGIN;
    if max_width < STATUS_BAR_MIN_WIDTH || canvas.size.height < needed_height {
        return None;
    }
    let width = (content_width.max(0.0) + 2.0 * STATUS_BAR_PADDING_X)
        .clamp(STATUS_BAR_MIN_WIDTH, max_width)
        .ceil();
    Some(Rect::new(
        (canvas.origin.x + (canvas.size.width - width) / 2.0).round(),
        canvas.bottom() - STATUS_BAR_BOTTOM_MARGIN - STATUS_BAR_HEIGHT,
        width,
        STATUS_BAR_HEIGHT,
    ))
}

/// Joins the non-empty items with the separator.
pub fn status_text<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    items
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(STATUS_BAR_SEPARATOR)
}

/// Remembers what the native pill last showed. Any frame or visibility change on that view
/// makes AppKit rebuild its tracking regions on the next display cycle, which sets the
/// cursor again; with a custom pointer that costs about 4 ms. So the pill is only touched
/// when its text or placement changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PillUpdate {
    text: Option<(String, f64)>,
    placement: Option<(Option<Rect>, f64)>,
    /// Measuring the text resizes the label, so it needs laying out again.
    label_stale: bool,
}

/// What the native pill has to touch for a placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PillLayout {
    /// The outer view's frame or visibility changed.
    pub frame: bool,
    /// The label needs its frame set inside the pill.
    pub label: bool,
}

impl PillUpdate {
    /// The width measured for `text` last time, or `None` when it must be measured again.
    pub fn cached_width(&self, text: &str) -> Option<f64> {
        self.text
            .as_ref()
            .filter(|(last, _)| last == text)
            .map(|(_, width)| *width)
    }

    pub fn record_text(&mut self, text: &str, width: f64) {
        self.text = Some((text.to_string(), width));
        self.label_stale = true;
    }

    /// What changed since the last placement, and records `rect` (or hiding, for `None`).
    /// `viewport_height` is part of the key since the native frame is flipped by it.
    pub fn placement(&mut self, rect: Option<Rect>, viewport_height: f64) -> PillLayout {
        let next = Some((rect, viewport_height));
        let frame = self.placement != next;
        self.placement = next;
        let label = rect.is_some() && (frame || self.label_stale);
        if label {
            self.label_stale = false;
        }
        PillLayout { frame, label }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unchanged_pill_is_not_touched_again() {
        let mut pill = super::PillUpdate::default();
        let rect = Some(crate::geometry::Rect::new(10.0, 20.0, 120.0, 36.0));
        assert_eq!(pill.cached_width("100%"), None);
        pill.record_text("100%", 41.0);
        assert_eq!(pill.cached_width("100%"), Some(41.0));
        assert!(pill.placement(rect, 800.0).frame);
        assert!(!pill.placement(rect, 800.0).frame);
    }

    #[test]
    fn a_new_text_rect_or_viewport_updates_the_pill() {
        let mut pill = super::PillUpdate::default();
        let rect = Some(crate::geometry::Rect::new(10.0, 20.0, 120.0, 36.0));
        pill.record_text("100%", 41.0);
        assert_eq!(pill.cached_width("101%"), None);
        assert!(pill.placement(rect, 800.0).frame);
        assert!(
            pill.placement(
                Some(crate::geometry::Rect::new(10.0, 20.0, 124.0, 36.0)),
                800.0
            )
            .frame
        );
        assert!(
            pill.placement(
                Some(crate::geometry::Rect::new(10.0, 20.0, 124.0, 36.0)),
                900.0
            )
            .frame
        );
        assert!(pill.placement(None, 900.0).frame);
        assert!(!pill.placement(None, 900.0).frame);
    }

    #[test]
    fn a_text_change_with_an_unchanged_rect_still_lays_out_the_label() {
        // Stats off, 100% to 99%: both fit the minimum width, so the outer rect is the same,
        // but measuring "99%" resized the label and it must be laid out again.
        let mut pill = super::PillUpdate::default();
        let rect = Some(crate::geometry::Rect::new(10.0, 20.0, 96.0, 36.0));
        pill.record_text("100%", 33.0);
        let first = pill.placement(rect, 800.0);
        assert_eq!(
            first,
            super::PillLayout {
                frame: true,
                label: true
            }
        );
        pill.record_text("99%", 25.0);
        assert_eq!(
            pill.placement(rect, 800.0),
            super::PillLayout {
                frame: false,
                label: true
            },
            "a changed text must re-lay out the label even when the outer rect is unchanged"
        );
        assert_eq!(pill.placement(rect, 800.0), super::PillLayout::default());
    }

    #[test]
    fn a_text_change_while_hidden_lays_out_the_label_when_shown() {
        let mut pill = super::PillUpdate::default();
        let rect = Some(crate::geometry::Rect::new(10.0, 20.0, 96.0, 36.0));
        pill.record_text("100%", 33.0);
        pill.placement(rect, 800.0);
        pill.placement(None, 800.0);
        pill.record_text("99%", 25.0);
        assert_eq!(pill.placement(None, 800.0), super::PillLayout::default());
        assert_eq!(
            pill.placement(rect, 800.0),
            super::PillLayout {
                frame: true,
                label: true
            }
        );
    }

    use super::*;

    const CANVAS: Rect = Rect::new(0.0, 0.0, 1280.0, 800.0);

    #[test]
    fn bar_is_centered_and_sits_above_the_bottom_margin() {
        let bar = status_bar_rect(CANVAS, 200.0).unwrap();
        assert_eq!(bar, Rect::new(522.0, 750.0, 236.0, 36.0));
        assert_eq!(bar.bottom(), CANVAS.bottom() - STATUS_BAR_BOTTOM_MARGIN);
        assert_eq!(bar.origin.x - CANVAS.origin.x, CANVAS.right() - bar.right());
    }

    #[test]
    fn bar_follows_a_canvas_with_an_offset_origin() {
        let canvas = Rect::new(100.0, 40.0, 400.0, 300.0);
        let bar = status_bar_rect(canvas, 64.0).unwrap();
        assert_eq!(bar, Rect::new(250.0, 290.0, 100.0, 36.0));
    }

    #[test]
    fn short_text_gets_the_minimum_width() {
        let bar = status_bar_rect(CANVAS, 10.0).unwrap();
        assert_eq!(bar.size.width, STATUS_BAR_MIN_WIDTH);
    }

    #[test]
    fn long_text_is_clamped_inside_the_side_margins() {
        let bar = status_bar_rect(CANVAS, 5000.0).unwrap();
        assert_eq!(bar.origin.x, STATUS_BAR_SIDE_MARGIN);
        assert_eq!(bar.right(), CANVAS.right() - STATUS_BAR_SIDE_MARGIN);
    }

    #[test]
    fn bar_never_leaves_the_canvas() {
        for content in [0.0, 50.0, 300.0, 1255.0, 9999.0] {
            let bar = status_bar_rect(CANVAS, content).unwrap();
            assert_eq!(bar.intersect(&CANVAS), Some(bar), "content width {content}");
        }
    }

    #[test]
    fn tiny_canvas_has_no_bar() {
        assert_eq!(
            status_bar_rect(Rect::new(0.0, 0.0, 100.0, 800.0), 0.0),
            None
        );
        assert_eq!(
            status_bar_rect(Rect::new(0.0, 0.0, 1280.0, 60.0), 0.0),
            None
        );
        assert_eq!(status_bar_rect(Rect::new(0.0, 0.0, 0.0, 0.0), 0.0), None);
    }

    #[test]
    fn status_text_skips_empty_items() {
        assert_eq!(
            status_text(["", "150%", "", "FPS 60.0"]),
            "150%  ·  FPS 60.0"
        );
        assert_eq!(status_text(["100%"]), "100%");
        assert_eq!(status_text([]), "");
    }
}
