mod background;
mod browser_panel;
mod canvas_view;
mod dev_menu;
mod grid_paint;
mod icon_image;
mod panel_settings;
mod plugin_host;
mod plugin_services;
mod settings_menu;
mod smoke_runner;
mod terminal;
mod terminal_profiles;
mod theme;
mod tui_host;

use calm_core::geometry::{Point, Rect, Size};
use calm_core::grid::{GridKind, mark_primitive_bound};
use calm_core::interaction::{Effect, ResizeEdge};
use calm_core::join::JOIN_SNAP_GAP;
use calm_core::layout::{load_layout_from, move_aside};
use calm_core::preview::PanelMode;
use calm_core::restore::{
    LaunchExit, LaunchFlags, RestorePlan, Source, after_quarantine, pick_source, plan_restore,
};
use calm_core::scene::{PanelId, PanelKind};
use calm_core::settings::RestoreOnLaunch;
use calm_core::snap::{SnapConfig, snap_move};
use calm_core::tui::TuiState;
use std::cell::Cell;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::{Mutex, PoisonError, TryLockError};
use std::task::Poll;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Focusable, TitlebarOptions, Window,
    WindowBounds, WindowOptions, px, size,
};

use crate::canvas_view::CanvasView;
use crate::settings_menu::SettingsState;

#[derive(Default)]
struct Args {
    smoke: bool,
    fullscreen: bool,
    restore: LaunchFlags,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--smoke" => args.smoke = true,
            "--fullscreen" => args.fullscreen = true,
            "--no-restore" => args.restore.no_restore = true,
            "--layout" => match rest.next() {
                Some(name) => args.restore.layout = Some(name),
                None => {
                    eprintln!("calm: --layout needs a layout name");
                    std::process::exit(2);
                }
            },
            other => eprintln!("calm: ignoring unknown argument {other}"),
        }
    }
    args
}

/// Reads what the launch needs, moves a session that failed to load aside, and returns
/// the plan. Reads only `session` and, for `--layout`, `layouts/<name>.json`.
fn launch_plan(
    flags: &LaunchFlags,
    setting: RestoreOnLaunch,
    session: &Path,
    layouts: &Path,
) -> Result<RestorePlan, LaunchExit> {
    let (source, notice) = pick_source(flags, setting);
    let dir = session.parent().unwrap_or(Path::new("."));
    let stem = session
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session");
    let session_read = load_layout_from(dir, stem);
    let layout_read = match &source {
        Source::NamedLayout(name) => Some(load_layout_from(layouts, name)),
        _ => None,
    };
    let mut plan = plan_restore(&source, notice, session_read, layout_read)?;
    if let Some(path) = plan.quarantine.clone() {
        after_quarantine(&mut plan, move_aside(&path));
    }
    Ok(plan)
}

/// A page with no network use that clicks its own link as soon as it parses, so following
/// it proves link navigation and the URL event path. Fully percent-encoded for NSURL.
const SMOKE_BROWSER_PAGE: &str = "data:text/html,%3Ca%20id%3Dl%20href%3D%22about%3Ablank%23clicked%22%3Elink%3C%2Fa%3E%3Cscript%3Edocument.getElementById(%22l%22).click()%3C%2Fscript%3E";
const SMOKE_BROWSER_TARGET: &str = "about:blank#clicked";
const SMOKE_BROWSER_TIMEOUT: Duration = Duration::from_secs(10);

/// Two live terminals at zoom 1.5 and one browser below them. The first terminal starts
/// above the canvas so its chrome and content cross the window's top edge and the native
/// view is clipped.
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
            profile: None,
        };
        let id = view.scene.add(kind, rect);
        view.spawn_terminal(id, terminal::options(None, None), window, cx)?;
    }
    let url = SMOKE_BROWSER_PAGE.to_string();
    let id = view.scene.add(
        PanelKind::Browser {
            url: url.clone(),
            page_zoom: None,
        },
        Rect::new(20.0, 220.0, 320.0, 200.0),
    );
    view.spawn_browser(id, &url, window, cx)?;
    Ok(())
}

const SMOKE_FRAME_TIMEOUT: Duration = Duration::from_secs(10);

/// Inside a step, ends only that step: `smoke_step` records the failure and moves on.
/// Elsewhere (setup before the steps), ends the process with exit code 1.
fn smoke_fail(reason: &str) -> ! {
    eprintln!("smoke: FAILED {reason}");
    if IN_SMOKE_STEP.get() {
        std::panic::panic_any(SmokeFailure(reason.to_string()));
    }
    remove_smoke_dir();
    std::process::exit(1);
}

/// The panic payload of a failed step. The panic hook stays quiet for it.
struct SmokeFailure(String);

thread_local! {
    /// True only while `smoke_step` polls a step, so setup failures still exit.
    static IN_SMOKE_STEP: Cell<bool> = const { Cell::new(false) };
}

/// Runs one named step unless the filter or a failed requirement rules it out, and records
/// PASS, FAIL or SKIPPED. Steps must not call `smoke_fail` inside a GPUI update closure:
/// unwinding through one would leave GPUI's state broken for the later steps.
async fn smoke_step(run: &mut smoke_runner::SmokeRun, name: &str, step: impl Future<Output = ()>) {
    match run.decide(name) {
        None => return,
        Some(Err(why)) => {
            println!("{}", run.record(name, smoke_runner::Outcome::Skipped(why)));
            return;
        }
        Some(Ok(())) => {}
    }
    let mut step = std::pin::pin!(step);
    let result = futures::future::poll_fn(|task| {
        IN_SMOKE_STEP.set(true);
        let polled = std::panic::catch_unwind(AssertUnwindSafe(|| step.as_mut().poll(task)));
        IN_SMOKE_STEP.set(false);
        match polled {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(())) => Poll::Ready(Ok(())),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    })
    .await;
    let line = match result {
        Ok(()) => run.record(name, smoke_runner::Outcome::Pass),
        Err(payload) => match payload.downcast::<SmokeFailure>() {
            Ok(failure) => run.fail(name, failure.0),
            // A real panic is a bug, not a step result: keep its old behavior.
            Err(payload) => std::panic::resume_unwind(payload),
        },
    };
    println!("{line}");
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
/// CPU needs two samples one interval (default 1 s) apart. Bounds the sampler wait.
const SMOKE_STATS_TIMEOUT: Duration = Duration::from_secs(5);
/// A paint after the sampler has CPU puts it in the pill. Bounds that wait.
const SMOKE_STATS_PAINT_TIMEOUT: Duration = Duration::from_secs(2);
/// CALM with GPUI, Metal and two terminals is far above this; less means a bad reading.
const SMOKE_STATS_MIN_MB: f64 = 10.0;

/// Waits until the sampler reports two samples and a CPU reading, then for the status
/// pill to show CPU and memory, and parses them back out of its text, so the line checks
/// what the user sees, not only the sampler.
async fn smoke_process_stats(view: &gpui::Entity<CanvasView>, cx: &mut gpui::AsyncApp) {
    let started = Instant::now();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let (samples, cpu) = cx.update(|cx| {
            let view = view.read(cx);
            let cpu = view.process_stats.and_then(|sample| sample.cpu_percent);
            (view.process_samples, cpu)
        });
        if samples >= 2 && cpu.is_some() {
            break;
        }
        if started.elapsed() > SMOKE_STATS_TIMEOUT {
            smoke_fail(&format!(
                "stats: sampler has {samples} samples and cpu={cpu:?} after {:.1}s, CPU needs two",
                started.elapsed().as_secs_f64()
            ));
        }
    }
    let started = Instant::now();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
        let (text, fps) = cx.update(|cx| {
            let view = view.read(cx);
            let text = view.status_bar_state().map(|(_, _, text)| text);
            (text, view.frame_stats.fps())
        });
        let text = match text {
            Ok(text) => text,
            Err(err) => smoke_fail(&format!("stats: no status bar: {err}")),
        };
        if let (Some(cpu), Some(mem)) = calm_core::process_stats::parse_stats_text(&text) {
            if !(cpu >= 0.0 && cpu.is_finite()) || mem <= SMOKE_STATS_MIN_MB {
                smoke_fail(&format!(
                    "stats: implausible cpu={cpu} mem={mem}MB in {text:?}"
                ));
            }
            let fps = fps.unwrap_or(0.0);
            println!("smoke: stats fps={fps:.1} cpu={cpu:.1}% mem={mem:.0}MB");
            return;
        }
        if started.elapsed() > SMOKE_STATS_PAINT_TIMEOUT {
            smoke_fail(&format!(
                "stats: sampler has CPU but the status bar does not show it: {text:?}"
            ));
        }
    }
}
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
/// The native status bar exists, is shown and sits above every panel view.
fn smoke_status_bar(state: Result<(bool, bool, String), String>) {
    match state {
        Ok((visible, front, text)) => {
            println!("smoke: status_bar visible={visible} front={front} text={text:?}");
            if !visible || !front {
                smoke_fail("status bar is hidden or below a panel");
            }
        }
        Err(err) => smoke_fail(&format!("status bar missing: {err}")),
    }
}

/// Menu reorders must update AppKit's stack before any later GPUI paint. An occluded
/// window may not paint at all, but these native views still exist.
fn smoke_native_reorder(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    let result = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let last = view.scene.panels.last().map(|panel| panel.id)?;
            view.apply_menu_choice(
                calm_core::menu::PANEL_SEND_TO_BACK,
                canvas_view::MenuTarget::Panel(last),
                window,
                cx,
            );
            let back = (view.native_zorder(cx), view.status_bar_state());
            view.apply_menu_choice(
                calm_core::menu::PANEL_BRING_TO_FRONT,
                canvas_view::MenuTarget::Panel(last),
                window,
                cx,
            );
            Some((back, (view.native_zorder(cx), view.status_bar_state())))
        })
    });
    let (back, front) = match result {
        Ok(Some(orders)) => orders,
        Ok(None) => smoke_fail("no panel to reorder"),
        Err(err) => smoke_fail(&format!("native reorder window: {err:#}")),
    };
    for (action, (orders, bar)) in [("send to back", back), ("bring to front", front)] {
        let (scene, native) = orders;
        if scene != native {
            smoke_fail(&format!(
                "native order after {action}: zorder={native:?} scene={scene:?}"
            ));
        }
        if !matches!(bar, Ok((true, true, _))) {
            smoke_fail(&format!("status bar after {action}: {bar:?}"));
        }
    }
    println!("smoke: native reorder back=match front=match bar=front");
}

fn wait_for_terminal_frames(view: Entity<CanvasView>, window: AnyWindowHandle, cx: &mut App) {
    let started = Instant::now();
    cx.spawn(async move |cx| {
        let only = std::env::var(smoke_runner::ONLY_ENV).ok();
        let mut run = smoke_runner::SmokeRun::new(only.as_deref())
            .unwrap_or_else(|err| smoke_fail(&err));
        println!("{}", run.filter_line());
        smoke_step(&mut run, "terminals", async {
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
        })
        .await;
        smoke_step(&mut run, "zorder", async {
            // The first terminal was created first, so it starts at the back. Raise it through
            // the menu path, which restacks the native views at once; a raw scene change waits
            // for a paint that an occluded window never does.
            let raised = cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    let first = view.scene.panels.first().map(|panel| panel.id)?;
                    view.apply_menu_choice(
                        calm_core::menu::PANEL_BRING_TO_FRONT,
                        canvas_view::MenuTarget::Panel(first),
                        window,
                        cx,
                    );
                    Some((view.native_zorder(cx), view.status_bar_state()))
                })
            });
            let ((scene, native), bar) = match raised {
                Ok(Some(state)) => state,
                Ok(None) => smoke_fail("no panel to raise"),
                Err(err) => smoke_fail(&format!("zorder window: {err:#}")),
            };
            let line = format!("zorder={native:?} scene={scene:?}");
            // Two terminals and the browser share the native stack.
            if scene != native || scene.len() != 3 {
                smoke_fail(&format!("native order does not match the scene: {line}"));
            }
            println!("smoke: {line}");
            // Restacking the panels must not bury the status bar.
            smoke_status_bar(bar);
        })
        .await;
        smoke_step(&mut run, "join", async {
            // Drop the second terminal 5 units right of the first through the real snap path:
            // it must land one unit off and render as one joined pair with a visible divider.
            let gap = cx.update(|cx| {
                view.update(cx, |view, cx| {
                    let mut ids: Vec<PanelId> = view.scene.panels.iter().map(|p| p.id).collect();
                    ids.sort();
                    let anchor = view.scene.get(ids[0]).map(|p| p.rect)?;
                    let moving = view.scene.get_mut(ids[1])?;
                    let dropped = Rect::new(
                        anchor.right() + 5.0,
                        anchor.origin.y,
                        moving.rect.size.width,
                        moving.rect.size.height,
                    );
                    moving.rect = snap_move(dropped, &[anchor], &SnapConfig::default());
                    let gap = moving.rect.origin.x - anchor.right();
                    cx.notify();
                    Some(gap)
                })
            });
            if gap != Some(JOIN_SNAP_GAP) {
                smoke_fail(&format!(
                    "flush drop landed at gap {gap:?}, not {JOIN_SNAP_GAP}"
                ));
            }
            let moved = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let (pairs, dividers) = cx.update(|cx| view.read(cx).join_counts);
                if (pairs, dividers) == (1, 1) {
                    println!("smoke: join pairs={pairs} dividers={dividers} gap={JOIN_SNAP_GAP}");
                    break;
                }
                if moved.elapsed() > SMOKE_ZORDER_TIMEOUT {
                    smoke_fail(&format!(
                        "flush pair not joined: pairs={pairs} dividers={dividers}"
                    ));
                }
            }
        })
        .await;
        smoke_step(&mut run, "app_icon", async { smoke_app_icon() }).await;
        smoke_step(&mut run, "native_reorder", async {
            smoke_native_reorder(&view, window, cx)
        })
        .await;
        smoke_step(&mut run, "plugins", smoke_plugins(&view, window, cx)).await;
        smoke_step(&mut run, "tui", smoke_tui(&view, window, cx)).await;
        smoke_step(&mut run, "show_stats", async {
            let toggled = cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| view.set_show_stats(true, window, cx))
            });
            if let Err(err) = toggled {
                smoke_fail(&format!("show_stats toggle: {err:#}"));
            }
        })
        .await;
        smoke_step(&mut run, "fps", async {
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
                    smoke_fail(&format!("only {frames} frames with show_stats on"));
                }
            }
        })
        .await;
        smoke_step(&mut run, "stats", smoke_process_stats(&view, cx)).await;
        smoke_step(&mut run, "preview", async {
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
        })
        .await;
        smoke_step(&mut run, "preview_cadence", async {
            // show_stats is still on, so a frame paints every tick. Readbacks must stay at
            // one per refresh period anyway.
            let before = cx.update(|cx| view.read(cx).snapshot_counts(cx));
            cx.background_executor().timer(SMOKE_SNAPSHOT_WINDOW).await;
            let (after, previews) = cx.update(|cx| {
                let view = view.read(cx);
                (view.snapshot_counts(cx), view.terminal_previews(cx))
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
                let visible = cx.update(|cx| window_visible(window, cx));
                smoke_fail(&format!(
                    "preview refresh outside 1..=3 snapshots: {line} window_visible={visible:?}"
                ));
            }
        })
        .await;
        smoke_step(&mut run, "grid", async {
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
        })
        .await;
        smoke_step(&mut run, "preview_input_targets", async {
            // Input divs over a preview would steal its scroll and keys from the canvas.
            let targets = cx.update(|cx| view.read(cx).preview_input_targets);
            println!("smoke: preview_input_targets={targets}");
            if targets != 0 {
                smoke_fail(&format!("{targets} terminal input divs over previews"));
            }
        })
        .await;
        smoke_step(&mut run, "live", async {
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
                    let visible = cx.update(|cx| window_visible(window, cx));
                    smoke_fail(&format!(
                        "terminals not live at zoom 1.5: {previews:?} window_visible={visible:?}"
                    ));
                }
            }
        })
        .await;
        smoke_step(&mut run, "background", async {
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
        })
        .await;
        smoke_step(&mut run, "browser", async {
            // The browser loaded its page, followed the link and reported the new URL.
            let started = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let status = cx.update(|cx| view.read(cx).browser_status());
                let [(mode, loading, page, scene)] = status.as_slice() else {
                    smoke_fail(&format!("expected 1 browser, found {}", status.len()));
                };
                let target = Some(SMOKE_BROWSER_TARGET.to_string());
                let done = !loading && *page == target && *scene == target;
                if done && *mode == PanelMode::Live {
                    println!(
                        "smoke: browser loaded=true link=followed url={SMOKE_BROWSER_TARGET} mode=live"
                    );
                    break;
                }
                if started.elapsed() > SMOKE_BROWSER_TIMEOUT {
                    smoke_fail(&format!(
                        "browser: mode={mode:?} loading={loading} page={page:?} scene={scene:?}"
                    ));
                }
            }
        })
        .await;
        smoke_step(&mut run, "resize", smoke_resize(&view, window, cx)).await;
        smoke_step(&mut run, "blur", async { smoke_blur(&view, window, cx) }).await;
        smoke_step(
            &mut run,
            "settings_dispatch",
            smoke_settings_dispatch(&view, window, cx),
        )
        .await;
        smoke_step(&mut run, "github_api", smoke_github_api(&view, window, cx)).await;
        smoke_step(&mut run, "plugin_menus", smoke_plugin_menus(&view, window, cx)).await;
        smoke_step(&mut run, "panel_overrides", async {
            smoke_panel_overrides(&view, window, cx)
        })
        .await;
        smoke_step(
            &mut run,
            "panel_settings",
            smoke_panel_settings(&view, window, cx),
        )
        .await;
        // Last: run early, it was followed by a plugins z-order mismatch (backlog item 6).
        smoke_step(&mut run, "session", async {
            smoke_autosave(&view, window, cx).await;
            smoke_session(&view, window, cx).await;
            smoke_session_edges(&view, window, cx);
        })
        .await;

        for line in run.summary() {
            println!("{line}");
        }
        let code = run.exit_code();
        remove_smoke_dir();
        if code != 0 {
            std::process::exit(code);
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

const SMOKE_TUI_TIMEOUT: Duration = Duration::from_secs(10);
/// Prints a marker, proves it ran by writing `$1`, exits 0.
const SMOKE_TUI_SCRIPT: &str =
    "#!/bin/sh\nprintf 'calm-smoke-tui-marker\\n'\nprintf ran > \"$1\"\nexit 0\n";

/// Two TUI fixtures: a script that runs and exits 0 (restart "prompt", run directly), and
/// a missing command through the login shell (restart "on-exit", which must not loop).
/// Opens the shipped top-example and checks its manifest look reaches the chrome model:
/// the focused ring takes its accent and the icon slot gets its decoded image.
fn smoke_tui_chrome(view: &Entity<CanvasView>, window: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    use calm_core::chrome::{ChromeState, chrome_colors};
    let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let discovery = calm_plugins::discover(&[plugins]);
    let Some(top) = discovery
        .plugins
        .into_iter()
        .find(|plugin| plugin.manifest.id == "top-example")
    else {
        smoke_fail(&format!(
            "top-example not discovered: {:?}",
            discovery.errors
        ));
    };
    let Some(expected) = top.manifest.tui().and_then(|spec| spec.accent_color) else {
        smoke_fail("top-example has no accentColor");
    };
    let checked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.install_plugins(vec![top]);
            let kind = PanelKind::Tui {
                plugin_id: "top-example".to_string(),
                plugin_version: None,
            };
            let id = view.scene.add(kind, Rect::new(720.0, 460.0, 300.0, 160.0));
            view.open_tui_panel(id, "top-example", window, cx)?;
            let look = view.panel_look(id);
            let ring = chrome_colors(
                ChromeState::Focused,
                &look.overrides,
                &crate::canvas_view::chrome_palette(),
            )
            .border;
            let image = matches!(look.icon, Some(crate::canvas_view::IconLook::Image(_)));
            let title = view.scene.get(id).map(|panel| panel.title.clone());
            // `top` is still running: close as if the user confirmed.
            view.apply_effects(
                vec![calm_core::interaction::Effect::ClosePanel(id)],
                window,
                cx,
            );
            Ok::<_, String>((ring, image, title, view.tui_status().len()))
        })
    });
    match checked {
        Ok(Ok((ring, true, Some(title), 0))) if ring == expected => println!(
            "smoke: tui chrome top-example accent=#{expected:08x} ring=#{ring:08x} icon=image title={title:?} closed=ok"
        ),
        other => smoke_fail(&format!(
            "top-example chrome: expected ring #{expected:08x} and an image icon, got {other:?}"
        )),
    }
}

/// A plain terminal from a fixture profile, chosen the way the Add menu's profile item
/// chooses it. Checks the chrome model takes the profile's accent and that the terminal
/// started with a custom Ghostty theme, then swaps the palette on the live surface.
fn smoke_terminal_profile(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use calm_core::chrome::{ChromeState, chrome_colors};
    use calm_core::terminal_profile::{
        PALETTE_PRESETS, TerminalColorsSetting, TerminalProfile, TerminalSettings,
    };
    const ACCENT: u32 = 0xff4fd8ff;
    let profile = |preset: usize| TerminalSettings {
        profiles: vec![TerminalProfile {
            id: "neon".to_string(),
            name: "Neon".to_string(),
            fields: calm_core::terminal_profile::TerminalFields {
                title_template: Some("neon {n}".to_string()),
                icon: Some("N".to_string()),
                accent_color: Some("#ff4fd8".to_string()),
                use_ghostty_user_config: Some(false),
                terminal_colors: Some(TerminalColorsSetting::from_colors(
                    &PALETTE_PRESETS[preset].colors,
                )),
                ..Default::default()
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    let checked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let fixture = profile(0);
            fixture.validate()?;
            view.settings.settings.terminal = fixture;
            let chosen = calm_core::menu::terminal_profile_choice("add.terminal_profile.neon")
                .map(str::to_string);
            let kind = PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
            };
            let id = view.scene.add(kind, Rect::new(720.0, 660.0, 300.0, 160.0));
            view.open_plain_terminal(id, chosen, None, None, window, cx)?;
            let look = view.panel_look(id);
            let ring = chrome_colors(
                ChromeState::Focused,
                &look.overrides,
                &crate::canvas_view::chrome_palette(),
            )
            .border;
            let meta = view.terminal_meta(id).cloned().ok_or("no terminal meta")?;
            let title = view.scene.get(id).map(|panel| panel.title.clone());
            // Live recolor: swap in another palette and let the settings hook apply it.
            view.settings.settings.terminal = profile(1);
            view.terminal_settings_changed(cx);
            let recolored = view.terminal_meta(id).and_then(|meta| meta.applied_colors)
                == Some(PALETTE_PRESETS[1].colors);
            view.settings.settings.terminal = TerminalSettings::default();
            view.apply_effects(
                vec![calm_core::interaction::Effect::ClosePanel(id)],
                window,
                cx,
            );
            Ok::<_, String>((meta, ring, title, recolored))
        })
    });
    match checked {
        Ok(Ok((meta, ring, Some(title), true)))
            if ring == ACCENT
                && meta.configuration == "custom"
                && meta.profile.as_deref() == Some("neon")
                && title.starts_with("neon ") =>
        {
            println!(
                "smoke: terminal profile=neon accent=#{ring:08x} theme={} title={title:?} recolor=ok",
                meta.configuration
            )
        }
        other => smoke_fail(&format!(
            "terminal profile: expected ring #{ACCENT:08x}, theme custom and a recolor, got {other:?}"
        )),
    }
}

async fn smoke_tui(view: &Entity<CanvasView>, window: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let fixtures = match tempfile::Builder::new().prefix("calm-smoke-tui-").tempdir() {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("tui temp dir: {err}")),
    };
    let root = fixtures.path();
    // A space in the path checks that argv survives Ghostty's shell.
    let marker = root.join("ran marker.txt");
    let script = root.join("smoke-tui").join("run.sh");
    let manifests = [
        (
            "smoke-tui",
            serde_json::json!({
                "kind": "tui", "id": "smoke-tui", "name": "Smoke TUI", "icon": "S",
                "command": script, "args": [marker], "shell": false, "restart": "prompt"
            }),
        ),
        (
            "smoke-tui-missing",
            serde_json::json!({
                "kind": "tui", "id": "smoke-tui-missing", "name": "Missing",
                "command": "calm-smoke-no-such-command", "restart": "on-exit"
            }),
        ),
    ];
    let written = manifests.iter().try_for_each(|(dir, manifest)| {
        std::fs::create_dir_all(root.join(dir))?;
        std::fs::write(
            root.join(dir).join("calm-plugin.json"),
            manifest.to_string(),
        )
    });
    let written = written.and_then(|()| {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(&script, SMOKE_TUI_SCRIPT)?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
    });
    if let Err(err) = written {
        smoke_fail(&format!("tui fixtures: {err}"));
    }
    let discovery = calm_plugins::discover(&[root.to_path_buf()]);
    if discovery.plugins.len() != 2 || !discovery.errors.is_empty() {
        smoke_fail(&format!(
            "tui fixtures not discovered: {:?}",
            discovery.errors
        ));
    }
    let opened = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.install_plugins(discovery.plugins);
            let mut ids = Vec::new();
            for (plugin_id, rect) in [
                ("smoke-tui", Rect::new(720.0, 60.0, 300.0, 160.0)),
                ("smoke-tui-missing", Rect::new(720.0, 260.0, 300.0, 160.0)),
            ] {
                let kind = PanelKind::Tui {
                    plugin_id: plugin_id.to_string(),
                    plugin_version: None,
                };
                let id = view.scene.add(kind, rect);
                view.open_tui_panel(id, plugin_id, window, cx)?;
                ids.push(id);
            }
            cx.notify();
            Ok::<_, String>(ids)
        })
    });
    let ids = match opened {
        Ok(Ok(ids)) => ids,
        Ok(Err(err)) => smoke_fail(&format!("tui panel: {err}")),
        Err(err) => smoke_fail(&format!("tui panel: {err:#}")),
    };

    let wait_for = |launches: u64| {
        let view = view.clone();
        let marker = marker.clone();
        async move |cx: &mut gpui::AsyncApp| {
            let started = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let status = cx.update(|cx| view.read(cx).tui_status());
                let ran = std::fs::read_to_string(&marker).is_ok_and(|text| text == "ran");
                let done = match status.as_slice() {
                    [
                        (ok_title, TuiState::Exited(Some(0)), true, ok_launches),
                        (missing_title, TuiState::NotFound, true, 1),
                    ] => {
                        *ok_launches == launches
                            && ok_title == "Smoke TUI: exited, code 0"
                            && missing_title
                                == "Missing: command not found: calm-smoke-no-such-command"
                    }
                    _ => false,
                };
                if done && ran {
                    return;
                }
                if started.elapsed() > SMOKE_TUI_TIMEOUT {
                    smoke_fail(&format!("tui: ran={ran} status={status:?}"));
                }
            }
        }
    };
    wait_for(1)(cx).await;

    // Restart from the exited state, as the menu and Enter do; the script runs again.
    if let Err(err) = std::fs::remove_file(&marker) {
        smoke_fail(&format!("tui marker: {err}"));
    }
    let restarted = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_menu_choice(
                calm_core::menu::PANEL_SEND_TO_BACK,
                canvas_view::MenuTarget::Panel(ids[0]),
                window,
                cx,
            );
            let before = view.native_zorder(cx);
            view.restart_tui(ids[0], true, window, cx);
            (before, view.native_zorder(cx), view.status_bar_state())
        })
    });
    let (before, after, bar) = match restarted {
        Ok(orders) => orders,
        Err(err) => smoke_fail(&format!("tui restart: {err:#}")),
    };
    if before.0 != before.1 || after.0 != after.1 {
        smoke_fail(&format!(
            "tui restart native order: before={before:?} after={after:?}"
        ));
    }
    if !matches!(bar, Ok((true, true, _))) {
        smoke_fail(&format!("tui restart status bar: {bar:?}"));
    }
    wait_for(2)(cx).await;

    // Both processes exited, so close skips the confirmation. Later steps count panels.
    let closed = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let effects = ids
                .iter()
                .map(|id| calm_core::interaction::Effect::ClosePanelRequested(*id))
                .collect();
            view.apply_effects(effects, window, cx);
            view.tui_status().len()
        })
    });
    match closed {
        Ok(0) => {}
        other => smoke_fail(&format!(
            "exited tui panels did not close without asking: {other:?}"
        )),
    }
    println!("smoke: tui ran=ok exit=0 missing=reported restarted=ok closed=ok");
    smoke_tui_chrome(view, window, cx);
    smoke_terminal_profile(view, window, cx);
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the tui temp dir: {err}");
    }
}

/// Drags the browser's bottom-right handle just outside its web view, and every edge of a
/// fixture plugin with `"resizable": false`, through the real mouse handlers. The browser
/// grows and its web view follows; the locked plugin keeps its rect.
/// Focuses a terminal, then the browser's page, and clicks empty canvas through the real
/// mouse handlers each time. Focus must drop in the scene and in AppKit.
fn smoke_blur(view: &Entity<CanvasView>, window: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let outcome = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let empty = view
                .smoke_empty_point()
                .ok_or("no empty canvas on screen")?;
            let terminal = view
                .scene
                .panels
                .iter()
                .find(|panel| matches!(panel.kind, PanelKind::Terminal { .. }))
                .map(|panel| panel.id)
                .ok_or("no terminal panel")?;
            let browser = view
                .browser_page_sizes()
                .first()
                .map(|(id, _)| *id)
                .ok_or("no browser panel")?;
            let click =
                |view: &mut CanvasView, window: &mut Window, cx: &mut Context<CanvasView>| {
                    view.smoke_click(empty, gpui::MouseButton::Left, window, cx);
                    let responder = calm_native::host_view_is_first_responder(window);
                    let canvas_keys = view.focus_handle(cx).is_focused(window);
                    (view.focused(), responder, canvas_keys)
                };

            view.apply_effects(vec![Effect::FocusPanel(terminal)], window, cx);
            if view.focus_handle(cx).is_focused(window) || view.focused() != Some(terminal) {
                return Err("terminal did not take focus".to_string());
            }
            let after_terminal = click(view, window, cx);
            if after_terminal != (None, Some(true), true) {
                return Err(format!("after terminal: {after_terminal:?}"));
            }

            view.apply_effects(vec![Effect::FocusPanel(browser)], window, cx);
            // Positive control: the web view really holds the keyboard before the click.
            let before = calm_native::host_view_is_first_responder(window);
            if view.focused() != Some(browser) || before != Some(false) {
                return Err(format!(
                    "browser focus: focused={:?} host_responder={before:?}",
                    view.focused()
                ));
            }
            let after_browser = click(view, window, cx);
            if after_browser != (None, Some(true), true) {
                return Err(format!("after browser: {after_browser:?}"));
            }
            Ok(())
        })
    });
    match outcome {
        Ok(Ok(())) => println!("smoke: blur focused=none responder=canvas"),
        Ok(Err(err)) => smoke_fail(&format!("blur: {err}")),
        Err(err) => smoke_fail(&format!("blur: window gone: {err}")),
    }
}

async fn smoke_resize(view: &Entity<CanvasView>, window: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let fixtures = match tempfile::Builder::new()
        .prefix("calm-smoke-resize-")
        .tempdir()
    {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("resize temp dir: {err}")),
    };
    let locked_html = "<!doctype html><body>locked</body>";
    if let Err(err) = write_smoke_plugin(
        fixtures.path(),
        "smoke-locked",
        locked_html,
        r#","resizable":false"#,
    ) {
        smoke_fail(&format!("plugin fixture smoke-locked: {err}"));
    }
    let discovery = calm_plugins::discover(&[fixtures.path().to_path_buf()]);
    if discovery.plugins.len() != 1 || !discovery.errors.is_empty() {
        smoke_fail(&format!(
            "locked fixture not discovered: {:?}",
            discovery.errors
        ));
    }
    let delta = Point::new(80.0, 60.0);
    let started = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            // Zoom 1 at the origin: screen pixels equal world units, all panels on screen.
            view.scene.camera = calm_core::camera::Camera::default();
            view.install_plugins(discovery.plugins);
            let kind = PanelKind::Plugin {
                plugin_id: "smoke-locked".to_string(),
                plugin_version: None,
                settings: serde_json::json!({}),
            };
            let locked = view.scene.add(kind, Rect::new(20.0, 460.0, 300.0, 120.0));
            view.open_plugin_panel(locked, "smoke-locked", window, cx)?;
            let browser = view
                .scene
                .panels
                .iter()
                .find(|panel| matches!(panel.kind, PanelKind::Browser { .. }))
                .map(|panel| (panel.id, panel.rect))
                .ok_or("no browser panel")?;
            let locked_rect = view.scene.get(locked).map(|panel| panel.rect);
            let locked_rect = locked_rect.ok_or("locked panel missing")?;

            // Just outside the browser's corner: pixels the web view does not cover.
            let grab = Point::new(browser.1.right() + 3.0, browser.1.bottom() + 3.0);
            let cursor = view.resize_cursor_at(grab);
            if cursor != Some(ResizeEdge::BottomRight) {
                return Err(format!("browser corner shows cursor {cursor:?}"));
            }
            view.smoke_drag(grab, delta, window, cx);

            let right = locked_rect.right();
            let bottom = locked_rect.bottom();
            let middle_y = locked_rect.origin.y + locked_rect.size.height / 2.0;
            for press in [
                Point::new(right - 4.0, middle_y),
                Point::new(right + 3.0, middle_y),
                Point::new(right - 4.0, bottom - 4.0),
                Point::new(right + 3.0, bottom + 3.0),
            ] {
                if let Some(edge) = view.resize_cursor_at(press) {
                    return Err(format!("locked plugin shows {edge:?} cursor at {press:?}"));
                }
                view.smoke_drag(press, delta, window, cx);
            }
            cx.notify();
            Ok::<_, String>((browser.0, browser.1, locked, locked_rect))
        })
    });
    let (browser, before, locked, locked_before) = match started {
        Ok(Ok(ids)) => ids,
        Ok(Err(err)) => smoke_fail(&format!("resize: {err}")),
        Err(err) => smoke_fail(&format!("resize: {err:#}")),
    };
    let want = Size::new(before.size.width + delta.x, before.size.height + delta.y);
    let started = Instant::now();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let (rect, page, locked_rect) = cx.update(|cx| {
            let view = view.read(cx);
            let rect = view
                .scene
                .get(browser)
                .map(|panel| (panel.rect, calm_core::scene::content_rect(panel).size));
            let page = view
                .browser_page_sizes()
                .into_iter()
                .find(|(id, _)| *id == browser)
                .map(|(_, size)| size);
            (rect, page, view.scene.get(locked).map(|panel| panel.rect))
        });
        let Some((rect, content)) = rect else {
            smoke_fail("browser left the scene during resize");
        };
        if rect.size != want {
            smoke_fail(&format!("browser rect {rect:?}, want size {want:?}"));
        }
        if locked_rect != Some(locked_before) {
            smoke_fail(&format!(
                "locked plugin moved: {locked_rect:?} from {locked_before:?}"
            ));
        }
        // The web view fills the content below the address bar.
        let want_page = Size::new(
            content.width,
            content.height - calm_native::BROWSER_BAR_HEIGHT,
        );
        if page == Some(want_page) {
            println!(
                "smoke: resize browser=ok plugin_locked=ok browser={}x{} page={}x{}",
                rect.size.width, rect.size.height, want_page.width, want_page.height
            );
            break;
        }
        if started.elapsed() > SMOKE_FRAME_TIMEOUT {
            smoke_fail(&format!(
                "browser web view did not follow the resize: page={page:?} want={want_page:?}"
            ));
        }
    }
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the resize temp dir: {err}");
    }
}

/// Whether AppKit has the window on screen. GPUI stops painting an occluded window, so a
/// failed frame check with `Some(false)` points at the desktop, not the code.
fn window_visible(window: AnyWindowHandle, cx: &mut App) -> Option<bool> {
    cx.update_window(window, |_, window, _| calm_native::window_visible(window))
        .ok()
        .flatten()
}

/// A fixture plugin whose page sets the title twice; the second call runs only after the
/// host answered the first, so the final title proves load, prelude and bridge round trip.
/// It first probes error codes, `calm.has` and the read-only calls through the real bridge
/// and stores what it saw with `settings.set`, so the smoke reads it once the title says
/// `smoke-ok`.
const SMOKE_PLUGIN_HTML: &str = r#"<!doctype html><body>smoke<script>
function codeOf(promise) {
  return promise.then(function () { return "resolved"; }, function (err) { return err && err.code; });
}
function valueOf(promise) {
  return promise.then(function (v) { return v; }, function (err) { return "rejected " + (err && err.code); });
}
// The prelude has no generic call, so rename the next posted method to an unknown one.
var realStringify = JSON.stringify;
JSON.stringify = function (value) {
  JSON.stringify = realStringify;
  return realStringify.call(JSON, { id: value.id, method: "nope", params: null });
};
var unknown = codeOf(calm.theme.get());
var badParam = codeOf(calm.canvas.focusPanel(-1));
var reads = Promise.all([
  valueOf(calm.panel.getBounds()),
  valueOf(calm.theme.get()),
  valueOf(calm.canvas.listPanels()),
  valueOf(calm.process.exec("/bin/echo", ["smoke", "exec"])),
]);
Promise.all([unknown, badParam, reads]).then(function (got) {
  var r = got[2];
  var mine = Array.isArray(r[2]) ? r[2].filter(function (p) { return p.pluginId === "smoke-plugin"; }) : [];
  var errors = { unknown: got[0], badParam: got[1],
    has: [calm.has("panel.setTitle"), calm.has("nope"), calm.has("canvas.createPanel")],
    bounds: typeof r[0].zoom === "number" && r[0].width > 0,
    theme: /^#[0-9a-f]{6}$/.test(r[1].background),
    listed: mine.length === 1 && mine[0].kind === "plugin",
    exec: r[3].code === 0 && r[3].stdout === "smoke exec\n" };
  calm.log("smoke errors " + JSON.stringify(errors));
  return calm.settings.set({ errors: errors });
}).then(function () { return calm.panel.setTitle("smoke-1"); })
  .then(function () { return calm.panel.setTitle("smoke-ok"); });
</script></body>"#;
/// A plugin whose bundle was never built: the panel must say so.
const SMOKE_UNBUILT_HTML: &str =
    r#"<!doctype html><body><script src="dist/index.js"></script></body>"#;

/// `extra` is appended to the manifest object, e.g. `,"resizable":false`.
fn write_smoke_plugin(
    root: &std::path::Path,
    id: &str,
    html: &str,
    extra: &str,
) -> std::io::Result<()> {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("index.html"), html)?;
    let manifest = format!(
        r#"{{"id":"{id}","name":"{id}","version":"0.1.0","entry":"index.html","defaultSize":{{"width":300,"height":160}}{extra}}}"#
    );
    std::fs::write(dir.join("calm-plugin.json"), manifest)
}

/// Checks the startup scan: the workspace's first-party plugins are in the Add menu,
/// unless built-ins were turned off.
fn smoke_startup_plugins(view: &Entity<CanvasView>, cx: &mut gpui::AsyncApp) {
    let (counts, items) = cx.update(|cx| {
        let view = view.read(cx);
        (view.plugin_counts, view.canvas_menu())
    });
    let (builtin, user) = counts;
    println!("smoke: plugins builtin={builtin} user={user}");
    let mut ids = Vec::new();
    let mut stack = items;
    while let Some(item) = stack.pop() {
        match item {
            calm_core::menu::MenuItem::Submenu { items, .. } => stack.extend(items),
            calm_core::menu::MenuItem::Action { id, .. } => ids.push(id),
            _ => {}
        }
    }
    let builtin_off =
        std::env::var_os(calm_plugins::NO_BUILTIN_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    if builtin_off {
        if builtin != 0 {
            smoke_fail(&format!("built-ins off but {builtin} loaded"));
        }
        return;
    }
    for plugin_id in ["hello-world", "github-prs"] {
        let menu_id = format!("{}{plugin_id}", calm_core::menu::ADD_PLUGIN_PREFIX);
        if !ids.contains(&menu_id) {
            smoke_fail(&format!(
                "workspace plugin {plugin_id} not in the Add menu: {ids:?}"
            ));
        }
    }
}

/// Adds a working and an unbuilt plugin panel from temp fixtures, waits for the working one
/// to finish its bridge calls, and checks the shared native stack still follows the scene.
async fn smoke_plugins(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    smoke_startup_plugins(view, cx);
    let fixtures = match tempfile::Builder::new()
        .prefix("calm-smoke-plugins-")
        .tempdir()
    {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("plugin temp dir: {err}")),
    };
    for (id, html) in [
        ("smoke-plugin", SMOKE_PLUGIN_HTML),
        ("smoke-unbuilt", SMOKE_UNBUILT_HTML),
    ] {
        if let Err(err) = write_smoke_plugin(fixtures.path(), id, html, "") {
            smoke_fail(&format!("plugin fixture {id}: {err}"));
        }
    }
    let discovery = calm_plugins::discover(&[fixtures.path().to_path_buf()]);
    if discovery.plugins.len() != 2 || !discovery.errors.is_empty() {
        smoke_fail(&format!(
            "plugin fixtures not discovered: {:?}",
            discovery.errors
        ));
    }
    let opened = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.install_plugins(discovery.plugins);
            let mut ids = Vec::new();
            for (plugin_id, rect) in [
                ("smoke-plugin", Rect::new(20.0, 240.0, 300.0, 160.0)),
                ("smoke-unbuilt", Rect::new(380.0, 320.0, 300.0, 120.0)),
            ] {
                let kind = PanelKind::Plugin {
                    plugin_id: plugin_id.to_string(),
                    plugin_version: None,
                    settings: serde_json::json!({}),
                };
                let id = view.scene.add(kind, rect);
                view.open_plugin_panel(id, plugin_id, window, cx)?;
                ids.push(id);
            }
            cx.notify();
            Ok::<_, String>(ids)
        })
    });
    let ids = match opened {
        Ok(Ok(ids)) => ids,
        Ok(Err(err)) => smoke_fail(&format!("plugin panel: {err}")),
        Err(err) => smoke_fail(&format!("plugin panel: {err:#}")),
    };

    let started = Instant::now();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let (title, status, settings) = cx.update(|cx| {
            let view = view.read(cx);
            let panel = view.scene.get(ids[0]);
            let title = panel.map(|panel| panel.title.clone());
            let settings = match panel.map(|panel| &panel.kind) {
                Some(PanelKind::Plugin { settings, .. }) => settings.clone(),
                _ => serde_json::Value::Null,
            };
            (title, view.plugin_status(), settings)
        });
        if title.as_deref() == Some("smoke-ok") {
            println!("smoke: plugin loaded=true bridge=ok title=smoke-ok");
            smoke_plugin_errors(&settings["errors"]);
            match &status[1] {
                (_, Some(err)) if err.contains("not built") => {
                    println!("smoke: plugin unbuilt reported=true")
                }
                other => smoke_fail(&format!("unbuilt plugin not reported: {other:?}")),
            }
            break;
        }
        if started.elapsed() > SMOKE_FRAME_TIMEOUT {
            smoke_fail(&format!(
                "plugin bridge did not answer within 10 s: title={title:?} status={status:?}"
            ));
        }
    }

    // Raise the backmost terminal over the plugins through the panel menu action.
    let orders = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            if let Some(first) = view.scene.panels.first().map(|panel| panel.id) {
                view.apply_menu_choice(
                    calm_core::menu::PANEL_BRING_TO_FRONT,
                    canvas_view::MenuTarget::Panel(first),
                    window,
                    cx,
                );
            }
            view.native_zorder(cx)
        })
    });
    let (scene, native) = match orders {
        Ok(orders) => orders,
        Err(err) => smoke_fail(&format!("plugin native reorder window: {err:#}")),
    };
    let line = format!("zorder={native:?} scene={scene:?}");
    if scene != native || scene.len() != 5 {
        smoke_fail(&format!("native order with plugins does not match: {line}"));
    }
    println!("smoke: plugins {line}");
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the plugin temp dir: {err}");
    }
}

/// What the fixture page saw: the codes of an unknown call and a bad param, `calm.has`, and
/// whether `getBounds`, `theme.get`, `listPanels` and `process.exec` answered as expected.
fn smoke_plugin_errors(errors: &serde_json::Value) {
    let text = |key: &str| errors[key].as_str().unwrap_or("missing").to_string();
    let (unknown, bad_param) = (text("unknown"), text("badParam"));
    let has = errors["has"].to_string();
    println!("smoke: plugin errors unknown={unknown} bad_param={bad_param} has={has}");
    if unknown != "unknown_method" || bad_param != "invalid_params" || has != "[true,false,true]" {
        smoke_fail(&format!(
            "plugin error codes or calm.has are wrong: {errors}"
        ));
    }
    let reads: Vec<String> = ["bounds", "theme", "listed", "exec"]
        .iter()
        .map(|key| format!("{key}={}", errors[*key]))
        .collect();
    println!("smoke: plugin reads {}", reads.join(" "));
    if reads.iter().any(|read| !read.ends_with("=true")) {
        smoke_fail(&format!("plugin read calls answered wrong: {errors}"));
    }
}

/// A fixture plugin with one canvas and one panel item. Its page registers the command
/// handler late, so a command that arrives first must wait in the prelude.
const SMOKE_MENUS_HTML: &str = r#"<!doctype html><body>menus<script>
setTimeout(function () {
  calm.events.on("command", function (c) {
    calm.panel.setTitle("cmd:" + c.id + ":" + c.location + ":" +
      (c.panel ? c.panel.id : "none") + ":" + (c.world ? "world" : "noworld"));
  });
}, 300);
</script></body>"#;
const SMOKE_MENUS_CONTRIBUTES: &str = r#","contributes":{"menus":[
  {"id":"ping","title":"Smoke ping","location":"canvas"},
  {"id":"pong","title":"Smoke pong","location":"panel","when":{"plugin":["smoke-menus"]}}
]}"#;

fn menu_ids(items: Vec<calm_core::menu::MenuItem>) -> Vec<String> {
    let mut ids = Vec::new();
    let mut stack = items;
    while let Some(item) = stack.pop() {
        match item {
            calm_core::menu::MenuItem::Submenu { items, .. } => stack.extend(items),
            calm_core::menu::MenuItem::Action { id, .. } => ids.push(id),
            _ => {}
        }
    }
    ids
}

/// Waits until panel `id` has `title`, which the fixture page sets from its command handler.
async fn wait_for_title(
    view: &Entity<CanvasView>,
    id: calm_core::scene::PanelId,
    title: &str,
    cx: &mut gpui::AsyncApp,
) {
    let started = Instant::now();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let now = cx.update(|cx| view.read(cx).scene.get(id).map(|panel| panel.title.clone()));
        if now.as_deref() == Some(title) {
            return;
        }
        if started.elapsed() > SMOKE_FRAME_TIMEOUT {
            smoke_fail(&format!("plugin menus: want title {title:?}, got {now:?}"));
        }
    }
}

/// Plugin menu items: both fixture items are in the built menus, the panel item only for
/// the plugin's own panels. The canvas item runs through the real menu choice path with no
/// panel open, so CALM opens one at the click point and delivers after load; the panel
/// item then reuses that panel.
/// Opens a plain terminal, gives it a literal title, an accent, a palette and borderless,
/// then reads back the look and the content rect paint and placement use. The content (the
/// terminal's logical size) must not move and the palette is applied live. Clearing the
/// overrides brings the bar and the rendered title back.
fn smoke_panel_overrides(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use calm_core::scene::{chrome_height, content_rect, set_borderless};
    use calm_core::terminal_profile::{PALETTE_PRESETS, TerminalColorsSetting};
    const TITLE: &str = "smoke {n} literal";
    const ACCENT: u32 = 0x12ab34ff;
    let palette = PALETTE_PRESETS[1].colors;
    let checked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let kind = PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
            };
            let id = view.scene.add(kind, Rect::new(720.0, 860.0, 300.0, 160.0));
            view.open_plain_terminal(id, None, None, None, window, cx)?;
            let before = view.scene.get(id).map(content_rect).ok_or("no panel")?;
            if let Some(panel) = view.scene.get_mut(id) {
                panel.overrides.title = Some(TITLE.to_string());
                panel.overrides.accent_color = Some("#12ab34".to_string());
                panel.overrides.terminal_colors =
                    Some(TerminalColorsSetting::from_colors(&palette));
                set_borderless(panel, true);
            }
            view.apply_panel_overrides(id, cx);
            let ids: Vec<PanelId> = view.scene.panels.iter().map(|panel| panel.id).collect();
            let styled = ids
                .iter()
                .filter(|id| {
                    let look = view.panel_look(**id);
                    look.title.as_deref() == Some(TITLE) && look.overrides.accent == Some(ACCENT)
                })
                .count();
            let borderless = view
                .scene
                .panels
                .iter()
                .filter(|panel| chrome_height(panel) == 0.0 && content_rect(panel) == panel.rect)
                .count();
            let after = view.scene.get(id).map(content_rect).ok_or("no panel")?;
            let scene_title = view.scene.get(id).map(|panel| panel.title.clone());
            let recolored =
                view.terminal_meta(id).and_then(|meta| meta.applied_colors) == Some(palette);
            if let Some(panel) = view.scene.get_mut(id) {
                set_borderless(panel, false);
                panel.overrides = Default::default();
            }
            view.apply_panel_overrides(id, cx);
            let cleared = view.scene.get(id).map(content_rect) == Some(before)
                && view.scene.get(id).map(chrome_height) == Some(calm_core::scene::CHROME_HEIGHT)
                && view.panel_look(id).title.is_none()
                && view.scene.get(id).is_some_and(|panel| panel.title != TITLE);
            // The shell is still running: close as if the user confirmed.
            view.apply_effects(
                vec![calm_core::interaction::Effect::ClosePanel(id)],
                window,
                cx,
            );
            Ok::<_, String>((
                styled,
                borderless,
                before == after,
                scene_title,
                recolored,
                cleared,
            ))
        })
    });
    match checked {
        Ok(Ok((1, 1, true, Some(title), true, true))) if title == TITLE => println!(
            "smoke: panel_overrides styled=1 borderless=1 size=unchanged title={title:?} recolor=ok cleared=ok"
        ),
        other => smoke_fail(&format!(
            "panel_overrides: expected styled=1 borderless=1, size unchanged, literal title, recolor, cleared, got {other:?}"
        )),
    }
}

/// Opens a panel's Settings window twice, renames the panel through it, closes the panel,
/// then sends a late change. One window while open, none after, and the late change is
/// ignored without touching the scene.
async fn smoke_panel_settings(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use calm_core::settings_form::FieldValue;
    const TITLE: &str = "smoke panel settings";
    let title = crate::panel_settings::window_title(TITLE);
    let count = |title: &str| {
        calm_native::settings_window_count(title)
            .unwrap_or_else(|err| smoke_fail(&format!("panel settings count: {err}")))
    };
    let opened = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let kind = PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
            };
            let id = view.scene.add(kind, Rect::new(760.0, 1080.0, 300.0, 160.0));
            view.open_panel_settings(id, window, cx);
            view.open_panel_settings(id, window, cx);
            view.panel_settings_changed(
                id,
                "panel.title",
                FieldValue::Text(TITLE.to_string()),
                window,
                cx,
            );
            let renamed = view.scene.get(id).and_then(|p| p.overrides.title.clone());
            let open = count(&title);
            let mapped = view.panel_settings_windows.len();
            view.apply_effects(
                vec![calm_core::interaction::Effect::ClosePanel(id)],
                window,
                cx,
            );
            (id, renamed, open, mapped)
        })
    });
    let Ok((id, renamed, open, mapped)) = opened else {
        smoke_fail("panel_settings: canvas window unavailable");
    };
    // Let AppKit finish closing before counting.
    cx.background_executor()
        .timer(Duration::from_millis(200))
        .await;
    let late = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let before = view.scene.revision();
            view.panel_settings_changed(
                id,
                "panel.title",
                FieldValue::Text("late".into()),
                window,
                cx,
            );
            (
                view.scene.revision() == before && view.scene.get(id).is_none(),
                view.panel_settings_windows.len(),
            )
        })
    });
    let after = count(&title);
    println!(
        "smoke: panel_settings windows_open={open} after_close={after} renamed={} late_ignored={}",
        renamed.as_deref() == Some(TITLE),
        matches!(late, Ok((true, 0)))
    );
    if open != 1 || mapped != 1 || after != 0 || renamed.as_deref() != Some(TITLE) {
        smoke_fail(&format!(
            "panel_settings: expected windows_open=1 after_close=0, got open={open} mapped={mapped} after={after} renamed={renamed:?}"
        ));
    }
    if !matches!(late, Ok((true, 0))) {
        smoke_fail(&format!(
            "panel_settings: late change was not ignored: {late:?}"
        ));
    }
}

async fn smoke_plugin_menus(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use calm_core::plugin_menu::item_menu_id;
    let fixtures = match tempfile::Builder::new()
        .prefix("calm-smoke-menus-")
        .tempdir()
    {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("plugin menus temp dir: {err}")),
    };
    let id = "smoke-menus";
    if let Err(err) = write_smoke_plugin(
        fixtures.path(),
        id,
        SMOKE_MENUS_HTML,
        SMOKE_MENUS_CONTRIBUTES,
    ) {
        smoke_fail(&format!("plugin fixture {id}: {err}"));
    }
    let discovery = calm_plugins::discover(&[fixtures.path().to_path_buf()]);
    if discovery.plugins.len() != 1 || !discovery.errors.is_empty() {
        smoke_fail(&format!(
            "plugin menus fixture not discovered: {:?}",
            discovery.errors
        ));
    }
    let ping = item_menu_id(id, "ping");
    let pong = item_menu_id(id, "pong");
    let invoked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.install_plugins(discovery.plugins);
            // Near the top-left corner, so the new page is on screen and live.
            let world = view.scene.camera.screen_to_world(Point::new(40.0, 40.0));
            let canvas = menu_ids(view.canvas_menu());
            if !canvas.contains(&ping) || canvas.contains(&pong) {
                return Err(format!("canvas menu: {canvas:?}"));
            }
            let other = view.scene.panels.first().map(|panel| panel.id);
            let other_ids = other
                .and_then(|panel| view.panel_menu(panel))
                .unwrap_or_default();
            if menu_ids(other_ids).contains(&pong) {
                return Err("pong shown on a panel of another kind".to_string());
            }
            let before = view.scene.panels.len();
            view.apply_menu_choice(&ping, canvas_view::MenuTarget::Canvas { world }, window, cx);
            let opened: Vec<_> = view
                .scene
                .panels
                .iter()
                .filter(|panel| calm_core::plugin_menu::plugin_of(&panel.kind) == Some(id))
                .map(|panel| (panel.id, panel.rect.origin))
                .collect();
            match opened[..] {
                [(panel, origin)] if view.scene.panels.len() == before + 1 => {
                    // The menu path stamps the installed manifest version on the kind.
                    match view.scene.get(panel).map(|panel| &panel.kind) {
                        Some(PanelKind::Plugin {
                            plugin_version: Some(version),
                            ..
                        }) if version == "0.1.0" => Ok((panel, origin, world)),
                        other => Err(format!("plugin_version not from the manifest: {other:?}")),
                    }
                }
                _ => Err(format!("canvas item opened {opened:?}")),
            }
        })
    });
    let (panel, origin, world) = match invoked {
        Ok(Ok(found)) => found,
        Ok(Err(err)) => smoke_fail(&format!("plugin menus: {err}")),
        Err(err) => smoke_fail(&format!("plugin menus: {err:#}")),
    };
    if (origin.x - world.x).abs() > 8.0 || (origin.y - world.y).abs() > 8.0 {
        smoke_fail(&format!(
            "plugin menus: panel at {origin:?}, clicked {world:?}"
        ));
    }
    wait_for_title(view, panel, "cmd:ping:canvas:none:world", cx).await;

    let invoked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let items = menu_ids(view.panel_menu(panel).unwrap_or_default());
            // The canvas menu held ping (checked above); the panel menu must hold pong.
            let count = 1 + usize::from(items.contains(&pong));
            if !items.contains(&pong) {
                return Err(format!("panel menu: {items:?}"));
            }
            let before = view.scene.panels.len();
            view.apply_menu_choice(&pong, canvas_view::MenuTarget::Panel(panel), window, cx);
            if view.scene.panels.len() != before {
                return Err("panel item opened a panel instead of reusing it".to_string());
            }
            Ok(count)
        })
    });
    let items = match invoked {
        Ok(Ok(count)) => count,
        Ok(Err(err)) => smoke_fail(&format!("plugin menus: {err}")),
        Err(err) => smoke_fail(&format!("plugin menus: {err:#}")),
    };
    wait_for_title(
        view,
        panel,
        &format!("cmd:pong:panel:{}:noworld", panel.0),
        cx,
    )
    .await;
    println!("smoke: plugin menus items={items} invoked=ok");
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the plugin menus temp dir: {err}");
    }
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

/// Calls `github.api` twice and storage once from a fixture page. `gh` is a fake script in
/// a temp dir, so the smoke never reaches the network or the user's token. The PR list
/// answers; any other path fails the way a logged-out `gh` does.
const SMOKE_GH_SCRIPT: &str = r#"#!/bin/sh
if [ "$1" = api ] && [ "$2" = "repos/smoke/calm/pulls?state=open&per_page=30" ]; then
  echo '[{"number":1,"title":"Smoke PR","draft":false}]'
else
  echo "To get started with GitHub CLI, please run:  gh auth login" >&2
  exit 4
fi
"#;
const SMOKE_GH_HTML: &str = r#"<!doctype html><body>gh<script>
calm.github.api("repos/smoke/calm/pulls?state=open&per_page=30").then(function (pulls) {
  return calm.storage.set("seen", pulls.length)
    .then(function () { return calm.storage.get("seen"); })
    .then(function (seen) {
      return calm.github.api("user").then(
        function () { return "auth=answered"; },
        function (err) { return "auth=" + err.message.split(" (")[0]; }
      ).then(function (auth) {
        return calm.panel.setTitle("gh-ok " + pulls[0].title + " stored=" + seen + " " + auth);
      });
    });
}).catch(function (err) { calm.panel.setTitle("gh-err " + err.message); });
</script></body>"#;

async fn smoke_github_api(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let fixtures = match tempfile::Builder::new().prefix("calm-smoke-gh-").tempdir() {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("github.api temp dir: {err}")),
    };
    let gh = fixtures.path().join("bin/gh");
    let written = std::fs::create_dir_all(fixtures.path().join("bin"))
        .and_then(|()| std::fs::write(&gh, SMOKE_GH_SCRIPT))
        .and_then(|()| std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)))
        .and_then(|()| {
            write_smoke_plugin(
                &fixtures.path().join("plugins"),
                "smoke-gh",
                SMOKE_GH_HTML,
                "",
            )
        });
    if let Err(err) = written {
        smoke_fail(&format!("github.api fixture: {err}"));
    }
    // Plugins get their own dir: discovery reads every subdir, `bin` included.
    let discovery = calm_plugins::discover(&[fixtures.path().join("plugins")]);
    if discovery.plugins.len() != 1 || !discovery.errors.is_empty() {
        smoke_fail(&format!(
            "github.api fixture not discovered: {:?}",
            discovery.errors
        ));
    }
    let services = crate::plugin_services::Services {
        gh: calm_plugins::GhClient::new(gh),
        data_dir: fixtures.path().join("plugin-data"),
        opener: fixtures.path().join("no-open"),
    };
    let opened = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.plugin_services = std::sync::Arc::new(services);
            view.install_plugins(discovery.plugins);
            let kind = PanelKind::Plugin {
                plugin_id: "smoke-gh".to_string(),
                plugin_version: None,
                settings: serde_json::json!({}),
            };
            let id = view.scene.add(kind, Rect::new(20.0, 420.0, 300.0, 160.0));
            view.open_plugin_panel(id, "smoke-gh", window, cx)?;
            cx.notify();
            Ok::<_, String>(id)
        })
    });
    let id = match opened {
        Ok(Ok(id)) => id,
        Ok(Err(err)) => smoke_fail(&format!("github.api panel: {err}")),
        Err(err) => smoke_fail(&format!("github.api panel: {err:#}")),
    };
    let expected = "gh-ok Smoke PR stored=1 auth=gh is not logged in, run gh auth login";
    let started = Instant::now();
    let title = loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let title = cx.update(|cx| view.read(cx).scene.get(id).map(|p| p.title.clone()));
        if title.as_deref().is_some_and(|t| t.starts_with("gh-")) {
            break title.unwrap_or_default();
        }
        if started.elapsed() > SMOKE_FRAME_TIMEOUT {
            smoke_fail(&format!(
                "github.api did not answer within 10 s: title={title:?}"
            ));
        }
    };
    if title != expected {
        smoke_fail(&format!(
            "github.api: title={title:?}, expected {expected:?}"
        ));
    }
    let stored = std::fs::read_to_string(fixtures.path().join("plugin-data/smoke-gh.json"))
        .unwrap_or_else(|err| smoke_fail(&format!("github.api storage file: {err}")));
    if stored != r#"{"seen":1}"# {
        smoke_fail(&format!("github.api storage file holds {stored:?}"));
    }
    println!("smoke: github.api ok pulls=1 title=\"Smoke PR\" logged_out=reported storage=ok");
    // Back to the real services; quit follows, but nothing should keep the fake.
    cx.update(|cx| {
        view.update(cx, |view, _| {
            view.plugin_services = Default::default();
        })
    });
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the github.api temp dir: {err}");
    }
}

/// Opens Settings the way a menu click or `cmd-,` does: `OpenSettings` dispatched on the
/// canvas window, so handlers run while GPUI holds that window. Calling `open_settings`
/// directly skips that path, which is how a dead menu item once passed the smoke.
async fn smoke_settings_dispatch(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use gpui::Action as _;
    let dispatched = cx.update(|cx| -> gpui::Result<(bool, usize)> {
        let available = cx.is_action_available(&settings_menu::OpenSettings);
        let bindings = window.update(cx, |_, window, _| {
            window
                .bindings_for_action(&settings_menu::OpenSettings)
                .len()
        })?;
        if let Some(settings_window) = &view.read(cx).settings_window {
            settings_window.hide();
        }
        window.update(cx, |_, window, cx| {
            window.dispatch_action(settings_menu::OpenSettings.boxed_clone(), cx)
        })?;
        Ok((available, bindings))
    });
    let (available, bindings) =
        dispatched.unwrap_or_else(|err| smoke_fail(&format!("settings dispatch: {err:#}")));
    let started = Instant::now();
    let (visible, on_screen) = loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let state = cx.update(|cx| {
            view.read(cx)
                .settings_window
                .as_ref()
                .map(|w| (w.is_visible(), w.is_on_screen()))
                .unwrap_or((false, false))
        });
        if state.0 || started.elapsed() > Duration::from_secs(2) {
            break state;
        }
    };
    println!(
        "smoke: settings_dispatch available={available} bindings={bindings} visible={visible} on_screen={on_screen}"
    );
    if !(available && bindings > 0 && visible && on_screen) {
        smoke_fail("OpenSettings dispatched on the canvas window did not show Settings");
    }
    let activated = cx.update(|cx| {
        if let Some(settings_window) = &view.read(cx).settings_window {
            settings_window.hide();
        }
        window.update(cx, |_, window, _| window.activate_window())
    });
    activated.unwrap_or_else(|err| smoke_fail(&format!("settings dispatch: {err:#}")));
    smoke_settings_from_canvas_menu(view, window, cx).await;
}

/// Picks "Settings…" in the canvas context menu the way a click does and waits for the
/// Settings window to show.
async fn smoke_settings_from_canvas_menu(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    let listed = cx.update(|cx| {
        menu_ids(view.read(cx).canvas_menu()).contains(&calm_core::menu::OPEN_SETTINGS.to_string())
    });
    let picked = cx.update(|cx| {
        window.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                let world = Point::new(0.0, 0.0);
                view.apply_menu_choice(
                    calm_core::menu::OPEN_SETTINGS,
                    canvas_view::MenuTarget::Canvas { world },
                    window,
                    cx,
                );
            })
        })
    });
    picked.unwrap_or_else(|err| smoke_fail(&format!("settings menu item: {err:#}")));
    let started = Instant::now();
    let visible = loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let visible = cx.update(|cx| {
            view.read(cx)
                .settings_window
                .as_ref()
                .is_some_and(|w| w.is_visible())
        });
        if visible || started.elapsed() > Duration::from_secs(2) {
            break visible;
        }
    };
    println!("smoke: settings_menu_item listed={listed} visible={visible}");
    if !(listed && visible) {
        smoke_fail("the canvas menu Settings item did not show Settings");
    }
    let restored = cx.update(|cx| {
        if let Some(settings_window) = &view.read(cx).settings_window {
            settings_window.hide();
        }
        window.update(cx, |_, window, _| window.activate_window())
    });
    restored.unwrap_or_else(|err| smoke_fail(&format!("settings menu item: {err:#}")));
}

/// Sets a gradient background and the grid through the settings path, reads the file back,
/// then builds the native Settings window and checks every control was bound.
fn smoke_settings(view: &mut CanvasView, window: &mut Window, cx: &mut Context<CanvasView>) {
    use calm_core::background::BackgroundKind;
    use calm_core::settings::read_settings_from;
    use calm_core::settings_form::{Control, FieldState, settings_form};
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
    let count = calm_native::settings_window_count("CALM Settings")
        .unwrap_or_else(|err| smoke_fail(&format!("settings window count: {err}")));
    println!(
        "smoke: settings_window opens=2 windows={count} same={}",
        first == second
    );
    if count != 1 || first.is_none() || first != second {
        smoke_fail("opening Settings twice did not reuse one window");
    }
    // One binding per non-note control, plus a Reset button per field with its own value.
    let expected = settings_form(&view.settings.settings, &view.form_context())
        .iter()
        .flat_map(|section| &section.fields)
        .map(|field| {
            usize::from(!matches!(field.control, Control::Note(_)))
                + usize::from(field.state == FieldState::Set)
        })
        .sum::<usize>();
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

/// Brand icon for the Dock, cmd-tab, About and alerts when running without a bundle.
/// A failure keeps the default icon and is logged, never fatal.
fn install_app_icon() {
    match calm_native::app_icon::install_app_icon() {
        Ok(_) => {}
        Err(err) => eprintln!("calm: app icon not set, using the default: {err}"),
    }
}

/// The application icon must be the embedded brand image when running unbundled.
fn smoke_app_icon() {
    match calm_native::app_icon::app_icon_state() {
        Ok((set, w, h)) => {
            println!("smoke: app_icon set={set} size={w}x{h}");
            if !set || w <= 0.0 || h <= 0.0 {
                smoke_fail("app icon is not the brand image");
            }
        }
        Err(err) => smoke_fail(&format!("app icon state: {err}")),
    }
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys(canvas_view::key_bindings());
        cx.bind_keys(dev_menu::key_bindings());
        cx.bind_keys(settings_menu::key_bindings());
        dev_menu::install(cx);
        install_app_icon();
        if args.smoke {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if info.payload().is::<SmokeFailure>() {
                    return;
                }
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
            // The canvas runs under a transparent title bar; the traffic lights stay native.
            titlebar: Some(TitlebarOptions {
                title: Some("CALM".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            ..Default::default()
        };
        let smoke = args.smoke;
        // The smoke never reads or writes the real session or layouts.
        let plan = if smoke {
            None
        } else {
            let session = calm_core::layout::session_path();
            let layouts = calm_core::layout::layouts_dir();
            let setting = settings.settings.session.restore_on_launch;
            match launch_plan(&args.restore, setting, &session, &layouts) {
                Ok(plan) => Some((plan, session)),
                Err(exit) => {
                    eprintln!("calm: {}", exit.message);
                    std::process::exit(exit.code);
                }
            }
        };
        let builtin_plugins = settings.settings.plugins.builtin;
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let mut view = CanvasView::new(builtin_plugins, window, cx);
                view.set_settings_state(settings);
                if let Some((plan, session)) = plan {
                    view.apply_restore_plan(plan, session, window, cx);
                }
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
                    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || panels != 3 {
                        smoke_fail("canvas did not lay out with all three panels");
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

/// How long the smoke waits for a debounced save to reach the disk.
const SMOKE_AUTOSAVE_TIMEOUT: Duration = Duration::from_secs(5);

/// The `type` of each saved panel kind, back to front.
fn layout_kinds(layout: &calm_core::layout::LayoutFile) -> Vec<String> {
    layout
        .panels
        .iter()
        .map(|panel| match serde_json::to_value(&panel.kind) {
            Ok(value) => value["type"].as_str().unwrap_or("?").to_string(),
            Err(_) => "?".to_string(),
        })
        .collect()
}

/// Kind and rect of each panel, back to front: what must survive a save in order.
fn layout_order(
    layout: &calm_core::layout::LayoutFile,
) -> Vec<(String, calm_core::geometry::Rect)> {
    layout_kinds(layout)
        .into_iter()
        .zip(layout.panels.iter().map(|panel| panel.rect))
        .collect()
}

/// Sets `session.autosave` through the same path the Settings window uses.
fn smoke_set_autosave(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    on: bool,
    cx: &mut gpui::AsyncApp,
) {
    let set = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.update_settings(|s| s.session.autosave = on, window, cx);
        });
    });
    if let Err(err) = set {
        smoke_fail(&format!("autosave: settings update: {err}"));
    }
}

/// Drives autosave in the smoke's own temp dir (never the user's session): an untouched
/// scene writes nothing, a change is written after the debounce, and the quit hook writes
/// a newer order at once. Each file is decoded and compared with the scene.
async fn smoke_autosave(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    let dir = SMOKE_DIR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|dir| dir.path().to_path_buf());
    let Some(dir) = dir else {
        smoke_fail("autosave: no smoke dir");
    };
    let path = dir.join("session.json");
    let read = || calm_core::layout::load_layout_from(&dir, "session");
    let started = view.update(cx, |view, cx| view.start_autosave(path.clone(), cx));
    if let Err(err) = started {
        smoke_fail(&format!("autosave start: {err}"));
    }
    let debounce = view.read_with(cx, |view, _| view.settings.settings.session.debounce_ms);
    let debounce = Duration::from_millis(debounce);

    // No change: nothing may be written, even after the quiet period.
    view.update(cx, |_, cx| cx.notify());
    cx.background_executor().timer(debounce * 2).await;
    if path.exists() {
        smoke_fail("autosave: wrote an unchanged scene");
    }

    // A move, then wait for the debounced write.
    let original = view.read_with(cx, |view, _| view.scene.to_layout());
    let first = view.read_with(cx, |view, _| {
        view.scene.panels.first().map(|panel| panel.id)
    });
    view.update(cx, |view, cx| {
        if let Some(id) = first
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.x += 16.0;
        }
        // An explicit tick: a window that is not painting would never run the render hook.
        // The debounce timer it sets still does the write.
        view.autosave_tick(cx);
        cx.notify();
    });
    let started = Instant::now();
    let saved = loop {
        if let Ok(layout) = read() {
            break layout;
        }
        if started.elapsed() > debounce + SMOKE_AUTOSAVE_TIMEOUT {
            smoke_fail(&format!("autosave: no session file: {:?}", read().err()));
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    };
    let expected = view.read_with(cx, |view, _| view.scene.to_layout());
    if layout_order(&saved) != layout_order(&expected) {
        smoke_fail(&format!(
            "autosave: saved {:?}, scene {:?}",
            layout_order(&saved),
            layout_order(&expected)
        ));
    }

    // The quit hook writes a newer change at once, before any debounce. A move, not a
    // reorder: a reorder and back leaves the native stack out of order (backlog item 6).
    let last = view.read_with(cx, |view, _| view.scene.panels.last().map(|panel| panel.id));
    let after_quit = view.update(cx, |view, _| {
        if let Some(id) = last
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.y += 16.0;
        }
        view.save_on_quit();
        view.scene.to_layout()
    });
    let on_disk = match read() {
        Ok(layout) => layout,
        Err(err) => smoke_fail(&format!("autosave: quit write unreadable: {err}")),
    };
    if layout_order(&on_disk) != layout_order(&after_quit) {
        smoke_fail(&format!(
            "autosave: quit wrote {:?}, scene {:?}",
            layout_order(&on_disk),
            layout_order(&after_quit)
        ));
    }
    // Let any timer left from the move fire; an older write must not replace the quit file.
    cx.background_executor().timer(debounce * 2).await;
    let settled = read().ok().map(|layout| layout_order(&layout));
    if settled != Some(layout_order(&after_quit)) {
        smoke_fail(&format!("autosave: file changed after quit: {settled:?}"));
    }
    // The settings path applies `session.autosave` live: off stops the timer and the quit
    // write, on starts again. The gate opens only for this step.
    view.update(cx, |view, _| view.autosave_gate = true);
    smoke_set_autosave(view, window, false, cx);
    let writes_before = view.read_with(cx, |view, _| view.autosave_writes);
    view.update(cx, |view, cx| {
        if let Some(id) = first
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.y += 16.0;
        }
        view.autosave_tick(cx);
        cx.notify();
    });
    cx.background_executor().timer(debounce * 2).await;
    view.update(cx, |view, _| view.save_on_quit());
    let off_disk = read().ok().map(|layout| layout_order(&layout));
    let writes_off = view.read_with(cx, |view, _| view.autosave_writes);
    if off_disk != Some(layout_order(&after_quit)) || writes_off != writes_before {
        smoke_fail(&format!(
            "autosave: wrote while session.autosave was off: writes {writes_before}->{writes_off}"
        ));
    }
    smoke_set_autosave(view, window, true, cx);
    view.update(cx, |view, cx| {
        if let Some(id) = first
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.y += 16.0;
        }
        view.autosave_tick(cx);
        cx.notify();
    });
    let started = Instant::now();
    loop {
        let expected = view.read_with(cx, |view, _| layout_order(&view.scene.to_layout()));
        if read().ok().map(|layout| layout_order(&layout)) == Some(expected) {
            break;
        }
        if started.elapsed() > debounce + SMOKE_AUTOSAVE_TIMEOUT {
            smoke_fail("autosave: no write after turning session.autosave back on");
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    view.update(cx, |view, _| {
        view.autosave_gate = calm_core::autosave::APP_AUTOSAVE_ACTIVE;
    });
    println!("smoke: autosave live_toggle off=no_write on=write");
    // Undo the move and the reorder, so later steps see the scene they expect.
    let written = view.update(cx, |view, cx| {
        view.stop_autosave();
        if let Some(id) = last
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.y -= 16.0;
        }
        if let Some(id) = first
            && let Some(panel) = view.scene.get_mut(id)
        {
            panel.rect.origin.x -= 16.0;
            panel.rect.origin.y -= 32.0;
        }
        cx.notify();
        view.autosave_writes
    });
    let restored = view.read_with(cx, |view, _| view.scene.to_layout());
    if layout_order(&restored) != layout_order(&original) {
        smoke_fail("autosave: could not put the scene back");
    }
    // The reorder restacked the native views; wait for a paint to restack them back.
    let started = Instant::now();
    loop {
        let (scene, native) = cx.update(|cx| view.read(cx).native_zorder(cx));
        if scene == native {
            break;
        }
        if started.elapsed() > SMOKE_AUTOSAVE_TIMEOUT {
            smoke_fail(&format!(
                "autosave: native order {native:?}, scene {scene:?}"
            ));
        }
        view.update(cx, |_, cx| cx.notify());
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    if written < 2 {
        smoke_fail(&format!("autosave: written={written}, expected at least 2"));
    }
    println!(
        "smoke: autosave written={written} kinds={} order=match",
        layout_kinds(&on_disk).join(",")
    );
}

/// A data page with no script, so its url stays put between save and compare.
const SMOKE_SESSION_PAGE: &str = "data:text/html,%3Cp%3Esession%3C%2Fp%3E";
const SMOKE_MISSING_PLUGIN: &str = "calm-smoke-not-installed";

/// Closes every panel through the confirmed close path.
fn smoke_close_all(view: &Entity<CanvasView>, window: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let closed = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let ids: Vec<PanelId> = view.scene.panels.iter().map(|panel| panel.id).collect();
            let effects = ids.into_iter().map(Effect::ClosePanel).collect();
            view.apply_effects(effects, window, cx);
        });
    });
    if let Err(err) = closed {
        smoke_fail(&format!("session: close: {err}"));
    }
}

/// Restores panels whose saved state no longer fits: saved titles must survive the open
/// paths, and a gone cwd starts the shell in home even when the profile has a fixed cwd.
/// Every problem is collected, so one run reports them all.
fn smoke_session_edges(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    let fixed = match tempfile::Builder::new()
        .prefix("calm-smoke-fixed-")
        .tempdir()
    {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("session edges: temp dir: {err}")),
    };
    let gone = fixed.path().join("gone");
    smoke_close_all(view, window, cx);
    let old_cwd = view.update(cx, |view, _| {
        view.scene = calm_core::scene::Scene::default();
        let cwd = &mut view.settings.settings.terminal.defaults.cwd;
        cwd.replace(fixed.path().display().to_string())
    });
    let fixture = [
        PanelKind::Terminal {
            command: None,
            cwd: Some(gone.clone()),
            profile: None,
        },
        PanelKind::Plugin {
            plugin_id: "smoke-plugin".to_string(),
            plugin_version: None,
            settings: serde_json::Value::Null,
        },
        PanelKind::Tui {
            plugin_id: "top-example".to_string(),
            plugin_version: None,
        },
    ];
    let mut layout = calm_core::scene::Scene::default().to_layout();
    for (n, kind) in fixture.into_iter().enumerate() {
        layout.panels.push(calm_core::layout::LayoutPanel {
            kind,
            rect: Rect::new(40.0 + 340.0 * n as f64, 40.0, 320.0, 200.0),
            title: format!("saved title {n}"),
            minimized: false,
            overrides: Default::default(),
        });
    }
    let plan = RestorePlan {
        layout: Some(layout),
        layout_name: None,
        quarantine: None,
        autosave: calm_core::restore::AutosaveStart::Off("smoke fixture".to_string()),
        notices: Vec::new(),
    };
    let home = terminal_profiles::home_dir();
    let checked = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let gate = view.autosave_gate;
            view.apply_restore_plan(plan, fixed.path().join("session.json"), window, cx);
            view.autosave_gate = gate;
            let mut problems = Vec::new();
            for (n, panel) in view.scene.panels.iter().enumerate() {
                let want = format!("saved title {n}");
                if panel.title != want {
                    problems.push(format!("title {n}: want {want:?}, got {:?}", panel.title));
                }
            }
            let terminal = view.scene.panels.first().map(|panel| panel.id);
            let cwd = terminal
                .and_then(|id| view.terminal_meta(id))
                .map(|meta| meta.cwd.clone());
            if cwd.as_deref() != Some(home.as_path()) {
                problems.push(format!("cwd: want {}, got {cwd:?}", home.display()));
            }
            let note = terminal.and_then(|id| view.title_note(id));
            let saved_titles = view.scene.to_layout().panels;
            if note.is_none() || saved_titles.iter().any(|p| p.title.contains("cwd missing")) {
                problems.push(format!("cwd note: {note:?}, saved {saved_titles:?}"));
            }
            problems
        })
    });
    view.update(cx, |view, _| {
        view.settings.settings.terminal.defaults.cwd = old_cwd;
    });
    smoke_close_all(view, window, cx);
    let problems = match checked {
        Ok(problems) => problems,
        Err(err) => smoke_fail(&format!("session edges: restore: {err}")),
    };
    if !problems.is_empty() {
        smoke_fail(&format!("session edges: {}", problems.join("; ")));
    }
    println!("smoke: session edges titles=saved cwd=home");
}

/// Runs the launch restore path on `session` with restore on.
fn smoke_launch(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    session: &Path,
    cx: &mut gpui::AsyncApp,
) {
    let plan = match launch_plan(
        &LaunchFlags::default(),
        RestoreOnLaunch::On,
        session,
        &session.with_file_name("layouts"),
    ) {
        Ok(plan) => plan,
        Err(exit) => smoke_fail(&format!("session: launch plan: {}", exit.message)),
    };
    let applied = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_restore_plan(plan, session.to_path_buf(), window, cx);
        });
    });
    if let Err(err) = applied {
        smoke_fail(&format!("session: restore: {err}"));
    }
}

/// Saves a four panel canvas, closes it, drops the scene and restores it from disk
/// through the launch path; then a one byte corrupt copy is moved aside and starts empty.
async fn smoke_session(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use calm_core::layout::{LayoutFile, SessionWriter};
    let root = SMOKE_DIR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|dir| dir.path().join("session-restore"));
    let Some(root) = root else {
        smoke_fail("session: no smoke dir");
    };
    let cwd = match tempfile::Builder::new().prefix("calm-smoke-cwd-").tempdir() {
        Ok(dir) => dir,
        Err(err) => smoke_fail(&format!("session: cwd temp dir: {err}")),
    };
    let malformed_dir = root.join("malformed");
    if let Err(err) = std::fs::create_dir_all(&malformed_dir) {
        smoke_fail(&format!("session: dir: {err}"));
    }
    let session = root.join("session.json");
    view.update(cx, |view, _| view.stop_autosave());
    smoke_close_all(view, window, cx);

    let settings = serde_json::json!({ "repos": ["kolezka/CALM"], "depth": { "n": 3 } });
    let missing_settings = serde_json::json!({ "kept": [1, 2, 3] });
    let fixture = vec![
        PanelKind::Terminal {
            command: None,
            cwd: Some(cwd.path().to_path_buf()),
            profile: None,
        },
        PanelKind::Browser {
            url: SMOKE_SESSION_PAGE.to_string(),
            page_zoom: None,
        },
        PanelKind::Plugin {
            plugin_id: "smoke-plugin".to_string(),
            plugin_version: Some("0.1.0".to_string()),
            settings: settings.clone(),
        },
        PanelKind::Plugin {
            plugin_id: SMOKE_MISSING_PLUGIN.to_string(),
            plugin_version: Some("9.9.9".to_string()),
            settings: missing_settings.clone(),
        },
    ];
    let mut layout = calm_core::scene::Scene::default().to_layout();
    for (n, kind) in fixture.into_iter().enumerate() {
        layout.panels.push(calm_core::layout::LayoutPanel {
            kind,
            rect: Rect::new(40.0 + 340.0 * n as f64, 40.0, 320.0, 200.0),
            title: format!("panel {n}"),
            minimized: false,
            overrides: Default::default(),
        });
    }
    layout.camera.zoom = 0.75;
    // Build the live canvas, then save it with the session writer autosave uses.
    let built = cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let plan = RestorePlan {
                layout: Some(layout),
                layout_name: None,
                quarantine: None,
                autosave: calm_core::restore::AutosaveStart::Off("smoke fixture".to_string()),
                notices: Vec::new(),
            };
            // The fixture plan closes the launch gate; later steps run their own launches.
            let gate = view.autosave_gate;
            view.apply_restore_plan(plan, session.clone(), window, cx);
            view.autosave_gate = gate;
            let saved = view.scene.to_layout();
            let first = view.scene.panels.first().map(|panel| panel.id);
            let old = first
                .and_then(|id| view.terminal(id))
                .map(|terminal| (terminal.entity_id(), terminal.downgrade()));
            let mut writer = SessionWriter::new(
                session.clone(),
                0,
                calm_core::browser_url::BrowserQueryMode::StripAll,
            );
            writer.save(&saved).map(|_| (saved, old))
        })
    });
    let (saved_scene, old_terminal) = match built {
        Ok(Ok((saved, Some(old)))) => (saved, old),
        Ok(Ok((_, None))) => smoke_fail("session: fixture terminal did not start"),
        Ok(Err(err)) => smoke_fail(&format!("session: save: {err}")),
        Err(err) => smoke_fail(&format!("session: fixture: {err}")),
    };
    let on_disk: LayoutFile = match calm_core::layout::load_layout_from(&root, "session") {
        Ok(layout) => layout,
        Err(err) => smoke_fail(&format!("session: saved file: {err}")),
    };
    let saved = on_disk.panels.len();
    if layout_order(&on_disk) != layout_order(&saved_scene) {
        smoke_fail("session: the file does not match the scene it saved");
    }

    // Dispose of it all, then drop the old scene.
    smoke_close_all(view, window, cx);
    view.update(cx, |view, _| {
        view.scene = calm_core::scene::Scene::default()
    });
    let started = Instant::now();
    while old_terminal.1.upgrade().is_some() {
        if started.elapsed() > SMOKE_AUTOSAVE_TIMEOUT {
            smoke_fail("session: the closed terminal was never released");
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }

    smoke_launch(view, window, &session, cx);
    let report = view.update(cx, |view, cx| {
        let restored = view.scene.to_layout();
        let kinds: Vec<String> = view
            .scene
            .panels
            .iter()
            .zip(layout_kinds(&restored))
            .map(|(panel, kind)| match view.placeholder(panel.id) {
                Some(_) => "placeholder".to_string(),
                None => kind,
            })
            .collect();
        let new_terminal = view
            .scene
            .panels
            .first()
            .and_then(|panel| view.terminal(panel.id))
            .map(|terminal| terminal.entity_id());
        let frames = view.terminal_status(cx);
        (
            restored,
            kinds,
            new_terminal,
            frames,
            view.native_zorder(cx),
        )
    });
    view.update(cx, |view, _| view.stop_autosave());
    let (restored, kinds, new_terminal, frames, (scene_order, native_order)) = report;
    if scene_order != native_order {
        smoke_fail(&format!(
            "session: native order after restore: zorder={native_order:?} scene={scene_order:?}"
        ));
    }
    let kind_json = |layout: &LayoutFile| -> Vec<String> {
        layout
            .panels
            .iter()
            .map(|panel| serde_json::to_string(&panel.kind).unwrap_or_default())
            .collect()
    };
    let order =
        layout_order(&on_disk) == layout_order(&restored) && restored.camera == on_disk.camera;
    let plugin_settings = restored
        .panels
        .iter()
        .filter_map(|panel| match &panel.kind {
            PanelKind::Plugin { settings, .. } => Some(settings.clone()),
            _ => None,
        });
    let settings_match = kind_json(&on_disk) == kind_json(&restored)
        && plugin_settings.collect::<Vec<_>>() == vec![settings, missing_settings];
    let Some(new_terminal) = new_terminal else {
        smoke_fail("session: the restored terminal did not start");
    };
    // A new shell: a different entity, the old one released, and frames counted afresh.
    if new_terminal == old_terminal.0 || frames.len() != 1 || !frames[0].1 {
        smoke_fail(&format!(
            "session: terminal not fresh: old={:?} new={new_terminal:?} status={frames:?}",
            old_terminal.0
        ));
    }
    let line = format!(
        "smoke: session saved={saved} restored={} kinds={} order={} settings={} zorder=match",
        restored.panels.len(),
        kinds.join(","),
        if order { "match" } else { "mismatch" },
        if settings_match { "match" } else { "mismatch" },
    );
    println!("{line}");
    if saved != 4
        || restored.panels.len() != 4
        || kinds != ["terminal", "browser", "plugin", "placeholder"]
        || !order
        || !settings_match
    {
        smoke_fail(&format!("session: {line}"));
    }

    // The same file with one byte corrupted: moved aside, empty start.
    smoke_close_all(view, window, cx);
    view.update(cx, |view, _| {
        view.scene = calm_core::scene::Scene::default()
    });
    let bad = malformed_dir.join("session.json");
    let mut bytes = match std::fs::read(&session) {
        Ok(bytes) => bytes,
        Err(err) => smoke_fail(&format!("session: copy: {err}")),
    };
    bytes[0] = b'#';
    if let Err(err) = std::fs::write(&bad, &bytes) {
        smoke_fail(&format!("session: corrupt copy: {err}"));
    }
    smoke_launch(view, window, &bad, cx);
    let (empty, autosave_on) = view.update(cx, |view, _| {
        let state = (view.scene.panels.is_empty(), view.autosave_running());
        view.stop_autosave();
        state
    });
    let moved = malformed_dir.join("session.json.invalid-0");
    let moved_intact = std::fs::read(&moved).ok() == Some(bytes);
    println!(
        "smoke: session malformed moved={} started_empty={empty}",
        moved
            .file_name()
            .filter(|_| moved_intact)
            .map_or("none".into(), |name| name.to_string_lossy())
    );
    if !moved_intact || bad.exists() || !empty || !autosave_on {
        smoke_fail(&format!(
            "session: malformed moved={moved_intact} left={} empty={empty} autosave={autosave_on}",
            bad.exists()
        ));
    }
    if let Err(err) = cwd.close() {
        eprintln!("smoke: could not remove the cwd temp dir: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::browser_url::BrowserQueryMode;
    use calm_core::layout::SessionWriter;
    use calm_core::restore::AutosaveStart;

    fn saved_session(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("session.json");
        let mut scene = calm_core::scene::Scene::default();
        scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
            },
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        SessionWriter::new(path.clone(), 0, BrowserQueryMode::StripAll)
            .save(&scene.to_layout())
            .unwrap();
        path
    }

    #[test]
    fn launch_plan_restores_a_valid_session() {
        let dir = tempfile::tempdir().unwrap();
        let session = saved_session(dir.path());
        let flags = LaunchFlags::default();
        let plan = launch_plan(&flags, RestoreOnLaunch::On, &session, dir.path()).unwrap();
        assert_eq!(plan.layout.map(|layout| layout.panels.len()), Some(1));
        assert!(session.exists());
    }

    #[test]
    fn launch_plan_moves_a_malformed_session_aside_and_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let session = saved_session(dir.path());
        let mut bytes = std::fs::read(&session).unwrap();
        bytes[0] = b'#';
        std::fs::write(&session, &bytes).unwrap();
        let flags = LaunchFlags::default();
        let plan = launch_plan(&flags, RestoreOnLaunch::On, &session, dir.path()).unwrap();
        let moved = dir.path().join("session.json.invalid-0");
        assert_eq!(std::fs::read(&moved).ok(), Some(bytes), "not moved aside");
        assert!(!session.exists());
        assert_eq!(plan.layout, None);
        assert_eq!(plan.autosave, AutosaveStart::On);
        assert!(
            plan.notices
                .iter()
                .any(|n| n.contains("session.json.invalid-0"))
        );
    }

    #[test]
    fn launch_plan_exits_2_on_a_malformed_named_layout_without_moving_it() {
        let dir = tempfile::tempdir().unwrap();
        let layouts = dir.path().join("layouts");
        std::fs::create_dir(&layouts).unwrap();
        let named = layouts.join("work.json");
        std::fs::write(&named, b"{ not json").unwrap();
        let flags = LaunchFlags {
            layout: Some("work".to_string()),
            no_restore: false,
        };
        let session = dir.path().join("session.json");
        let exit = launch_plan(&flags, RestoreOnLaunch::On, &session, &layouts).unwrap_err();
        assert_eq!(exit.code, 2);
        assert!(named.exists());
        assert!(!layouts.join("work.json.invalid-0").exists());
    }
}
