//! Toolbar strip above the canvas viewport.

use calm_core::interaction::Tool;
use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Styled, div, px, rgb,
};

use crate::canvas_view::CanvasView;
use crate::{dev_menu, theme};

pub const TOOLBAR_HEIGHT: f32 = 36.0;

pub fn toolbar(view: &CanvasView, cx: &mut Context<CanvasView>) -> impl IntoElement {
    let mut bar = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .h(px(TOOLBAR_HEIGHT))
        .w_full()
        .flex_none()
        .bg(rgb(theme::TOOLBAR))
        .text_color(rgb(theme::TEXT))
        .text_sm()
        .child(tool_button("Terminal", Tool::DrawTerminal, view, cx))
        .child(tool_button("Browser", Tool::DrawBrowser, view, cx));
    for (plugin_id, name) in &view.plugins {
        bar = bar.child(tool_button(
            name,
            Tool::DrawPlugin(plugin_id.clone()),
            view,
            cx,
        ));
    }
    // Save is wired up with layouts in Task 13.
    bar = bar
        .child(button("Save", false))
        .child(div().flex_1())
        .child(format!("{:.0}%", view.scene.camera.zoom * 100.0));
    if view.show_fps {
        bar = bar.child(dev_menu::fps_text(&view.frame_stats));
    }
    bar
}

fn tool_button(
    label: &str,
    tool: Tool,
    view: &CanvasView,
    cx: &mut Context<CanvasView>,
) -> impl IntoElement {
    let active = view.input.tool == tool;
    button(label, active).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _, window, cx| {
            view.input.tool = tool.clone();
            window.refresh();
            cx.notify();
        }),
    )
}

fn button(label: &str, active: bool) -> gpui::Div {
    let color = if active {
        theme::TOOLBAR_BUTTON_ACTIVE
    } else {
        theme::TOOLBAR_BUTTON
    };
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(color))
        .child(label.to_string())
}
