//! CPU and memory readout math for the status bar. The native side reads raw counters;
//! everything here is pure so it can be tested.
//!
//! CPU percent follows Activity Monitor and `top`: 100% is one core fully busy, so a
//! process using four cores shows about 400%. Memory is phys_footprint, the number
//! Activity Monitor shows in its Memory column, in binary units (1 MB = 1024 * 1024 bytes).

use std::collections::HashMap;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

pub const DEFAULT_INTERVAL_MS: u64 = 1000;
pub const INTERVAL_MS_RANGE: RangeInclusive<u64> = 250..=10_000;

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MIB;

/// How the memory item picks its unit. `Auto` switches to GB at 1 GB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryUnit {
    #[default]
    Auto,
    Mb,
    Gb,
}

impl MemoryUnit {
    pub const ALL: [MemoryUnit; 3] = [MemoryUnit::Auto, MemoryUnit::Mb, MemoryUnit::Gb];

    pub fn label(self) -> &'static str {
        match self {
            MemoryUnit::Auto => "Auto",
            MemoryUnit::Mb => "MB",
            MemoryUnit::Gb => "GB",
        }
    }
}

/// Clamps a requested sampling interval into `INTERVAL_MS_RANGE`.
pub fn clamp_interval_ms(ms: u64) -> u64 {
    ms.clamp(*INTERVAL_MS_RANGE.start(), *INTERVAL_MS_RANGE.end())
}

/// Converts Mach absolute time ticks to nanoseconds with the `mach_timebase_info` ratio.
/// `proc_pid_rusage` reports CPU times in ticks, which are not nanoseconds on Apple silicon.
pub fn ticks_to_ns(ticks: u64, numer: u32, denom: u32) -> u64 {
    if denom == 0 {
        return 0;
    }
    (u128::from(ticks) * u128::from(numer) / u128::from(denom)) as u64
}

/// CPU time used over a wall-clock span, as a percent of one core.
pub fn cpu_percent(cpu_delta_ns: u64, wall_delta_ns: u64) -> Option<f64> {
    if wall_delta_ns == 0 {
        return None;
    }
    Some(cpu_delta_ns as f64 / wall_delta_ns as f64 * 100.0)
}

/// Turns successive per-process CPU totals into a percent over the last interval.
/// Only processes seen in both samples count, so a process that started or ended in
/// between does not spike or dip the reading.
#[derive(Clone, Debug, Default)]
pub struct CpuTracker {
    last: Option<(u64, HashMap<u32, u64>)>,
}

impl CpuTracker {
    /// `wall_ns` is a monotonic timestamp; `cpu_ns` is (pid, total CPU ns) per process.
    /// Returns `None` for the first sample or when time did not move.
    pub fn update(&mut self, wall_ns: u64, cpu_ns: &[(u32, u64)]) -> Option<f64> {
        let now: HashMap<u32, u64> = cpu_ns.iter().copied().collect();
        let percent = self.last.as_ref().and_then(|(last_wall, last)| {
            let used: u64 = now
                .iter()
                .filter_map(|(pid, total)| last.get(pid).map(|prev| total.saturating_sub(*prev)))
                .sum();
            cpu_percent(used, wall_ns.saturating_sub(*last_wall))
        });
        self.last = Some((wall_ns, now));
        percent
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// `4.2% CPU`, or `--% CPU` before two samples exist.
pub fn cpu_text(percent: Option<f64>) -> String {
    match percent {
        Some(p) if p.is_finite() => format!("{:.1}% CPU", p.max(0.0)),
        _ => "--% CPU".to_string(),
    }
}

/// `312 MB` or `1.25 GB`, per `unit`.
pub fn memory_text(bytes: u64, unit: MemoryUnit) -> String {
    let bytes = bytes as f64;
    let gb = match unit {
        MemoryUnit::Auto => bytes >= GIB,
        MemoryUnit::Mb => false,
        MemoryUnit::Gb => true,
    };
    if gb {
        format!("{:.2} GB", bytes / GIB)
    } else {
        format!("{:.0} MB", bytes / MIB)
    }
}

/// The readout's items in display order: FPS, CPU, memory. Hidden items are empty, which
/// `status_text` drops.
pub fn stats_items(
    fps: Option<String>,
    cpu: Option<String>,
    memory: Option<String>,
) -> [String; 3] {
    [
        fps.unwrap_or_default(),
        cpu.unwrap_or_default(),
        memory.unwrap_or_default(),
    ]
}

/// Reads the CPU percent and memory in MB back out of status bar text. Used by the smoke
/// to check what the pill really shows.
pub fn parse_stats_text(text: &str) -> (Option<f64>, Option<f64>) {
    let mut cpu = None;
    let mut memory_mb = None;
    for item in text.split(crate::status_bar::STATUS_BAR_SEPARATOR) {
        if let Some(value) = item.strip_suffix("% CPU") {
            cpu = value.parse().ok();
        } else if let Some(value) = item.strip_suffix(" MB") {
            memory_mb = value.parse().ok();
        } else if let Some(value) = item.strip_suffix(" GB") {
            memory_mb = value.parse::<f64>().ok().map(|gb| gb * 1024.0);
        }
    }
    (cpu, memory_mb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_bar::status_text;

    #[test]
    fn ticks_convert_with_the_timebase() {
        // Apple silicon reports 125/3: 24 ticks are 1000 ns.
        assert_eq!(ticks_to_ns(24, 125, 3), 1000);
        assert_eq!(ticks_to_ns(1000, 1, 1), 1000);
        assert_eq!(ticks_to_ns(u64::MAX, 1, 1), u64::MAX);
        assert_eq!(ticks_to_ns(5, 1, 0), 0);
    }

    #[test]
    fn percent_is_of_one_core() {
        assert_eq!(cpu_percent(500_000_000, 1_000_000_000), Some(50.0));
        assert_eq!(cpu_percent(2_000_000_000, 1_000_000_000), Some(200.0));
        assert_eq!(cpu_percent(1, 0), None);
    }

    #[test]
    fn tracker_needs_two_samples_and_sums_processes() {
        let mut tracker = CpuTracker::default();
        assert_eq!(tracker.update(0, &[(1, 100), (2, 50)]), None);
        // Over 1000 ns, pid 1 used 300 and pid 2 used 200.
        assert_eq!(tracker.update(1000, &[(1, 400), (2, 250)]), Some(50.0));
    }

    #[test]
    fn tracker_ignores_processes_not_in_both_samples() {
        let mut tracker = CpuTracker::default();
        tracker.update(0, &[(1, 100), (2, 9_999)]);
        // pid 2 exited, pid 3 is new: only pid 1's 100 ns count.
        assert_eq!(tracker.update(1000, &[(1, 200), (3, 5_000)]), Some(10.0));
        // pid 3 counts from its second sample on.
        assert_eq!(tracker.update(2000, &[(1, 200), (3, 5_500)]), Some(50.0));
    }

    #[test]
    fn tracker_survives_a_counter_going_backwards_and_resets() {
        let mut tracker = CpuTracker::default();
        tracker.update(0, &[(1, 500)]);
        assert_eq!(tracker.update(1000, &[(1, 100)]), Some(0.0));
        assert_eq!(tracker.update(1000, &[(1, 200)]), None);
        tracker.reset();
        assert_eq!(tracker.update(5000, &[(1, 300)]), None);
    }

    #[test]
    fn cpu_text_formats_and_dashes() {
        assert_eq!(cpu_text(Some(4.24)), "4.2% CPU");
        assert_eq!(cpu_text(Some(312.0)), "312.0% CPU");
        assert_eq!(cpu_text(Some(-0.1)), "0.0% CPU");
        assert_eq!(cpu_text(None), "--% CPU");
        assert_eq!(cpu_text(Some(f64::NAN)), "--% CPU");
    }

    #[test]
    fn memory_text_picks_units() {
        let mb = 1024 * 1024;
        assert_eq!(memory_text(312 * mb, MemoryUnit::Auto), "312 MB");
        assert_eq!(memory_text(1023 * mb, MemoryUnit::Auto), "1023 MB");
        assert_eq!(memory_text(1024 * mb, MemoryUnit::Auto), "1.00 GB");
        assert_eq!(memory_text(1280 * mb, MemoryUnit::Auto), "1.25 GB");
        assert_eq!(memory_text(1280 * mb, MemoryUnit::Mb), "1280 MB");
        assert_eq!(memory_text(256 * mb, MemoryUnit::Gb), "0.25 GB");
        assert_eq!(memory_text(0, MemoryUnit::Auto), "0 MB");
    }

    #[test]
    fn interval_clamps_to_range() {
        assert_eq!(clamp_interval_ms(0), 250);
        assert_eq!(clamp_interval_ms(1000), 1000);
        assert_eq!(clamp_interval_ms(60_000), 10_000);
    }

    #[test]
    fn items_join_as_fps_cpu_memory_and_skip_hidden() {
        let fps = Some("FPS 60.0  p95 16.7ms".to_string());
        let cpu = Some("4.2% CPU".to_string());
        let memory = Some("312 MB".to_string());
        let all = stats_items(fps.clone(), cpu.clone(), memory.clone());
        assert_eq!(
            status_text(all.iter().map(String::as_str)),
            "FPS 60.0  p95 16.7ms  ·  4.2% CPU  ·  312 MB"
        );
        let no_cpu = stats_items(fps, None, memory);
        assert_eq!(
            status_text(no_cpu.iter().map(String::as_str)),
            "FPS 60.0  p95 16.7ms  ·  312 MB"
        );
        let none = stats_items(None, None, None);
        assert_eq!(status_text(none.iter().map(String::as_str)), "");
    }

    #[test]
    fn parse_reads_back_what_the_formatters_write() {
        let text = format!(
            "100%  ·  FPS 60.0  p95 16.7ms  ·  {}  ·  {}",
            cpu_text(Some(4.2)),
            memory_text(312 * 1024 * 1024, MemoryUnit::Auto)
        );
        assert_eq!(parse_stats_text(&text), (Some(4.2), Some(312.0)));
        let gb = memory_text(1280 * 1024 * 1024, MemoryUnit::Auto);
        assert_eq!(parse_stats_text(&gb), (None, Some(1280.0)));
        assert_eq!(parse_stats_text("--% CPU  ·  100%"), (None, None));
    }
}
