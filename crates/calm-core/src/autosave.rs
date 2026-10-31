//! Autosave timing and write ordering. Pure: the caller passes the time in and does the
//! writes, so every rule here is tested without a clock or a GUI.

use std::time::Duration;

use crate::layout::{LayoutError, LayoutFile};

/// A scene that keeps changing (a long pan) still saves this often.
pub const MAX_DIRTY_AGE: Duration = Duration::from_secs(10);

/// Whether the app starts autosave at launch. Off until restore at launch lands (plan
/// Task 5): without restore, the first change on the empty start canvas would replace the
/// last real session. The smoke drives autosave explicitly in its own temp dir.
pub const APP_AUTOSAVE_ACTIVE: bool = false;

/// Autosave runs only when the build gate and the `session.autosave` setting both allow it.
pub fn autosave_active(gate: bool, setting: bool) -> bool {
    gate && setting
}

/// Decides from the launch read of the session file whether autosave may write it.
/// A missing file is fine. A file that could not be read or parsed is left alone for this
/// launch: writing would bury it before restore or the user could look at it.
pub fn autosave_allowed_after_read(read: &Result<LayoutFile, LayoutError>) -> Result<(), String> {
    match read {
        Ok(_) | Err(LayoutError::NotFound(_)) => Ok(()),
        Err(error) => Err(format!("autosave off for this launch: {error}")),
    }
}

/// What the status pill shows after a failed save.
pub fn save_error_text(message: &str) -> String {
    format!("\u{26a0} session not saved: {message}")
}

/// Decides when a dirty scene is saved: after `debounce` of quiet, or once it has been
/// dirty for `max_age`. Times are offsets from any fixed start the caller picks.
#[derive(Clone, Debug)]
pub struct AutosaveClock {
    debounce: Duration,
    max_age: Duration,
    seen: u64,
    /// Last revision handed to a writer, whether that write worked or not. A failed
    /// revision is not retried until the scene changes again.
    submitted: u64,
    last_change: Option<Duration>,
    dirty_since: Option<Duration>,
}

impl AutosaveClock {
    /// `revision` is the scene as it is now; it counts as saved, so an untouched start
    /// canvas never overwrites the session file.
    pub fn new(debounce: Duration, revision: u64) -> Self {
        Self::with_max_age(debounce, MAX_DIRTY_AGE, revision)
    }

    pub fn with_max_age(debounce: Duration, max_age: Duration, revision: u64) -> Self {
        AutosaveClock {
            debounce,
            max_age,
            seen: revision,
            submitted: revision,
            last_change: None,
            dirty_since: None,
        }
    }

    /// Records the scene revision at `now`. A new revision restarts the quiet period.
    pub fn observe(&mut self, revision: u64, now: Duration) {
        if revision == self.seen {
            return;
        }
        self.seen = revision;
        self.last_change = Some(now);
        self.dirty_since.get_or_insert(now);
    }

    pub fn is_dirty(&self) -> bool {
        self.seen != self.submitted
    }

    /// Whether `revision` differs from the last one handed to a writer. The quit path asks
    /// this with the live revision, which the clock may not have observed yet.
    pub fn changed_since_submit(&self, revision: u64) -> bool {
        revision != self.submitted
    }

    /// The revision to save now, if one is due.
    pub fn due(&self, now: Duration) -> Option<u64> {
        let deadline = self.next_deadline()?;
        (now >= deadline).then_some(self.seen)
    }

    /// When the next save becomes due, so the caller can set a timer.
    pub fn next_deadline(&self) -> Option<Duration> {
        if !self.is_dirty() {
            return None;
        }
        let quiet = self.last_change? + self.debounce;
        let oldest = self.dirty_since? + self.max_age;
        Some(quiet.min(oldest))
    }

    /// The caller handed `revision` to a writer.
    pub fn submitted(&mut self, revision: u64) {
        self.submitted = revision;
        if revision == self.seen {
            self.last_change = None;
            self.dirty_since = None;
        }
    }
}

/// How one write request ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written(u64),
    /// A newer or equal generation is already on disk, so this one was dropped.
    Skipped {
        generation: u64,
        newest: u64,
    },
    Failed {
        generation: u64,
        message: String,
    },
}

/// Puts writes in generation order. Shared behind a lock by the background writer and the
/// quit path, so an older write that runs late never replaces a newer file.
#[derive(Debug)]
pub struct OrderedWriter<W> {
    writer: W,
    written: Option<u64>,
}

impl<W> OrderedWriter<W> {
    pub fn new(writer: W) -> Self {
        OrderedWriter {
            writer,
            written: None,
        }
    }

    pub fn written(&self) -> Option<u64> {
        self.written
    }

    pub fn inner(&self) -> &W {
        &self.writer
    }

    /// Runs `write` unless `generation` is not newer than what is on disk.
    pub fn write(
        &mut self,
        generation: u64,
        write: impl FnOnce(&mut W) -> Result<(), String>,
    ) -> WriteOutcome {
        if let Some(newest) = self.written
            && generation <= newest
        {
            return WriteOutcome::Skipped { generation, newest };
        }
        match write(&mut self.writer) {
            Ok(()) => {
                self.written = Some(generation);
                WriteOutcome::Written(generation)
            }
            Err(message) => WriteOutcome::Failed {
                generation,
                message,
            },
        }
    }
}

/// One save in flight, at most one waiting. A newer snapshot replaces the waiting one.
#[derive(Debug)]
pub struct SaveQueue<S> {
    in_flight: Option<u64>,
    queued: Option<(u64, S)>,
    saved: Option<u64>,
    error: Option<String>,
}

impl<S> Default for SaveQueue<S> {
    fn default() -> Self {
        SaveQueue {
            in_flight: None,
            queued: None,
            saved: None,
            error: None,
        }
    }
}

impl<S> SaveQueue<S> {
    /// Returns the job to start now, or `None` when one is already running and this one
    /// waits.
    pub fn submit(&mut self, generation: u64, snapshot: S) -> Option<(u64, S)> {
        if self.in_flight.is_some() {
            self.queued = Some((generation, snapshot));
            return None;
        }
        self.in_flight = Some(generation);
        Some((generation, snapshot))
    }

    /// Acknowledges the running job and returns the next one to start. An ack for a
    /// generation that is not in flight is ignored.
    pub fn finish(&mut self, outcome: &WriteOutcome) -> Option<(u64, S)> {
        let generation = match outcome {
            WriteOutcome::Written(generation)
            | WriteOutcome::Skipped { generation, .. }
            | WriteOutcome::Failed { generation, .. } => *generation,
        };
        if self.in_flight != Some(generation) {
            return None;
        }
        self.in_flight = None;
        self.record(outcome);
        let next = self.queued.take()?;
        self.in_flight = Some(next.0);
        Some(next)
    }

    /// Records a write done outside the queue, such as the one on quit.
    pub fn record(&mut self, outcome: &WriteOutcome) {
        match outcome {
            WriteOutcome::Written(generation) => {
                self.saved = Some(self.saved.map_or(*generation, |s| s.max(*generation)));
                self.error = None;
            }
            WriteOutcome::Skipped { newest, .. } => {
                self.saved = Some(self.saved.map_or(*newest, |s| s.max(*newest)));
            }
            WriteOutcome::Failed {
                generation,
                message,
            } => {
                if self.saved.is_none_or(|saved| *generation > saved) {
                    self.error = Some(message.clone());
                }
            }
        }
    }

    /// Drops the waiting job, for the quit path that writes a newer snapshot itself.
    pub fn take_queued(&mut self) -> Option<(u64, S)> {
        self.queued.take()
    }

    pub fn in_flight(&self) -> Option<u64> {
        self.in_flight
    }

    pub fn saved(&self) -> Option<u64> {
        self.saved
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::browser_url::BrowserQueryMode;
    use crate::geometry::Rect;
    use crate::layout::{SessionWriter, load_layout_from};
    use crate::scene::{PanelKind, Scene};

    const SECOND: Duration = Duration::from_secs(1);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn app_activation_gate_is_off() {
        assert!(!autosave_active(APP_AUTOSAVE_ACTIVE, true));
        assert!(autosave_active(true, true));
        assert!(!autosave_active(true, false));
    }

    #[test]
    fn no_change_never_saves() {
        let mut clock = AutosaveClock::new(SECOND, 7);
        for t in [0, 500, 1_000, 20_000] {
            clock.observe(7, ms(t));
            assert_eq!(clock.due(ms(t)), None, "t={t}");
        }
        assert_eq!(clock.next_deadline(), None);
        assert!(!clock.is_dirty());
    }

    #[test]
    fn quiet_period_saves_after_debounce() {
        let mut clock = AutosaveClock::new(SECOND, 0);
        clock.observe(1, ms(100));
        // Renders that see the same revision do not restart the quiet period.
        clock.observe(1, ms(900));
        assert_eq!(clock.due(ms(1_099)), None);
        assert_eq!(clock.due(ms(1_100)), Some(1));
        // A change inside the quiet period restarts it.
        clock.observe(2, ms(900));
        assert_eq!(clock.due(ms(1_100)), None);
        assert_eq!(clock.next_deadline(), Some(ms(1_900)));
        assert_eq!(clock.due(ms(1_900)), Some(2));
        clock.submitted(2);
        assert_eq!(clock.due(ms(5_000)), None);
    }

    #[test]
    fn continuous_change_saves_at_max_age() {
        let mut clock = AutosaveClock::new(SECOND, 0);
        // A pan bumping the revision every 100 ms never goes quiet.
        let mut saves = Vec::new();
        for step in 1..=250u64 {
            let now = ms(step * 100);
            clock.observe(step, now);
            if let Some(revision) = clock.due(now) {
                saves.push((now, revision));
                clock.submitted(revision);
            }
        }
        assert_eq!(
            saves,
            vec![(ms(10_100), 101), (ms(20_200), 202)],
            "dirty since 100 ms, saved at 10.1 s, dirty again at 10.2 s"
        );
    }

    #[test]
    fn failed_save_waits_for_the_next_change() {
        let mut clock = AutosaveClock::new(SECOND, 0);
        clock.observe(1, ms(0));
        assert_eq!(clock.due(SECOND), Some(1));
        clock.submitted(1);
        // The write failed; nothing changes, so nothing is retried in a loop.
        assert!(!clock.is_dirty());
        assert_eq!(clock.due(ms(30_000)), None);
        clock.observe(2, ms(30_000));
        assert_eq!(clock.due(ms(31_000)), Some(2));
    }

    fn written(log: &Arc<Mutex<Vec<u64>>>) -> Vec<u64> {
        log.lock().unwrap().clone()
    }

    #[test]
    fn queue_coalesces_to_the_latest_snapshot() {
        let mut queue = SaveQueue::default();
        assert_eq!(queue.submit(1, "a"), Some((1, "a")));
        assert_eq!(queue.submit(2, "b"), None);
        assert_eq!(queue.submit(3, "c"), None);
        assert_eq!(queue.finish(&WriteOutcome::Written(1)), Some((3, "c")));
        assert_eq!(queue.finish(&WriteOutcome::Written(3)), None);
        assert_eq!(queue.saved(), Some(3));
        assert_eq!(queue.in_flight(), None);
    }

    #[test]
    fn mutation_during_an_in_flight_save_is_saved_next() {
        let mut clock = AutosaveClock::new(SECOND, 0);
        let mut queue = SaveQueue::default();
        let mut writer = OrderedWriter::new(Vec::new());
        clock.observe(1, ms(0));
        let due = clock.due(SECOND).unwrap();
        clock.submitted(due);
        let job = queue.submit(due, "first").unwrap();
        // The user drags while the first write runs.
        clock.observe(2, ms(1_100));
        let due = clock.due(ms(2_100)).unwrap();
        clock.submitted(due);
        assert_eq!(queue.submit(due, "second"), None);
        let outcome = writer.write(job.0, |log| {
            log.push(job.1);
            Ok(())
        });
        let next = queue.finish(&outcome).expect("queued job starts");
        let outcome = writer.write(next.0, |log| {
            log.push(next.1);
            Ok(())
        });
        assert_eq!(queue.finish(&outcome), None);
        assert_eq!(writer.inner(), &vec!["first", "second"]);
        assert_eq!(queue.saved(), Some(2));
        assert!(!clock.is_dirty());
    }

    #[test]
    fn quit_sees_a_change_the_clock_has_not_observed() {
        let mut clock = AutosaveClock::new(SECOND, 0);
        clock.observe(1, ms(0));
        clock.submitted(1);
        // A change right before quit, with no tick in between.
        assert!(!clock.is_dirty());
        assert!(clock.changed_since_submit(2));
        assert!(!clock.changed_since_submit(1));
    }

    #[test]
    fn stale_ack_is_ignored() {
        let mut queue = SaveQueue::default();
        queue.submit(4, ());
        assert_eq!(queue.finish(&WriteOutcome::Written(3)), None);
        assert_eq!(queue.in_flight(), Some(4));
        assert_eq!(queue.saved(), None);
    }

    #[test]
    fn failure_shows_until_a_later_write_succeeds() {
        let mut queue = SaveQueue::default();
        queue.submit(1, ());
        queue.finish(&WriteOutcome::Failed {
            generation: 1,
            message: "disk full".to_string(),
        });
        assert_eq!(queue.error(), Some("disk full"));
        assert_eq!(
            save_error_text("disk full"),
            "\u{26a0} session not saved: disk full"
        );
        queue.submit(2, ());
        queue.finish(&WriteOutcome::Written(2));
        assert_eq!(queue.error(), None);
        // An older failure arriving after a newer success does not bring the warning back.
        queue.record(&WriteOutcome::Failed {
            generation: 1,
            message: "late".to_string(),
        });
        assert_eq!(queue.error(), None);
    }

    #[test]
    fn older_write_never_replaces_a_newer_quit_write() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::new(Mutex::new(OrderedWriter::new(log.clone())));
        let mut queue = SaveQueue::default();
        let job = queue.submit(5, ()).unwrap();
        // Quit writes generation 6 before the background job got the lock.
        let quit = writer.lock().unwrap().write(6, |log| {
            log.lock().unwrap().push(6);
            Ok(())
        });
        queue.record(&quit);
        let late = writer.lock().unwrap().write(job.0, |log| {
            log.lock().unwrap().push(job.0);
            Ok(())
        });
        assert_eq!(
            late,
            WriteOutcome::Skipped {
                generation: 5,
                newest: 6
            }
        );
        queue.finish(&late);
        assert_eq!(written(&log), vec![6]);
        assert_eq!(queue.saved(), Some(6));
        // The same generation twice is written once.
        let again = writer.lock().unwrap().write(6, |_| Ok(()));
        assert!(matches!(again, WriteOutcome::Skipped { .. }));
    }

    #[test]
    fn failed_write_does_not_advance_the_written_generation() {
        let mut writer = OrderedWriter::new(());
        let failed = writer.write(3, |_| Err("no space".to_string()));
        assert_eq!(
            failed,
            WriteOutcome::Failed {
                generation: 3,
                message: "no space".to_string()
            }
        );
        assert_eq!(writer.written(), None);
        assert_eq!(writer.write(3, |_| Ok(())), WriteOutcome::Written(3));
    }

    fn scene_with(kinds: &[PanelKind]) -> Scene {
        let mut scene = Scene::default();
        for (i, kind) in kinds.iter().enumerate() {
            scene.add(kind.clone(), Rect::new(i as f64 * 400.0, 0.0, 300.0, 200.0));
        }
        scene
    }

    #[test]
    fn late_background_write_leaves_the_quit_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path: PathBuf = dir.path().join("session.json");
        let session = SessionWriter::new(path.clone(), 1, BrowserQueryMode::StripAll);
        let writer = Arc::new(Mutex::new(OrderedWriter::new(session)));
        let terminal = PanelKind::Terminal {
            command: None,
            cwd: None,
        };
        let older = scene_with(std::slice::from_ref(&terminal)).to_layout();
        let newer = scene_with(&[terminal.clone(), terminal]).to_layout();
        let quit = writer
            .lock()
            .unwrap()
            .write(2, |w| w.save(&newer).map_err(|e| e.to_string()));
        assert_eq!(quit, WriteOutcome::Written(2));
        let background = {
            let writer = writer.clone();
            std::thread::spawn(move || {
                writer
                    .lock()
                    .unwrap()
                    .write(1, |w| w.save(&older).map_err(|e| e.to_string()))
            })
        };
        assert!(matches!(
            background.join().unwrap(),
            WriteOutcome::Skipped { .. }
        ));
        let on_disk = load_layout_from(dir.path(), "session").unwrap();
        assert_eq!(on_disk.panels.len(), 2);
    }

    #[test]
    fn launch_read_failure_turns_autosave_off() {
        let dir = tempfile::tempdir().unwrap();
        let missing = load_layout_from(dir.path(), "session");
        assert!(matches!(missing, Err(LayoutError::NotFound(_))));
        assert_eq!(autosave_allowed_after_read(&missing), Ok(()));

        let path = dir.path().join("session.json");
        let session = SessionWriter::new(path.clone(), 0, BrowserQueryMode::StripAll);
        let mut session = session;
        session.save(&Scene::default().to_layout()).unwrap();
        let valid = load_layout_from(dir.path(), "session");
        assert_eq!(
            autosave_allowed_after_read(&valid),
            Ok(()),
            "positive control"
        );

        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, &text[..text.len() - 1]).unwrap();
        let malformed = load_layout_from(dir.path(), "session");
        assert!(matches!(malformed, Err(LayoutError::Malformed { .. })));
        assert!(autosave_allowed_after_read(&malformed).is_err());

        // A directory where the file should be is an I/O error, not a missing file.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let unreadable = load_layout_from(dir.path(), "session");
        assert!(
            matches!(unreadable, Err(LayoutError::Io { .. })),
            "{unreadable:?}"
        );
        let message = autosave_allowed_after_read(&unreadable).unwrap_err();
        assert!(
            message.starts_with("autosave off for this launch"),
            "{message}"
        );
    }
}
