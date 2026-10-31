//! Frame rate over a trailing time window, for the FPS readout and the benchmark.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::stats::percentile;

pub const DEFAULT_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct FrameStats {
    window: Duration,
    frames: VecDeque<Instant>,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW)
    }
}

impl FrameStats {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            frames: VecDeque::new(),
        }
    }

    pub fn record(&mut self, now: Instant) {
        self.frames.push_back(now);
        while let Some(&first) = self.frames.front() {
            if now.saturating_duration_since(first) > self.window {
                self.frames.pop_front();
            } else {
                break;
            }
        }
    }

    /// Intervals between the frames in the window, divided by the time they span.
    pub fn fps(&self) -> Option<f64> {
        let (first, last) = (self.frames.front()?, self.frames.back()?);
        let span = last.saturating_duration_since(*first).as_secs_f64();
        if self.frames.len() < 2 || span <= 0.0 {
            return None;
        }
        Some((self.frames.len() - 1) as f64 / span)
    }

    /// Nearest-rank 95th percentile of the frame intervals in the window.
    pub fn p95_ms(&self) -> Option<f64> {
        if self.frames.len() < 2 {
            return None;
        }
        let mut intervals: Vec<f64> = self
            .frames
            .iter()
            .zip(self.frames.iter().skip(1))
            .map(|(a, b)| b.saturating_duration_since(*a).as_secs_f64() * 1000.0)
            .collect();
        Some(percentile(&mut intervals, 95.0))
    }

    /// Frames currently inside the window.
    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    pub fn reset(&mut self) {
        self.frames.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_at(start: Instant, offsets_ms: &[u64]) -> FrameStats {
        let mut stats = FrameStats::default();
        for ms in offsets_ms {
            stats.record(start + Duration::from_millis(*ms));
        }
        stats
    }

    #[test]
    fn fps_is_none_before_two_frames() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        assert_eq!(stats.fps(), None);
        assert_eq!(stats.p95_ms(), None);
        stats.record(start);
        assert_eq!(stats.fps(), None);
        assert_eq!(stats.p95_ms(), None);
        stats.record(start + Duration::from_millis(16));
        assert!(stats.fps().is_some());
    }

    #[test]
    fn fps_counts_frames_in_trailing_window() {
        // 61 frames 16 ms apart span 0.96 s: 60 intervals / 0.96 s = 62.5 fps.
        let offsets: Vec<u64> = (0..61).map(|i| i * 16).collect();
        let stats = frames_at(Instant::now(), &offsets);
        let fps = stats.fps().unwrap();
        assert!((fps - 62.5).abs() < 0.5, "fps {fps}");
    }

    #[test]
    fn old_frames_fall_out_of_window() {
        let start = Instant::now();
        let mut offsets: Vec<u64> = (0..30).map(|i| i * 10).collect();
        // Long after the first burst, two frames 100 ms apart are all that is left.
        offsets.push(5_000);
        offsets.push(5_100);
        let stats = frames_at(start, &offsets);
        let fps = stats.fps().unwrap();
        assert!((fps - 10.0).abs() < 1e-6, "fps {fps}");
        assert!((stats.p95_ms().unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn p95_of_mixed_intervals() {
        // 18 intervals of 10 ms and 2 of 50 ms: nearest rank 19 of 20 is 50 ms.
        let mut offsets = vec![0u64];
        let mut t = 0;
        for interval in std::iter::repeat_n(10, 18).chain([50, 50]) {
            t += interval;
            offsets.push(t);
        }
        let stats = frames_at(Instant::now(), &offsets);
        assert!((stats.p95_ms().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn reset_clears() {
        let mut stats = frames_at(Instant::now(), &[0, 16, 32]);
        assert!(stats.fps().is_some());
        stats.reset();
        assert_eq!(stats.fps(), None);
        assert_eq!(stats.p95_ms(), None);
    }
}
