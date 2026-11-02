//! Opt-in timing of render and paint sections, read by the `zoom_bench` smoke step.
//! Off unless `start` was called; then each section adds one `Instant` pair per call.

use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// `CanvasView::render`: the paint snapshot, grid prepare, input targets.
    Render,
    Background,
    Grid,
    /// Terminal modes, placements and snapshots.
    Terminals,
    Browsers,
    Plugins,
    /// Chrome of every panel, titles included.
    Panels,
    /// Title text, a part of `Panels`.
    Titles,
    /// Plugin placement, native order and status bar after the canvas paint.
    After,
}

pub const SECTIONS: [Section; 9] = [
    Section::Render,
    Section::Background,
    Section::Grid,
    Section::Terminals,
    Section::Browsers,
    Section::Plugins,
    Section::Panels,
    Section::Titles,
    Section::After,
];

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Section::Render => "render",
            Section::Background => "background",
            Section::Grid => "grid",
            Section::Terminals => "terminals",
            Section::Browsers => "browsers",
            Section::Plugins => "plugins",
            Section::Panels => "panels",
            Section::Titles => "titles",
            Section::After => "after",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub started: Instant,
    pub sections: [Duration; SECTIONS.len()],
    /// Render start to the end of the canvas paint; zero if the paint never came.
    pub total: Duration,
}

thread_local! {
    static FRAMES: RefCell<Option<Vec<Frame>>> = const { RefCell::new(None) };
}

pub fn start() {
    FRAMES.with(|f| *f.borrow_mut() = Some(Vec::new()));
}

/// Stops recording and returns the frames that were painted.
pub fn stop() -> Vec<Frame> {
    let frames = FRAMES.with(|f| f.borrow_mut().take()).unwrap_or_default();
    frames.into_iter().filter(|f| !f.total.is_zero()).collect()
}

pub fn begin_frame() {
    FRAMES.with(|f| {
        if let Some(frames) = f.borrow_mut().as_mut() {
            frames.push(Frame {
                started: Instant::now(),
                sections: [Duration::ZERO; SECTIONS.len()],
                total: Duration::ZERO,
            });
        }
    });
}

pub fn end_frame() {
    FRAMES.with(|f| {
        if let Some(frame) = f.borrow_mut().as_mut().and_then(|frames| frames.last_mut()) {
            frame.total = frame.started.elapsed();
        }
    });
}

pub fn time<R>(section: Section, run: impl FnOnce() -> R) -> R {
    let _scope = Scope::new(section);
    run()
}

/// Adds the time until it drops to `section` of the current frame.
pub struct Scope {
    section: Section,
    started: Option<Instant>,
}

impl Scope {
    pub fn new(section: Section) -> Self {
        let on = FRAMES.with(|f| f.borrow().is_some());
        Scope {
            section,
            started: on.then(Instant::now),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        let spent = started.elapsed();
        FRAMES.with(|f| {
            if let Some(frame) = f.borrow_mut().as_mut().and_then(|frames| frames.last_mut()) {
                let index = SECTIONS
                    .iter()
                    .position(|s| *s == self.section)
                    .unwrap_or(0);
                frame.sections[index] += spent;
            }
        });
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn spread(mut values: Vec<f64>) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    let max = values.iter().copied().fold(0.0, f64::max);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let p50 = calm_core::stats::percentile(&mut values, 50.0);
    let p95 = calm_core::stats::percentile(&mut values, 95.0);
    format!("mean={mean:.2} p50={p50:.2} p95={p95:.2} max={max:.2}")
}

/// Lines for one bench phase: frame interval, CPU per frame, then each section, in ms.
pub fn report(phase: &str, frames: &[Frame]) -> Vec<String> {
    let intervals: Vec<f64> = frames
        .windows(2)
        .map(|pair| ms(pair[1].started.saturating_duration_since(pair[0].started)))
        .collect();
    let mut lines = vec![
        format!(
            "phase={phase} frames={} interval_ms {}",
            frames.len(),
            spread(intervals)
        ),
        format!(
            "phase={phase} cpu_ms {}",
            spread(frames.iter().map(|f| ms(f.total)).collect())
        ),
    ];
    for (index, section) in SECTIONS.iter().enumerate() {
        let values = frames.iter().map(|f| ms(f.sections[index])).collect();
        lines.push(format!(
            "phase={phase} section={} {}",
            section.name(),
            spread(values)
        ));
    }
    lines
}
