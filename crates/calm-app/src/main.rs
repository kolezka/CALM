mod background;
mod browser_panel;
mod canvas_view;
mod dev_menu;
mod grid_paint;
mod icon_image;
mod plugin_host;
mod settings_menu;
mod terminal;
mod terminal_profiles;
mod theme;
mod tui_host;

use calm_core::geometry::{Point, Rect, Size};
use calm_core::grid::{GridKind, mark_primitive_bound};
use calm_core::interaction::{Effect, ResizeEdge};
use calm_core::join::JOIN_SNAP_GAP;
use calm_core::preview::PanelMode;
use calm_core::scene::{PanelId, PanelKind};
use calm_core::snap::{SnapConfig, snap_move};
use calm_core::tui::TuiState;
use std::sync::{Mutex, PoisonError, TryLockError};
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
        };
        let id = view.scene.add(kind, rect);
        view.spawn_terminal(id, terminal::options(None, None), window, cx)?;
    }
    let url = SMOKE_BROWSER_PAGE.to_string();
    let id = view.scene.add(
        PanelKind::Browser { url: url.clone() },
        Rect::new(20.0, 220.0, 320.0, 200.0),
    );
    view.spawn_browser(id, &url, window, cx)?;
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
            let (scene, native) = cx.update(|cx| view.read(cx).native_zorder(cx));
            let line = format!("zorder={native:?} scene={scene:?}");
            // Two terminals and the browser share the native stack.
            if scene == native && scene.len() == 3 {
                println!("smoke: {line}");
                // Restacking the panels must not bury the status bar.
                smoke_status_bar(cx.update(|cx| view.read(cx).status_bar_state()));
                break;
            }
            if raised.elapsed() > SMOKE_ZORDER_TIMEOUT {
                smoke_fail(&format!("native order does not match the scene: {line}"));
            }
        }

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

        smoke_plugins(&view, window, cx).await;
        smoke_tui(&view, window, cx).await;

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
            let visible = cx.update(|cx| window_visible(window, cx));
            smoke_fail(&format!(
                "preview refresh outside 1..=3 snapshots: {line} window_visible={visible:?}"
            ));
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
                let visible = cx.update(|cx| window_visible(window, cx));
                smoke_fail(&format!(
                    "terminals not live at zoom 1.5: {previews:?} window_visible={visible:?}"
                ));
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

        smoke_resize(&view, window, cx).await;
        smoke_blur(&view, window, cx);
        smoke_settings_dispatch(&view, window, cx).await;
        smoke_plugin_menus(&view, window, cx).await;

        remove_smoke_dir();
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
        view.update(cx, |view, cx| view.restart_tui(ids[0], true, window, cx))
    });
    if let Err(err) = restarted {
        smoke_fail(&format!("tui restart: {err:#}"));
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
                settings: serde_json::json!({}),
            };
            let locked = view.scene.add(kind, Rect::new(20.0, 460.0, 300.0, 120.0));
            view.open_plugin_panel(locked, "smoke-locked", window)?;
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
            let rect = view.scene.get(browser).map(|panel| panel.rect);
            let page = view
                .browser_page_sizes()
                .into_iter()
                .find(|(id, _)| *id == browser)
                .map(|(_, size)| size);
            (rect, page, view.scene.get(locked).map(|panel| panel.rect))
        });
        let Some(rect) = rect else {
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
        let content = Size::new(
            rect.size.width,
            rect.size.height - calm_core::scene::CHROME_HEIGHT,
        );
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
const SMOKE_PLUGIN_HTML: &str = r#"<!doctype html><body>smoke<script>
calm.panel.setTitle("smoke-1").then(function () { return calm.panel.setTitle("smoke-ok"); });
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
                    settings: serde_json::json!({}),
                };
                let id = view.scene.add(kind, rect);
                view.open_plugin_panel(id, plugin_id, window)?;
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
        let (title, status) = cx.update(|cx| {
            let view = view.read(cx);
            let title = view.scene.get(ids[0]).map(|panel| panel.title.clone());
            (title, view.plugin_status())
        });
        if title.as_deref() == Some("smoke-ok") {
            println!("smoke: plugin loaded=true bridge=ok title=smoke-ok");
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

    // Raise the backmost terminal over the plugins: all native views must restack.
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
        let (scene, native) = cx.update(|cx| view.read(cx).native_zorder(cx));
        let line = format!("zorder={native:?} scene={scene:?}");
        // Two terminals, the browser and two plugins.
        if scene == native && scene.len() == 5 {
            println!("smoke: plugins {line}");
            break;
        }
        if raised.elapsed() > SMOKE_ZORDER_TIMEOUT {
            smoke_fail(&format!("native order with plugins does not match: {line}"));
        }
    }
    if let Err(err) = fixtures.close() {
        eprintln!("smoke: could not remove the plugin temp dir: {err}");
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
                    Ok((panel, origin, world))
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

/// Opens Settings the way a menu click or `cmd-,` does: `OpenSettings` dispatched on the
/// canvas window, so handlers run while GPUI holds that window. Calling `open_settings`
/// directly skips that path, which is how a dead menu item once passed the smoke.
async fn smoke_settings_dispatch(
    view: &Entity<CanvasView>,
    window: AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
) {
    use gpui::Action as _;
    let (available, bindings) = cx.update(|cx| {
        let available = cx.is_action_available(&settings_menu::OpenSettings);
        let bindings = window
            .update(cx, |_, window, _| {
                window
                    .bindings_for_action(&settings_menu::OpenSettings)
                    .len()
            })
            .unwrap_or_else(|err| smoke_fail(&format!("settings dispatch: {err:#}")));
        if let Some(settings_window) = &view.read(cx).settings_window {
            settings_window.hide();
        }
        window
            .update(cx, |_, window, cx| {
                window.dispatch_action(settings_menu::OpenSettings.boxed_clone(), cx)
            })
            .unwrap_or_else(|err| smoke_fail(&format!("settings dispatch: {err:#}")));
        (available, bindings)
    });
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
    cx.update(|cx| {
        if let Some(settings_window) = &view.read(cx).settings_window {
            settings_window.hide();
        }
        window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap_or_else(|err| smoke_fail(&format!("settings dispatch: {err:#}")));
    });
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
            // The canvas runs under a transparent title bar; the traffic lights stay native.
            titlebar: Some(TitlebarOptions {
                title: Some("CALM".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            ..Default::default()
        };
        let smoke = args.smoke;
        let builtin_plugins = settings.settings.plugins.builtin;
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let mut view = CanvasView::new(builtin_plugins, cx);
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
