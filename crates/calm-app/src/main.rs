mod background;
mod canvas_view;
mod dev_menu;
mod grid_paint;
mod settings_menu;
mod terminal;
mod theme;
mod toolbar;

use calm_core::geometry::{Rect, Size};
use calm_core::grid::{GridKind, mark_primitive_bound};
use calm_core::preview::PanelMode;
use calm_core::scene::PanelKind;
use std::sync::{Mutex, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Focusable, Window, WindowBounds,
    WindowOptions, px, size,
};

use crate::canvas_view::CanvasView;
use crate::settings_menu::SettingsState;

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
    remove_smoke_dir();
    std::process::exit(1);
}

/// The smoke's settings dir. Held here because `process::exit` and statics skip destructors,
/// so every smoke exit path drops it explicitly through `remove_smoke_dir`.
static SMOKE_DIR: Mutex<Option<tempfile::TempDir>> = Mutex::new(None);

/// Safe to call any number of times and from the panic hook.
fn remove_smoke_dir() {
    let dir = match SMOKE_DIR.try_lock() {
        Ok(mut slot) => slot.take(),
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().take(),
        // A panic while the lock is held; skipping beats deadlocking in the hook.
        Err(TryLockError::WouldBlock) => None,
    };
    if let Some(dir) = dir
        && let Err(err) = dir.close()
    {
        eprintln!("smoke: could not remove the settings temp dir: {err}");
    }
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
        if taken.len() != 2
            || taken
                .iter()
                .any(|count| *count == 0 || *count > SMOKE_MAX_SNAPSHOTS)
        {
            smoke_fail(&format!("preview refresh outside 1..=3 snapshots: {line}"));
        }
        // The grid painted every frame of that window; its draw count must not follow the
        // number of dots on screen.
        let (kind, last, most, rasters, canvas) = cx.update(|cx| {
            let view = view.read(cx);
            let grid = &view.grid;
            (
                view.settings.settings.grid.kind,
                grid.last_primitives,
                grid.max_primitives,
                grid.rasters,
                Size::new(view.viewport.size.width, view.viewport.size.height),
            )
        });
        let bound = mark_primitive_bound(canvas);
        println!(
            "smoke: grid kind={kind:?} primitives={last} max_primitives={most} bound={bound} \
             rasters={rasters}"
        );
        if kind != GridKind::None && last == 0 {
            smoke_fail("the grid drew nothing during the preview window");
        }
        if matches!(kind, GridKind::Dots | GridKind::Crosses) && most > bound {
            smoke_fail(&format!("grid drew {most} primitives, over {bound}"));
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
        // The gradient set by `smoke_settings` rendered once per size, off the UI thread.
        let (renders, idle, error) = cx.update(|cx| {
            let background = &view.read(cx).background;
            (
                background.renders,
                background.is_idle(),
                background.error.clone(),
            )
        });
        println!("smoke: background renders={renders} idle={idle} error={error:?}");
        if renders == 0 || !idle || error.is_some() {
            smoke_fail("gradient background did not render");
        }
        remove_smoke_dir();
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

/// The smoke keeps its settings in a fresh, uniquely named temp dir it owns, so the user's
/// file is never touched.
fn startup_settings(smoke: bool) -> SettingsState {
    if !smoke {
        return load_startup_settings(calm_core::settings::settings_path());
    }
    let dir = match tempfile::Builder::new().prefix("calm-smoke-").tempdir() {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("settings temp dir: {err}")),
    };
    let path = dir.path().join("settings.json");
    *SMOKE_DIR.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir);
    match calm_core::settings::load_settings_from(&path) {
        Ok(settings) => SettingsState {
            settings,
            path,
            notice: None,
        },
        Err(err) => smoke_fail(&format!("settings: {err}")),
    }
}

/// A rejected file is reported and moved aside before the defaults are used, so the next
/// save cannot overwrite it.
fn load_startup_settings(path: std::path::PathBuf) -> SettingsState {
    use calm_core::settings::{Settings, load_settings_from, set_aside_invalid};
    let error = match load_settings_from(&path) {
        Ok(settings) => {
            return SettingsState {
                settings,
                path,
                notice: None,
            };
        }
        Err(error) => error,
    };
    eprintln!("calm: {error}");
    let notice = match set_aside_invalid(&path) {
        Ok(moved) => {
            eprintln!(
                "calm: moved it to {}; using default settings",
                moved.display()
            );
            format!(
                "Your settings file could not be read ({error}). It was moved to {} and the defaults are in use.",
                moved.display()
            )
        }
        Err(move_error) => {
            eprintln!("calm: {move_error}");
            format!("Your settings file could not be read ({error}) or moved aside ({move_error}).")
        }
    };
    SettingsState {
        settings: Settings::default(),
        path,
        notice: Some(notice),
    }
}

/// The grid style the smoke draws: `CALM_SMOKE_GRID` (lines, dots, crosses, none), default
/// dots, the style whose per-dot quads once stalled preview refresh.
fn smoke_grid_kind() -> GridKind {
    let wanted = std::env::var("CALM_SMOKE_GRID").unwrap_or_else(|_| "dots".to_string());
    GridKind::ALL
        .into_iter()
        .find(|kind| kind.label().eq_ignore_ascii_case(&wanted))
        .unwrap_or_else(|| smoke_fail(&format!("CALM_SMOKE_GRID={wanted:?} is not a grid style")))
}

/// Sets a gradient background and the grid through the settings path, reads the file back,
/// then builds the native Settings window and checks every control was bound.
fn smoke_settings(view: &mut CanvasView, window: &mut Window, cx: &mut Context<CanvasView>) {
    use calm_core::background::BackgroundKind;
    use calm_core::settings::read_settings_from;
    use calm_core::settings_form::{Control, settings_form};
    let grid_kind = smoke_grid_kind();
    view.update_settings(
        |s| {
            s.background.kind = BackgroundKind::Gradient;
            s.grid.show = true;
            s.grid.kind = grid_kind;
            s.grid.major_every = 8;
        },
        window,
        cx,
    );
    let path = view.settings.path.clone();
    let saved = match read_settings_from(&path) {
        Ok(saved) => saved,
        Err(err) => smoke_fail(&format!("settings: {err}")),
    };
    if saved != view.settings.settings {
        smoke_fail(&format!("settings file {saved:?} differs from state"));
    }
    println!(
        "smoke: settings background={:?} grid={} grid_kind={:?} path={}",
        saved.background.kind,
        saved.grid.show,
        saved.grid.kind,
        path.display()
    );
    view.open_settings(window, cx);
    let first = view.settings_window.as_ref().map(|w| w.window_number());
    // A second open, as from another entry point, must reuse the same window.
    view.open_settings(window, cx);
    let second = view.settings_window.as_ref().map(|w| w.window_number());
    let count = calm_native::settings_window_count()
        .unwrap_or_else(|err| smoke_fail(&format!("settings window count: {err}")));
    println!(
        "smoke: settings_window opens=2 windows={count} same={}",
        first == second
    );
    if count != 1 || first.is_none() || first != second {
        smoke_fail("opening Settings twice did not reuse one window");
    }
    let expected = settings_form(&view.settings.settings, &view.form_context())
        .iter()
        .flat_map(|section| &section.fields)
        .filter(|field| !matches!(field.control, Control::Note(_)))
        .count();
    let Some(settings_window) = &view.settings_window else {
        smoke_fail("settings window was not created");
    };
    let bound = settings_window.bound_controls();
    println!(
        "smoke: settings_window visible={} controls={bound}",
        settings_window.is_visible()
    );
    if bound != expected || !settings_window.is_visible() {
        smoke_fail(&format!(
            "settings window bound {bound} of {expected} controls"
        ));
    }
    settings_window.hide();
    // Give focus back to the canvas, as a user closing Settings would.
    window.activate_window();
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys(canvas_view::key_bindings());
        cx.bind_keys(dev_menu::key_bindings());
        cx.bind_keys(settings_menu::key_bindings());
        dev_menu::install(cx);
        if args.smoke {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                remove_smoke_dir();
                previous(info);
            }));
        }
        let settings = startup_settings(args.smoke);
        settings_menu::install(cx, &settings.settings);
        // Single window in v1: closing it quits.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                // No-op outside the smoke; the smoke may quit here before its success path.
                remove_smoke_dir();
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
                view.set_settings_state(settings);
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
                    smoke_view.update(cx, |view, cx| smoke_settings(view, window, cx));
                    wait_for_terminal_frames(smoke_view, window.window_handle(), cx);
                });
            }
            view
        });
        if let Ok(canvas) = &opened {
            settings_menu::install_open_handler(cx, *canvas);
        }
        if let Err(err) = opened {
            if smoke {
                smoke_fail(&format!("open_window: {err:#}"));
            } else {
                eprintln!("calm: failed to open window: {err:#}");
            }
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
