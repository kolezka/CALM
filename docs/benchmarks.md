# Benchmarks

Every number here comes from a recorded run. Each entry gives the hardware, the method and
the command. `--bench` is not built yet; these come from opt-in smoke steps.

## Zoom and pan paint cost (`zoom_bench` smoke step), 2026-11-02

Hardware: Apple M4 Pro, 48 GB, macOS 27.0.1, built-in Liquid Retina XDR display
(3024 x 1964, scale 2). Release build (`cargo build --release -p calm-app`).

Method: `CALM_SMOKE_ONLY=zoom_bench ./target/release/calm-app --smoke`. The step runs after
the smoke has 5 panels (2 terminals, a browser, plugin panels). It feeds one camera per
frame through `CanvasView::camera_script`, so no OS input is synthesized:

- pan: 240 frames, 6 screen px per frame right, then back;
- zoom: 240 frames, 0.3 to 3.0 geometric about the canvas center, then back down through
  slightly different zooms (times 1.0037), as a real gesture would.

`cpu_ms` is render start to the end of CALM's canvas paint on the main thread. `titles` is
time inside `paint_text` (shaping and glyph rasterization), a part of `panels`. GPUI's draw
and present and AppKit layout after the paint are not timed; `interval_ms` is the time
between frame starts and includes them, so it varies between runs (one baseline run had a
34 ms zoom interval p50, the other 11.5 ms, with no change in CPU per frame).

Zoom phase, ms per frame (mean / p50 / p95):

| Build | Runs | cpu | titles | interval p50 |
| --- | --- | --- | --- | --- |
| before (`3a6546a` plus the bench step) | 2 | 2.15/1.87/4.45, 1.79/1.52/3.86 | 1.57/1.28/3.70, 1.36/1.09/3.36 | 34.2, 11.5 |
| after (text sizes on a 0.5 pt grid) | 3 | 1.24 to 1.33 / 0.93 to 0.98 / 3.18 to 3.65 | 0.81 to 0.89 / 0.38 to 0.42 / 2.66 to 3.21 | 9.4 to 10.0 |

Pan phase, both builds: cpu about 0.22 ms mean, titles 0.02 ms, interval p50 8.4 to 11 ms.
Other sections during zoom (both builds): render 0.05 to 0.13, grid up to 0.10, terminals
0.06, browsers 0.07, plugins 0.00, after (native order, status bar) 0.14 ms mean.

Verdict: titles were about 70% of CALM's own zoom frame cost; snapping the size cuts their
mean by about 40% and p50 by about 65%. The remaining p95 comes from frames where the size
steps and GPUI shapes the line again. With 5 panels none of this is near a 16 ms budget, so
this run does not show where a user's jank comes from; title cost grows with the panel count,
and frame time outside CALM's paint is not measured in those runs.

## Whole-frame native-kind matrix, 2026-11-02

Same hardware and release build as above. Baseline is `d5e669f` plus diagnostic code,
with no change to production pan, zoom or native placement behavior. Run:

```sh
CALM_SMOKE_ONLY=zoom_bench CALM_ZOOM_BENCH_VARIANTS=matrix \
  ./target/release/calm-app --smoke
```

`CALM_ZOOM_BENCH_VARIANTS` accepts `matrix`, `mixed` (the previous default fixture), or a
comma-separated subset of `none,terminal-2,terminal-8,browser-2,browser-8,plugin-2,plugin-8`.
Each variant closes the previous smoke panels and creates only the requested panel kind.
`none` has no native panels, but still has the native status pill. Panels are 280x180 in a
4-column grid, with a 2 s settling wait. Browser and plugin pages are static local HTML,
not a real website or a busy plugin. Terminals run the normal shell. The camera path is
the same 240-frame pan and zoom path described above. Preview and culling remain enabled,
so this measures their transitions as well as live native views, not 8 visible views at
every zoom. The smoke drives the camera, not actual pinch or scroll events.

`interval_ms` is render-start to next render-start, not a presentation timestamp.
`outside_paint_wall_ms` subtracts render-to-paint wall time from that interval, so it
includes display-link waits as well as GPUI draw, AppKit and CA work. The historical
`cpu_ms` field remains render-to-paint wall time. New `main_cpu_ms` and
`outside_paint_cpu_ms` use Darwin's calling-thread CPU clock, including system time but
excluding sleep and time on other threads. The clock has a busy-versus-sleep unit control.
`first_frame_unix_ms` maps the first painted frame to an Instruments recording. The
recording is trimmed to the first 240 painted frames, without the trailing idle frame.

Estimated missed slots sum `round(interval * maximum_refresh_hz) - 1`, floored at zero.
AppKit reports 120 Hz for this display. These are estimates against its maximum rate,
not measured dropped presentations on ProMotion. The arithmetic control fails if a
16.7 ms interval at 120 Hz counts as zero missed slots, while the same interval at 60 Hz
correctly counts as zero.

Two baseline matrix runs, both exit 0 with `summary pass=5 fail=0 skipped=0`. Each cell is
frame interval p50 / p95 / max in ms, followed by estimated missed slots in brackets.
Run 1 recorded 241 frames including one trailing frame. Run 2 trims that frame and adds
the thread CPU clock. This is repeatability evidence, not a before/after fix comparison.

| Native panels | Pan, run 1 | Zoom, run 1 | Pan, run 2 | Zoom, run 2 |
| --- | --- | --- | --- | --- |
| None | 8.32 / 9.76 / 27.85 [4] | 8.33 / 9.01 / 9.87 [0] | 8.30 / 11.38 / 12.67 [1] | 8.33 / 9.26 / 12.48 [0] |
| 2 terminals | 8.33 / 8.88 / 10.20 [0] | 8.33 / 9.25 / 12.03 [0] | 8.32 / 9.22 / 15.82 [1] | 8.31 / 9.29 / 12.40 [0] |
| 8 terminals | 8.32 / 9.17 / 12.69 [1] | 8.33 / 9.70 / 16.39 [4] | 8.34 / 9.58 / 12.23 [0] | 8.33 / 8.94 / 10.85 [0] |
| 2 browsers | 8.54 / 13.85 / 21.68 [29] | 8.35 / 13.49 / 16.47 [21] | 8.37 / 9.22 / 14.95 [2] | 8.32 / 9.22 / 11.19 [0] |
| 8 browsers | 9.13 / 16.45 / 30.13 [67] | 15.72 / 22.57 / 30.20 [171] | 8.33 / 9.32 / 14.81 [2] | 8.41 / 11.64 / 16.47 [7] |
| 2 plugins | 8.33 / 8.97 / 9.82 [0] | 8.33 / 9.78 / 14.87 [5] | 8.30 / 9.80 / 14.74 [4] | 8.33 / 9.15 / 11.72 [0] |
| 8 plugins | 8.29 / 9.29 / 20.65 [2] | 8.33 / 10.61 / 14.75 [3] | 8.33 / 9.37 / 17.60 [3] | 8.33 / 9.86 / 19.89 [8] |

Run 2 main-thread CPU outside CALM's paint, p50 / p95 in ms:

| Native panels | Pan | Zoom |
| --- | --- | --- |
| None | 3.31 / 3.80 | 3.15 / 3.66 |
| 2 terminals | 3.18 / 3.85 | 3.13 / 3.78 |
| 8 terminals | 3.20 / 3.65 | 3.21 / 3.86 |
| 2 browsers | 3.17 / 3.68 | 4.04 / 4.73 |
| 8 browsers | 3.40 / 4.02 | 6.07 / 7.30 |
| 2 plugins | 3.22 / 3.92 | 3.42 / 3.99 |
| 8 plugins | 3.16 / 3.68 | 3.48 / 4.30 |

### Time Profiler: AppKit cursor and CA work after paint

```sh
xcrun xctrace record --template 'Time Profiler' --output /tmp/calm-zoom.trace \
  --time-limit 30s --env CALM_SMOKE_ONLY=zoom_bench \
  --env CALM_ZOOM_BENCH_VARIANTS=browser-8 --target-stdout /tmp/calm-zoom.log \
  --launch -- "$PWD/target/release/calm-app" --smoke
xcrun xctrace export --input /tmp/calm-zoom.trace --toc
xcrun xctrace export --input /tmp/calm-zoom.trace \
  --xpath '/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]'
```

Two recordings exited 0, and both targets exited 0. Both XML exports and the decoder
exited 0. Positive control: the precise recording contains 4,593 main-thread samples
from `calm-app`, including its `calm_app::main`, `canvas_view::PaintSnapshot::paint_canvas`
and `paint_profile::end_frame` stacks. Phase windows use the first painted
frame's epoch and the interval span, with a 50 ms margin. The pan window contains 1,499
1 ms samples, and the zoom window contains 2,099. Counts below are inclusive and overlap:

| Stack symbol | Pan samples | Zoom samples |
| --- | --- | --- |
| `CA::Transaction::commit` / `flush` | 743 | 1,249 |
| `-[NSCursor set]` / `-[NSCursor _reallySet]` | 1,155 | 995 |
| `-[_AXFMouseCursorGenerator createImageForScale:]` | 1,038 | 890 |

The cursor stacks run through `-[NSView(NSTrackingArea) cursorUpdate:]`,
`_AXFCursorSetAndReturnSeed` and cursor-image generation. The first recording also shows
CA commit and this cursor path in its zoom window. The precise recording's browser-8
interval p50 / p95 / max is 8.50 / 17.04 / 30.21 ms for pan and
12.66 / 19.68 / 25.09 ms for zoom. Outside-paint main-thread CPU averages 6.06 ms for pan
and 7.58 ms for zoom. Profiling adds overhead, so do not compare its intervals directly
with the unprofiled matrix as a performance change.

Conclusion: these runs locate substantial time outside CALM's paint in AppKit/CA,
including repeated cursor-image work. They do not establish WebKit re-tiling, Ghostty
resizing, or a real-input event backlog as the user's cause. Browser-8 is more expensive
than plugin-8 here, but the large run-to-run variation remains unexplained. Pointer
location and accessibility cursor settings were not controlled. GPU presentation and
WebContent process CPU were not sampled. No production workaround is applied: suppressing
cursor updates or changing native scaling could alter input or visual behavior without
proving the cause. Next checks should isolate pointer location/cursor settings manually,
then compare native geometry strategies only if the expensive stacks remain. Real
trackpad feel remains a manual acceptance check.

## Cursor sets during motion (status pill fix), 2026-11-02

Same hardware and release build. The user's system pointer is customized (Accessibility
pointer colors), which makes every `-[NSCursor set]` render a new pointer image.

Cost of one set, two instruments: a standalone timing loop in Swift, 200 sets of the same
cursor, 4.0 to 4.2 ms per set (an empty loop of `NSCursor.current` calls: 0.005 ms); and an
in-process counter that wrapped the real set during `zoom_bench` (an experiment, not
shipped): 3.3 to 4.3 ms per set, 100 to 194 sets per second during bench motion.

Callers, from lldb with breakpoints that only count, the window key and the pointer inside
(the bench prints `key_and_pointer_inside`): AppKit's
`displayCycleUpdateStructuralRegions` calls `setCursorForMouseLocation`, which sends
`cursorUpdate:` to GPUI's whole-view cursor rect on `GPUIView` and sets its cursor again.
Panels that move change tracking regions on every frame. With no panels, the status pill
did the same: every paint ran `sizeToFit`, `setFrame` and `setHidden` on it.

`none` variant, cursor sets per 240-frame phase, guard counter in count-only mode:

| Status pill | Pan | Zoom |
| --- | --- | --- |
| updated every paint (before) | 241 | 241 |
| updated when its text changed | 0 | 228 |
| not created | 4 | 0 |
| updated when text or placement changed (this fix) | 0 | 3 |

Matrix (`CALM_ZOOM_BENCH_VARIANTS=matrix`), one run each, both exit 1 only on the
environmental `plugins` smoke step. Main-thread CPU outside paint, p50 in ms:

| Native panels | Before pan | After pan | Before zoom | After zoom |
| --- | --- | --- | --- | --- |
| None | 3.27 | 0.32 | 3.21 | 0.69 |
| 8 terminals | 3.16 | 2.99 | 3.31 | 3.16 |
| 8 browsers | 5.67 | 5.40 | 9.18 | 8.66 |
| 8 plugins | 3.39 | 3.10 | 3.58 | 3.54 |

Frame interval p50 did not change in the matrix (`none` stays at 8.33 ms). Panels still make
one set per frame while they move, because their views really move. Two local experiments,
neither shipped, cut that too (browser-8, two runs each, pill fix in all):

| Build | Pan sets | Pan interval p50 | Zoom sets | Zoom interval p50 |
| --- | --- | --- | --- | --- |
| pill fix only | 510, 511 | 9.52, 11.64 | 424, 375 | 16.73, 16.31 |
| A: GPUIView `cursorUpdate:` sets only when `currentCursor` differs | 60, 129 | 8.32, 8.32 | 93, 87 | 8.34, 8.58 |
| B: process-local `-[NSCursor set]` guard | 116, 116 | 8.34, 8.34 | 84, 90 | 9.09, 10.19 |

Real mouse moves were not measured (agents send no OS input). The user reports lag when the
pointer moves with no panels; a mouse move repaints, and every paint updated the pill, so
this fix should cover that case. That is inferred, not measured.

## Cursor guard with real pointer motion, 2026-11-02

Same hardware, release build, customized system pointer. `zoom_bench`, variant browser-8,
while the user moved the pointer in circles over a browser address bar, so AppKit had real
cursor updates to make. One run each, controlled: same binary, the guard installed in both
runs so it counts every set; `CALM_ZOOM_BENCH_SKIP_CURSOR=off` turns skipping off. Both runs
print `key_and_pointer_inside=Some((true, true))` for each phase.

| Build | Phase | Interval p50 ms | Cursor calls | Skipped | Real sets | ms per set |
| --- | --- | --- | --- | --- | --- | --- |
| pill fix only (guard off) | Pan | 13.86 | 520 | 0 | 520 | 4.24 |
| pill fix and guard | Pan | 8.40 | 519 | 394 | 125 | 3.16 |
| pill fix only (guard off) | Zoom | 16.64 | 419 | 0 | 419 | 3.42 |
| pill fix and guard | Zoom | 9.52 | 376 | 301 | 75 | 3.43 |

Main-thread CPU outside paint, p50: pan 6.19 to 1.13 ms, zoom 9.36 to 4.49 ms. The guard
ships as `canvas.skip_repeated_cursor_sets` (default on). The long-term fix is the GPUI
patch (A above) once upstream takes it (zed-industries/zed#66403); see `.ai/decisions.md`.
Cursor shapes and real trackpad feel are checked by hand, not by this bench.

## Settings window path (2026-11-02)

Hardware: Apple M4 Pro, macOS 27.0.1. Debug build (`cargo run`), smoke temp settings dir.

Command: `CALM_SMOKE_ONLY=terminals,settings_perf cargo run -q -p calm-app -- --smoke`
(exit 0 in every run below). The `settings_perf` step runs only when named.

Method:

- Changes: 60 calls through `settings_field_changed`, the entry point the Settings window's
  AppKit action reaches, 16 ms apart, cycling `grid.opacity`, `gradient.angle` (sliders) and
  `grid.color` (a color well). Wall time per call on the main thread, keymap and menu rebuild
  included. Frame intervals during those changes come from a probe that records each render
  without asking for frames.
- Sweeps: 60 camera pan and zoom steps 16 ms apart, set on the model with `cx.notify()`.
  `closed` and `open` are the Settings window hidden and shown (key). The `_fps` rows also
  run the FPS readout, which requests every frame.

| Measure | Before | After |
| --- | --- | --- |
| Total for 60 changes | 611 to 639 ms | 3.4 to 4.0 ms |
| Per change p50 / p95 | 9.9 to 10.7 / 11.1 to 12.8 ms | 0.06 to 0.07 / 0.08 to 0.09 ms |
| Frame p50 / p95 during changes | 24.7 to 24.8 / 28.4 to 31.7 ms | 15.0 / 21.5 to 23.2 ms |
| Pan frames, window closed, p50 | 16.8 to 17.5 ms | 16.7 to 18.8 ms |
| Pan frames, window open, p50 | 16.7 ms | 16.7 ms |
| Pan frames with FPS readout, closed, p50 | 9.0 to 11.7 ms | 8.9 to 9.4 ms |
| Pan frames with FPS readout, open, p50 / p95 | 34.2 to 41.2 / 41.6 to 41.8 ms | 39.9 to 40.0 / 42.7 ms |

Before is `3a6546a` plus the measurement step; after adds the fix. Two runs each, ranges
shown. A temporary per-phase timer (not committed) split one change before the fix:
`settings.json` durable write 7.9 to 9.9 ms, keymap and menu rebuild 1.2 to 1.3 ms, terminal
refresh with two terminals about 4 µs, panel Settings forms about 2 µs.

The FPS readout rows are GPUI's own throttle: an inactive window (the canvas, while Settings
is key) gets requested frames at most every 33 ms (`inactive_frame_interval`). Plain pan
frames are not throttled. Not measured: real trackpad or mouse input, which agents may not
send; GPUI also lifts the throttle while input arrives at 60 Hz or more.
