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

#[cfg(test)]
mod tests {
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
