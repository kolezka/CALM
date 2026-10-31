//! Host to page event coalescing, pure so tests drive time by hand.
//!
//! The host owns one [`Coalescer`] per subscribed page, calls [`Coalescer::due`] after paint
//! and from a timer that runs only while [`Coalescer::next_deadline`] is `Some`, and sends each
//! non-empty batch with one `eval_js` of [`emit_batch_js`].

use std::collections::HashMap;
use std::time::{Duration, Instant};

use calm_core::scene::PanelId;
use serde_json::Value;

/// Shortest window a page may ask for (about one frame).
pub const MIN_WINDOW: Duration = Duration::from_millis(16);
/// Longest window a page may ask for.
pub const MAX_WINDOW: Duration = Duration::from_millis(5000);
/// Spec default for coalesced events.
pub const DEFAULT_WINDOW: Duration = Duration::from_millis(100);

const PANEL_CHANGED: &str = "panelChanged";
const PANEL_REMOVED: &str = "panelRemoved";

/// Events that are never merged. Everything else is coalesced per `(event, panel)`.
/// `bus` is in the spec table as "none": merging it would lose messages.
const DISCRETE_EVENTS: &[&str] = &[
    "panelAdded",
    PANEL_REMOVED,
    "terminalExited",
    "tuiExited",
    "command",
    "terminalCommandFinished",
    "browserNavigation",
    "browserMessage",
    "layoutSaved",
    "layoutOpened",
    "bus",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Every push is delivered, in arrival order.
    Discrete,
    /// Only the latest payload per `(event, panel)` is delivered, at most once per window.
    Coalesced,
}

/// The spec's coalescing column: discrete events listed there as "none", the rest coalesced.
pub fn classify(event: &str) -> EventKind {
    if DISCRETE_EVENTS.contains(&event) {
        EventKind::Discrete
    } else {
        EventKind::Coalesced
    }
}

fn clamp_window(window: Duration) -> Duration {
    window.clamp(MIN_WINDOW, MAX_WINDOW)
}

type Key = (String, Option<PanelId>);

#[derive(Debug)]
struct Pending {
    seq: u64,
    payload: Value,
    pushed_at: Instant,
}

#[derive(Debug, Default)]
struct Slot {
    pending: Option<Pending>,
    last_sent: Option<Instant>,
}

#[derive(Debug)]
struct DiscreteEvent {
    seq: u64,
    event: String,
    panel: Option<PanelId>,
    payload: Value,
    pushed_at: Instant,
}

/// Pending events for one subscribed page.
#[derive(Debug)]
pub struct Coalescer {
    default_window: Duration,
    windows: HashMap<String, Duration>,
    slots: HashMap<Key, Slot>,
    discrete: Vec<DiscreteEvent>,
    next_seq: u64,
}

impl Coalescer {
    /// `default_window` applies to coalesced events without their own window. Clamped to
    /// 16 to 5000 ms like [`Coalescer::set_window`].
    pub fn new(default_window: Duration) -> Self {
        Self {
            default_window: clamp_window(default_window),
            windows: HashMap::new(),
            slots: HashMap::new(),
            discrete: Vec::new(),
            next_seq: 0,
        }
    }

    /// Sets the page's window for a coalesced event (`throttleMs`). Discrete events ignore it.
    pub fn set_window(&mut self, event: &str, window: Duration) {
        if classify(event) == EventKind::Discrete {
            return;
        }
        self.windows.insert(event.to_string(), clamp_window(window));
    }

    /// The window in force for `event`.
    pub fn window(&self, event: &str) -> Duration {
        self.windows
            .get(event)
            .copied()
            .unwrap_or(self.default_window)
    }

    pub fn push(
        &mut self,
        event: &str,
        panel: Option<PanelId>,
        payload: Value,
        kind: EventKind,
        now: Instant,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        match kind {
            EventKind::Discrete => {
                if event == PANEL_REMOVED && panel.is_some() {
                    // A change for a panel that is gone is noise the page cannot act on.
                    if let Some(slot) = self.slots.get_mut(&(PANEL_CHANGED.to_string(), panel)) {
                        slot.pending = None;
                    }
                }
                self.discrete.push(DiscreteEvent {
                    seq,
                    event: event.to_string(),
                    panel,
                    payload,
                    pushed_at: now,
                });
            }
            EventKind::Coalesced => {
                if event == PANEL_CHANGED && panel.is_some() && self.removal_pending(panel) {
                    return;
                }
                let slot = self.slots.entry((event.to_string(), panel)).or_default();
                let pushed_at = slot.pending.as_ref().map_or(now, |p| p.pushed_at);
                slot.pending = Some(Pending {
                    seq,
                    payload,
                    pushed_at,
                });
            }
        }
    }

    fn removal_pending(&self, panel: Option<PanelId>) -> bool {
        self.discrete
            .iter()
            .any(|queued| queued.event == PANEL_REMOVED && queued.panel == panel)
    }

    fn ready_at(&self, key: &Key, slot: &Slot) -> Option<Instant> {
        let pending = slot.pending.as_ref()?;
        Some(match slot.last_sent {
            Some(sent) => sent + self.window(&key.0),
            None => pending.pushed_at,
        })
    }

    /// Takes every event whose time has come, in push order. A coalesced key is ready when
    /// its window has passed since its last send, so a quiet stream still gets its final
    /// payload one window after the previous send.
    pub fn due(&mut self, now: Instant) -> Vec<(String, Value)> {
        let mut out: Vec<(u64, String, Value)> = self
            .discrete
            .drain(..)
            .map(|d| (d.seq, d.event, d.payload))
            .collect();

        let ready: Vec<Key> = self
            .slots
            .iter()
            .filter(|(key, slot)| self.ready_at(key, slot).is_some_and(|at| at <= now))
            .map(|(key, _)| key.clone())
            .collect();
        for key in ready {
            let slot = self.slots.get_mut(&key).expect("ready key exists");
            let pending = slot.pending.take().expect("ready slot has a payload");
            slot.last_sent = Some(now);
            out.push((pending.seq, key.0, pending.payload));
        }

        // Forget idle keys whose window is over; their next push sends at once anyway.
        let windows = &self.windows;
        let default_window = self.default_window;
        self.slots.retain(|(event, _), slot| {
            let window = windows.get(event).copied().unwrap_or(default_window);
            slot.pending.is_some() || slot.last_sent.is_some_and(|sent| sent + window > now)
        });

        out.sort_by_key(|(seq, _, _)| *seq);
        out.into_iter()
            .map(|(_, event, payload)| (event, payload))
            .collect()
    }

    /// When `due` next has something to send, or `None` when nothing is pending.
    pub fn next_deadline(&self) -> Option<Instant> {
        let discrete = self.discrete.iter().map(|d| d.pushed_at);
        let coalesced = self
            .slots
            .iter()
            .filter_map(|(key, slot)| self.ready_at(key, slot));
        discrete.chain(coalesced).min()
    }

    /// True when no event is waiting to be sent.
    pub fn is_empty(&self) -> bool {
        self.discrete.is_empty() && self.slots.values().all(|slot| slot.pending.is_none())
    }
}

/// U+2028 and U+2029 are valid in JSON but end a line in older JS parsers.
fn js_literal(value: &Value) -> String {
    value
        .to_string()
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// The one script a flush evaluates in the page.
pub fn emit_batch_js(batch: &[(String, Value)]) -> String {
    let pairs = Value::Array(
        batch
            .iter()
            .map(|(event, payload)| {
                Value::Array(vec![Value::from(event.as_str()), payload.clone()])
            })
            .collect(),
    );
    format!("window.__calm.emitBatch({})", js_literal(&pairs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn push(c: &mut Coalescer, event: &str, panel: Option<u64>, payload: Value, now: Instant) {
        c.push(event, panel.map(PanelId), payload, classify(event), now);
    }

    #[test]
    fn latest_wins_per_panel_within_a_window() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        push(&mut c, "panelChanged", Some(1), json!({"x": 0}), t0);
        assert_eq!(c.due(t0), vec![("panelChanged".into(), json!({"x": 0}))]);

        for x in 1..=5 {
            push(
                &mut c,
                "panelChanged",
                Some(1),
                json!({"x": x}),
                t0 + ms(10 * x),
            );
            push(
                &mut c,
                "panelChanged",
                Some(2),
                json!({"y": x}),
                t0 + ms(10 * x),
            );
        }
        // Panel 1 is inside its window; panel 2 never sent, so it goes at once.
        assert_eq!(
            c.due(t0 + ms(60)),
            vec![("panelChanged".into(), json!({"y": 5}))]
        );
        assert_eq!(c.due(t0 + ms(99)), vec![]);
        assert_eq!(
            c.due(t0 + ms(100)),
            vec![("panelChanged".into(), json!({"x": 5}))]
        );
    }

    #[test]
    fn trailing_value_is_sent_after_the_stream_goes_quiet() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        let mut sent = Vec::new();
        for step in 0..=30u64 {
            let now = t0 + ms(step * 5);
            push(&mut c, "cameraChanged", None, json!({"zoom": step}), now);
            sent.extend(c.due(now));
        }
        // The stream stopped at 150 ms. Keep polling at the deadlines the host would use.
        while let Some(at) = c.next_deadline() {
            sent.extend(c.due(at));
        }
        assert_eq!(
            sent.last(),
            Some(&("cameraChanged".into(), json!({"zoom": 30})))
        );
        assert!(c.is_empty());
        assert_eq!(c.next_deadline(), None);
    }

    #[test]
    fn removal_drops_the_pending_change_for_that_panel_only() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        push(
            &mut c,
            "panelChanged",
            Some(1),
            json!({"id": 1, "x": 1}),
            t0,
        );
        push(
            &mut c,
            "panelChanged",
            Some(2),
            json!({"id": 2, "x": 1}),
            t0,
        );
        push(&mut c, "panelRemoved", Some(1), json!({"id": 1}), t0);
        // A change that arrives while the removal is still queued is dropped too.
        push(
            &mut c,
            "panelChanged",
            Some(1),
            json!({"id": 1, "x": 2}),
            t0,
        );
        assert_eq!(
            c.due(t0),
            vec![
                ("panelChanged".into(), json!({"id": 2, "x": 1})),
                ("panelRemoved".into(), json!({"id": 1})),
            ]
        );
        assert!(c.is_empty());
    }

    #[test]
    fn discrete_events_are_never_merged_and_keep_their_order() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        push(&mut c, "cameraChanged", None, json!(0), t0);
        let _ = c.due(t0);
        push(&mut c, "panelAdded", Some(1), json!({"id": 1}), t0 + ms(1));
        push(&mut c, "cameraChanged", None, json!(1), t0 + ms(2));
        push(&mut c, "command", None, json!({"command": "a"}), t0 + ms(3));
        push(&mut c, "cameraChanged", None, json!(2), t0 + ms(4));
        push(&mut c, "command", None, json!({"command": "a"}), t0 + ms(5));
        push(&mut c, "panelAdded", Some(1), json!({"id": 1}), t0 + ms(6));
        // The camera is still inside its window, so only the discrete four go, in order.
        assert_eq!(
            c.due(t0 + ms(10)),
            vec![
                ("panelAdded".into(), json!({"id": 1})),
                ("command".into(), json!({"command": "a"})),
                ("command".into(), json!({"command": "a"})),
                ("panelAdded".into(), json!({"id": 1})),
            ]
        );
        assert_eq!(
            c.due(t0 + ms(100)),
            vec![("cameraChanged".into(), json!(2))]
        );
    }

    #[test]
    fn classify_matches_the_spec_table() {
        for event in DISCRETE_EVENTS {
            assert_eq!(classify(event), EventKind::Discrete, "{event}");
        }
        for event in [
            "panelChanged",
            "panelFocused",
            "cameraChanged",
            "appSettingsChanged",
            "terminalOutput",
            "terminalBell",
            "pluginsChanged",
            "resize",
        ] {
            assert_eq!(classify(event), EventKind::Coalesced, "{event}");
        }
    }

    #[test]
    fn set_window_clamps_and_discrete_events_ignore_it() {
        let mut c = Coalescer::new(ms(1));
        assert_eq!(c.window("cameraChanged"), MIN_WINDOW);
        c.set_window("cameraChanged", ms(0));
        assert_eq!(c.window("cameraChanged"), ms(16));
        c.set_window("cameraChanged", ms(60_000));
        assert_eq!(c.window("cameraChanged"), ms(5000));
        c.set_window("cameraChanged", ms(250));
        assert_eq!(c.window("cameraChanged"), ms(250));
        c.set_window("panelAdded", ms(250));
        assert!(!c.windows.contains_key("panelAdded"));
        assert_eq!(Coalescer::new(ms(10_000)).window("x"), MAX_WINDOW);
    }

    /// Counts the perf budget claims over a simulated one second drag, one push per ms.
    fn flushes_per_second(window: Duration) -> Vec<Instant> {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        c.set_window("cameraChanged", window);
        let mut sends = Vec::new();
        for step in 0..1000u64 {
            let now = t0 + ms(step);
            push(&mut c, "cameraChanged", None, json!(step), now);
            if !c.due(now).is_empty() {
                sends.push(now);
            }
        }
        sends
    }

    #[test]
    fn a_window_caps_the_flush_rate() {
        for (window, max) in [(16, 63), (100, 10)] {
            let sends = flushes_per_second(ms(window));
            assert!(sends.len() <= max, "{window} ms: {} flushes", sends.len());
            assert!(
                sends.len() >= max - 1,
                "{window} ms: {} flushes",
                sends.len()
            );
            for pair in sends.windows(2) {
                assert!(
                    pair[1] - pair[0] >= ms(window),
                    "{window} ms: gap {:?}",
                    pair[1] - pair[0]
                );
            }
        }
    }

    #[test]
    fn next_deadline_tracks_pending_work() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(DEFAULT_WINDOW);
        assert_eq!(c.next_deadline(), None);
        assert!(c.is_empty());
        push(&mut c, "cameraChanged", None, json!(0), t0);
        assert_eq!(c.next_deadline(), Some(t0));
        let _ = c.due(t0);
        assert_eq!(c.next_deadline(), None, "nothing pending after a send");
        push(&mut c, "cameraChanged", None, json!(1), t0 + ms(30));
        assert_eq!(c.next_deadline(), Some(t0 + ms(100)));
        assert!(!c.is_empty());
    }

    #[test]
    fn emit_batch_js_escapes_line_separators() {
        let js = emit_batch_js(&[("x".into(), json!("a\u{2028}b\u{2029}c"))]);
        assert_eq!(
            js,
            "window.__calm.emitBatch([[\"x\",\"a\\u2028b\\u2029c\"]])"
        );
        assert!(!js.contains('\u{2028}') && !js.contains('\u{2029}'));
    }

    /// Runs the batch script under bun with a stub `emitBatch` and checks the page sees the
    /// exact values. Set CALM_SKIP_BUN_TESTS=1 to explicitly skip.
    #[test]
    fn emit_batch_js_round_trips_through_a_js_engine() {
        use std::process::Command;
        if std::env::var("CALM_SKIP_BUN_TESTS").as_deref() == Ok("1") {
            return;
        }
        let batch = vec![
            (
                "panelChanged".to_string(),
                json!({"title": "q\"uote' back\\slash </script> \u{2028}\u{2029}", "ctl": "\u{0}\u{1}\n\r\t\u{1f}"}),
            ),
            ("we\"ird\u{2028}".to_string(), json!([1, null, true, "ż"])),
        ];
        let script = format!(
            "var got;\nglobalThis.window = {{ __calm: {{ emitBatch: function (b) {{ got = b; }} }} }};\n{};\nconsole.log(JSON.stringify(got));\n",
            emit_batch_js(&batch)
        );
        let dir = std::env::temp_dir().join(format!("calm-emit-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.js");
        std::fs::write(&path, script).unwrap();
        let output = Command::new("bun")
            .arg(&path)
            .current_dir(&dir)
            .output()
            .unwrap_or_else(|error| {
                panic!("bun is required; install it or set CALM_SKIP_BUN_TESTS=1: {error}")
            });
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            output.status.success(),
            "bun failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let got: Value =
            serde_json::from_str(stdout.lines().last().expect("bun printed nothing")).unwrap();
        let expected: Value = batch
            .iter()
            .map(|(event, payload)| json!([event, payload]))
            .collect();
        assert_eq!(got, expected);
    }
}
