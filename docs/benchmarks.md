# Benchmarks

Every number here comes from a recorded run. Each entry gives the hardware, the method and
the command. `--bench` is not built yet; these come from opt-in smoke steps.

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
