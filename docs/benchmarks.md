# Benchmarks

Every number here comes from a recorded run. Hardware and method go with each table.

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
and frame time outside CALM's paint is not measured yet.
