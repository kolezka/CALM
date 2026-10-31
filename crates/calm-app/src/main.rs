mod canvas_view;
mod theme;
mod toolbar;

use calm_core::geometry::Rect;
use calm_core::scene::PanelKind;
use gpui::{App, AppContext, Bounds, Focusable, WindowBounds, WindowOptions, px, size};

use crate::canvas_view::CanvasView;

#[derive(Default)]
struct Args {
    smoke: bool,
    fullscreen: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--smoke" => args.smoke = true,
            "--fullscreen" => args.fullscreen = true,
            other => eprintln!("calm: ignoring unknown argument {other}"),
        }
    }
    args
}

/// Two placeholders at zoom 1.5, so the smoke frame paints real panels. The first
/// starts above the canvas so its chrome and title cross the toolbar edge.
fn add_smoke_panels(view: &mut CanvasView) {
    view.scene.camera.zoom = 1.5;
    view.scene.add(
        PanelKind::Terminal {
            command: None,
            cwd: None,
        },
        Rect::new(20.0, -20.0, 320.0, 200.0),
    );
    view.scene.add(
        PanelKind::Browser {
            url: "about:blank".to_string(),
        },
        Rect::new(380.0, 60.0, 300.0, 220.0),
    );
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys(canvas_view::key_bindings());
        // Single window in v1: closing it quits.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let window_bounds = if args.fullscreen {
            WindowBounds::Fullscreen(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        };
        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            ..Default::default()
        };
        let smoke = args.smoke;
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let mut view = CanvasView::new(cx);
                if smoke {
                    add_smoke_panels(&mut view);
                }
                view
            });
            view.focus_handle(cx).focus(window, cx);
            if smoke {
                let smoke_view = view.clone();
                // Quit only after a real frame, so a passing smoke means the window drew.
                window.on_next_frame(move |window, cx| {
                    let viewport = window.viewport_size();
                    println!(
                        "smoke: viewport={}x{} scale={}",
                        f32::from(viewport.width),
                        f32::from(viewport.height),
                        window.scale_factor()
                    );
                    let canvas = smoke_view.read(cx);
                    let rect = canvas.viewport;
                    let panels = canvas.scene.panels.len();
                    println!(
                        "smoke: canvas viewport={}x{} panels={panels}",
                        rect.size.width, rect.size.height
                    );
                    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || panels != 2 {
                        eprintln!("smoke: canvas did not lay out with both panels");
                        std::process::exit(1);
                    }
                    cx.quit();
                });
            }
            view
        });
        if let Err(err) = opened {
            if smoke {
                eprintln!("smoke: FAILED open_window: {err:#}");
            } else {
                eprintln!("calm: failed to open window: {err:#}");
            }
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
