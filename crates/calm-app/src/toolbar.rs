//! Status strip above the canvas viewport: save flash, plugin errors, zoom and FPS.
//! Panels are added from the right-click menu, so the strip has no buttons.

use gpui::{IntoElement, ParentElement, Styled, div, px, rgb};

use crate::canvas_view::CanvasView;
use crate::{dev_menu, theme};

pub const TOOLBAR_HEIGHT: f32 = 36.0;

pub fn toolbar(view: &CanvasView) -> impl IntoElement {
    let mut bar = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_2()
        .h(px(TOOLBAR_HEIGHT))
        .w_full()
        .flex_none()
        .bg(rgb(theme::TOOLBAR))
        .text_color(rgb(theme::TEXT))
        .text_sm();
    // Task 13 sets the flash after a layout save.
    if let Some(path) = &view.saved_flash {
        bar = bar.child(format!("Saved {path}"));
    }
    if view.plugin_errors > 0 {
        // stderr names each skipped plugin folder and why.
        bar = bar.child(
            div()
                .text_color(rgb(theme::ERROR_TEXT))
                .child(format!("plugins: {} errors", view.plugin_errors)),
        );
    }
    bar = bar
        .child(div().flex_1())
        .child(format!("{:.0}%", view.scene.camera.zoom * 100.0));
    if view.show_fps {
        bar = bar.child(dev_menu::fps_text(&view.frame_stats));
    }
    bar
}
