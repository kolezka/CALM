mod canvas_view;
mod dev_menu;
mod terminal;
mod theme;
mod toolbar;

use calm_core::geometry::Rect;
use calm_core::preview::PanelMode;
use calm_core::scene::PanelKind;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Focusable, Window, WindowBounds,
    WindowOptions, px, size,
};

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

/// Two live terminals at zoom 1.5. The first starts above the canvas so its chrome and
/// content cross the toolbar edge and the native view is clipped.
fn add_smoke_panels(
    view: &mut CanvasView,
    window: &mut Window,
    cx: &mut Context<CanvasView>,
) -> Result<(), String> {
    view.scene.camera.zoom = 1.5;
    for rect in [
        Rect::new(20.0, -20.0, 320.0, 200.0),
        Rect::new(380.0, 60.0, 300.0, 220.0),
    ] {
        let kind = PanelKind::Terminal {
            command: None,
            cwd: None,
        };
        let id = view.scene.add(kind, rect);
        view.spawn_terminal(id, terminal::options(None, None), window, cx)?;
    }
    Ok(())
}

const SMOKE_FRAME_TIMEOUT: Duration = Duration::from_secs(10);

fn smoke_fail(reason: &str) -> ! {
    eprintln!("smoke: FAILED {reason}");
    std::process::exit(1);
}

const SMOKE_FPS_FRAMES: usize = 10;
const SMOKE_FPS_TIMEOUT: Duration = Duration::from_secs(5);

/// How long both terminals may take to reach a mode after a zoom change.
const SMOKE_PREVIEW_TIMEOUT: Duration = Duration::from_secs(5);
const SMOKE_PREVIEW_ZOOM: f64 = 0.4;
const SMOKE_LIVE_ZOOM: f64 = 1.5;
/// How long smoke counts preview snapshots, and the most it allows in that time.
const SMOKE_SNAPSHOT_WINDOW: Duration = Duration::from_secs(3);
const SMOKE_MAX_SNAPSHOTS: u64 = 3;

/// How long the native restack may take to show after the scene order changes.
const SMOKE_ZORDER_TIMEOUT: Duration = Duration::from_secs(2);

/// Polls until both terminals have drawn a frame, raises the first one in the scene and
/// checks that the native views restack to match. Prints the result and quits.
fn wait_for_terminal_frames(view: Entity<CanvasView>, window: AnyWindowHandle, cx: &mut App) {
    let started = Instant::now();
    cx.spawn(async move |cx| {
        let status_line;
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let status = cx.update(|cx| view.update(cx, |view, cx| view.terminal_status(cx)));
            if status.len() != 2 {
                smoke_fail(&format!("expected 2 terminals, found {}", status.len()));
            }
            let drawn = status.iter().all(|(frames, _)| *frames > 0);
            if !drawn && started.elapsed() <= SMOKE_FRAME_TIMEOUT {
                continue;
            }
            let frames: Vec<String> = status.iter().map(|(f, _)| f.to_string()).collect();
            let alive: Vec<String> = status.iter().map(|(_, a)| a.to_string()).collect();
            let line = format!(
                "smoke: terminals=2 frames=[{}] alive=[{}]",
                frames.join(","),
                alive.join(",")
            );
            if !drawn {
                smoke_fail(&format!("no frame within 10 s: {line}"));
            }
            if !status.iter().all(|(_, alive)| *alive) {
                smoke_fail(&format!("a terminal exited: {line}"));
            }
            status_line = line;
            break;
        }
        println!("{status_line}");

        // The first terminal was created first, so it starts at the back.
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                if let Some(first) = view.scene.panels.first().map(|panel| panel.id) {
                    view.scene.bring_to_front(first);
                }
                cx.notify();
            })
        });
        let raised = Instant::now();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let (scene, native) = cx.update(|cx| view.read(cx).terminal_zorder(cx));
            let line = format!("zorder={native:?} scene={scene:?}");
            if scene == native && scene.len() == 2 {
                println!("smoke: {line}");
                break;
            }
            if raised.elapsed() > SMOKE_ZORDER_TIMEOUT {
                smoke_fail(&format!("native order does not match the scene: {line}"));
            }
        }

        let toggled = cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.set_show_fps(true, window, cx))
        });
        if let Err(err) = toggled {
            smoke_fail(&format!("show_fps toggle: {err:#}"));
        }
        let shown = Instant::now();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let (frames, fps) = cx.update(|cx| {
                let stats = &view.read(cx).frame_stats;
                (stats.frames(), stats.fps())
            });
            // The first frame after the toggle starts the window; wait for 10 more.
            if frames > SMOKE_FPS_FRAMES {
                match fps {
                    Some(fps) if fps > 0.0 => println!("smoke: fps={fps:.1}"),
                    _ => smoke_fail(&format!("no fps after {frames} frames")),
                }
                break;
            }
            if shown.elapsed() > SMOKE_FPS_TIMEOUT {
                smoke_fail(&format!("only {frames} frames with show_fps on"));
            }
        }
        // Zoom out: both terminals switch to GPUI-drawn snapshots.
        cx.update(|cx| {
            view.update(cx, |view, _| view.preview_input_targets = 0);
            set_zoom(&view, SMOKE_PREVIEW_ZOOM, cx);
        });
        let zoomed = Instant::now();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let previews = cx.update(|cx| view.read(cx).terminal_previews(cx));
            let ready = previews.len() == 2
                && previews
                    .iter()
                    .all(|(mode, snapshot)| *mode == PanelMode::Preview && *snapshot);
            if ready {
                println!("smoke: preview=2 snapshots=[true,true]");
                break;
            }
            if zoomed.elapsed() > SMOKE_PREVIEW_TIMEOUT {
                smoke_fail(&format!("no preview snapshots at zoom 0.4: {previews:?}"));
            }
        }

        // show_fps is still on, so a frame paints every tick. Readbacks must stay at
        // one per refresh period anyway.
        let before = cx.update(|cx| view.read(cx).snapshot_counts(cx));
        cx.background_executor().timer(SMOKE_SNAPSHOT_WINDOW).await;
        let (after, previews, targets) = cx.update(|cx| {
            let view = view.read(cx);
            (
                view.snapshot_counts(cx),
                view.terminal_previews(cx),
                view.preview_input_targets,
            )
        });
        if !previews.iter().all(|(mode, _)| *mode == PanelMode::Preview) {
            smoke_fail(&format!(
                "left preview while counting snapshots: {previews:?}"
            ));
        }
        let taken: Vec<u64> = after
            .iter()
            .zip(&before)
            .map(|(after, before)| after - before)
            .collect();
        let line = format!(
            "preview_snapshots_in_3s=[{}]",
            taken
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        println!("smoke: {line}");
        if taken.len() != 2 || taken.iter().any(|count| *count > SMOKE_MAX_SNAPSHOTS) {
            smoke_fail(&format!("too many preview snapshots: {line}"));
        }
        // Input divs over a preview would steal its scroll and keys from the canvas.
        println!("smoke: preview_input_targets={targets}");
        if targets != 0 {
            smoke_fail(&format!("{targets} terminal input divs over previews"));
        }

        // Zoom back in: both return to live native views.
        cx.update(|cx| set_zoom(&view, SMOKE_LIVE_ZOOM, cx));
        let zoomed = Instant::now();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let (previews, status) = cx.update(|cx| {
                let previews = view.read(cx).terminal_previews(cx);
                let status = view.update(cx, |view, cx| view.terminal_status(cx));
                (previews, status)
            });
            let live =
                previews.len() == 2 && previews.iter().all(|(mode, _)| *mode == PanelMode::Live);
            if live {
                if !status.iter().all(|(_, alive)| *alive) {
                    smoke_fail(&format!("a terminal exited during preview: {status:?}"));
                }
                println!("smoke: live=2");
                break;
            }
            if zoomed.elapsed() > SMOKE_PREVIEW_TIMEOUT {
                smoke_fail(&format!("terminals not live at zoom 1.5: {previews:?}"));
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn set_zoom(view: &Entity<CanvasView>, zoom: f64, cx: &mut App) {
    view.update(cx, |view, cx| {
        view.scene.camera.zoom = zoom;
        cx.notify();
    });
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys(canvas_view::key_bindings());
        cx.bind_keys(dev_menu::key_bindings());
        dev_menu::install(cx);
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
                if smoke && let Err(err) = add_smoke_panels(&mut view, window, cx) {
                    smoke_fail(&format!("terminal spawn: {err}"));
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
                        smoke_fail("canvas did not lay out with both panels");
                    }
                    wait_for_terminal_frames(smoke_view, window.window_handle(), cx);
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
