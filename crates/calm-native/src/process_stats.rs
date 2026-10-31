//! CPU and memory of the CALM process, optionally with its child processes, read through
//! libproc. The math lives in `calm_core::process_stats`; this only reads the counters.
//!
//! Memory is `ri_phys_footprint`, which Activity Monitor shows. CPU is user plus system
//! time summed over all threads (`ri_user_time + ri_system_time`), in Mach ticks.

use std::ffi::{c_int, c_void};
use std::io;
use std::time::Instant;

use calm_core::process_stats::{CpuTracker, ticks_to_ns};

/// `RUSAGE_INFO_V2` from `<sys/resource.h>`.
const RUSAGE_INFO_V2: c_int = 2;
/// Stop walking the process tree past this many processes.
const MAX_PROCESSES: usize = 512;
const MAX_CHILDREN_PER_PROCESS: usize = 256;

/// `struct rusage_info_v2` from `<sys/resource.h>`, field for field.
#[repr(C)]
#[derive(Default)]
struct RusageInfoV2 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
    ri_child_user_time: u64,
    ri_child_system_time: u64,
    ri_child_pkg_idle_wkups: u64,
    ri_child_interrupt_wkups: u64,
    ri_child_pageins: u64,
    ri_child_elapsed_abstime: u64,
    ri_diskio_bytesread: u64,
    ri_diskio_byteswritten: u64,
}

#[repr(C)]
#[derive(Default)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

// libproc and mach live in libSystem, which every macOS binary links.
unsafe extern "C" {
    fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut c_void) -> c_int;
    fn proc_listchildpids(ppid: c_int, buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> c_int;
}

/// Raw counters of one process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessCounters {
    /// User plus system CPU time over all threads, in nanoseconds.
    pub cpu_ns: u64,
    pub phys_footprint: u64,
    pub resident: u64,
}

/// One reading for the status bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessSample {
    /// Percent of one core since the previous sample. `None` on the first sample.
    pub cpu_percent: Option<f64>,
    /// phys_footprint in bytes, summed over the counted processes.
    pub memory_bytes: u64,
    /// Processes counted: 1 for CALM alone, more with children.
    pub processes: usize,
}

fn timebase() -> (u32, u32) {
    let mut info = MachTimebaseInfo::default();
    // SAFETY: `info` is a valid, writable `mach_timebase_info` struct that outlives the call.
    let status = unsafe { mach_timebase_info(&mut info) };
    if status != 0 || info.denom == 0 {
        // Intel reports 1/1; a failure here should never happen, so fall back to that.
        (1, 1)
    } else {
        (info.numer, info.denom)
    }
}

/// Counters of `pid`. Fails for a pid that exited or belongs to another user.
pub fn read_process(pid: u32) -> io::Result<ProcessCounters> {
    read_with_timebase(pid, timebase())
}

fn read_with_timebase(pid: u32, (numer, denom): (u32, u32)) -> io::Result<ProcessCounters> {
    let pid = c_int::try_from(pid).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut info = RusageInfoV2::default();
    // SAFETY: flavor RUSAGE_INFO_V2 writes exactly one `rusage_info_v2`, which `info` is,
    // laid out with #[repr(C)] to match the header. The pointer is valid for the call.
    let status = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V2, (&raw mut info).cast::<c_void>()) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    let ticks = info.ri_user_time.saturating_add(info.ri_system_time);
    Ok(ProcessCounters {
        cpu_ns: ticks_to_ns(ticks, numer, denom),
        phys_footprint: info.ri_phys_footprint,
        resident: info.ri_resident_size,
    })
}

/// Direct children of `pid`. Empty when it has none or the call fails.
fn child_pids(pid: u32) -> Vec<u32> {
    let Ok(pid) = c_int::try_from(pid) else {
        return Vec::new();
    };
    let mut buffer = vec![0 as c_int; MAX_CHILDREN_PER_PROCESS];
    let size = c_int::try_from(buffer.len() * size_of::<c_int>()).unwrap_or(c_int::MAX);
    // SAFETY: `buffer` holds `size` writable bytes of `c_int`s, and libproc writes at most
    // `size` bytes. The return value is the number of pids written.
    let count = unsafe { proc_listchildpids(pid, buffer.as_mut_ptr().cast::<c_void>(), size) };
    let count = usize::try_from(count).unwrap_or(0).min(buffer.len());
    buffer[..count]
        .iter()
        .filter_map(|pid| u32::try_from(*pid).ok().filter(|pid| *pid > 0))
        .collect()
}

/// `root` and, when asked, every process below it, breadth first, capped.
fn process_tree(root: u32, include_children: bool) -> Vec<u32> {
    let mut pids = vec![root];
    if !include_children {
        return pids;
    }
    let mut next = 0;
    while next < pids.len() && pids.len() < MAX_PROCESSES {
        let children = child_pids(pids[next]);
        let room = MAX_PROCESSES - pids.len();
        pids.extend(children.into_iter().take(room));
        next += 1;
    }
    pids
}

/// Samples CPU and memory. Keep one per readout: CPU percent needs the previous sample.
/// Cheap (one `proc_pid_rusage` per process), but run it off the main thread anyway.
pub struct ProcessSampler {
    pid: u32,
    timebase: (u32, u32),
    start: Instant,
    tracker: CpuTracker,
    include_children: Option<bool>,
}

impl ProcessSampler {
    /// A sampler for this process.
    pub fn new() -> Self {
        Self::for_pid(std::process::id())
    }

    pub fn for_pid(pid: u32) -> Self {
        Self {
            pid,
            timebase: timebase(),
            start: Instant::now(),
            tracker: CpuTracker::default(),
            include_children: None,
        }
    }

    /// Reads the counters. Fails only when the root process cannot be read; children
    /// that exit mid-walk are skipped.
    pub fn sample(&mut self, include_children: bool) -> io::Result<ProcessSample> {
        if self.include_children != Some(include_children) {
            // A different set of processes: the old totals do not compare.
            self.tracker.reset();
            self.include_children = Some(include_children);
        }
        let wall_ns = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let root = read_with_timebase(self.pid, self.timebase)?;
        let mut cpu = vec![(self.pid, root.cpu_ns)];
        let mut memory_bytes = root.phys_footprint;
        for pid in process_tree(self.pid, include_children).into_iter().skip(1) {
            if let Ok(counters) = read_with_timebase(pid, self.timebase) {
                cpu.push((pid, counters.cpu_ns));
                memory_bytes = memory_bytes.saturating_add(counters.phys_footprint);
            }
        }
        Ok(ProcessSample {
            cpu_percent: self.tracker.update(wall_ns, &cpu),
            memory_bytes,
            processes: cpu.len(),
        })
    }
}

impl Default for ProcessSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn busy(duration: Duration) -> u64 {
        let start = Instant::now();
        let mut x: u64 = 1;
        while start.elapsed() < duration {
            for i in 0..10_000u64 {
                x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(i));
            }
        }
        x
    }

    #[test]
    fn reads_own_memory_and_cpu_time_grows_with_work() {
        let pid = std::process::id();
        let before = read_process(pid).unwrap();
        assert!(before.phys_footprint > 1024 * 1024, "{before:?}");
        assert!(before.resident > 0, "{before:?}");
        busy(Duration::from_millis(200));
        let after = read_process(pid).unwrap();
        // 200 ms of spinning on one thread; allow for scheduling.
        let used = after.cpu_ns - before.cpu_ns;
        assert!(used >= 100_000_000, "cpu grew by only {used} ns");
        assert!(
            used < 10_000_000_000,
            "cpu grew by {used} ns, ticks not converted?"
        );
    }

    #[test]
    fn sampler_reports_a_plausible_percent() {
        let mut sampler = ProcessSampler::new();
        let first = sampler.sample(false).unwrap();
        assert_eq!(first.cpu_percent, None);
        assert_eq!(first.processes, 1);
        busy(Duration::from_millis(200));
        let second = sampler.sample(false).unwrap();
        let percent = second.cpu_percent.unwrap();
        // One thread spinning: near 100%, other test threads may add some.
        assert!((50.0..=1600.0).contains(&percent), "{percent}");
        assert!(second.memory_bytes > 0);
    }

    #[test]
    fn children_are_counted_when_asked() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let mut sampler = ProcessSampler::new();
        let alone = sampler.sample(false).unwrap();
        let tree = sampler.sample(true).unwrap();
        child.kill().ok();
        child.wait().ok();
        assert_eq!(alone.processes, 1);
        assert!(tree.processes >= 2, "{tree:?}");
        // Switching scope restarts the CPU delta.
        assert_eq!(tree.cpu_percent, None);
    }

    #[test]
    fn a_missing_process_is_an_error() {
        assert!(read_process(u32::MAX - 7).is_err());
    }

    /// Run with `--nocapture` to see the cost of one sample.
    #[test]
    fn sampling_is_cheap() {
        let mut sampler = ProcessSampler::new();
        let runs = 1000;
        for (include_children, label) in [(false, "self"), (true, "children")] {
            let start = Instant::now();
            for _ in 0..runs {
                sampler.sample(include_children).unwrap();
            }
            let per_sample = start.elapsed() / runs;
            println!("process_stats: one sample ({label}) takes {per_sample:?}");
            assert!(per_sample < Duration::from_millis(5), "{per_sample:?}");
        }
    }
}
