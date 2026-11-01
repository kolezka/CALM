//! Smoke step bookkeeping: the step list, the `CALM_SMOKE_ONLY` filter, per-step results,
//! the known-flake labels and the summary. Pure logic, so it is unit tested here.

use std::collections::BTreeSet;

/// Names the steps to run, comma separated. Required setup steps run too.
pub const ONLY_ENV: &str = "CALM_SMOKE_ONLY";

pub struct StepSpec {
    pub name: &'static str,
    /// Must have passed, or this step is skipped. The filter pulls them in.
    pub requires: &'static [&'static str],
    /// Must have run first (any result). The filter pulls them in.
    pub after: &'static [&'static str],
}

const fn step(
    name: &'static str,
    requires: &'static [&'static str],
    after: &'static [&'static str],
) -> StepSpec {
    StepSpec {
        name,
        requires,
        after,
    }
}

/// Every step, in run order. Each dependency comes earlier in the list.
pub const STEPS: &[StepSpec] = &[
    step("terminals", &[], &[]),
    step("zorder", &["terminals"], &[]),
    step("join", &["terminals"], &[]),
    step("app_icon", &[], &[]),
    step("native_reorder", &[], &[]),
    step("plugins", &[], &[]),
    step("tui", &[], &[]),
    step("show_stats", &[], &[]),
    step("fps", &["show_stats"], &[]),
    step("stats", &["show_stats"], &[]),
    step("preview", &["terminals"], &[]),
    // The cadence window is where the grid and input target counts are taken.
    step("preview_cadence", &["preview"], &[]),
    step("grid", &["preview"], &["preview_cadence"]),
    step("preview_input_targets", &["preview"], &["preview_cadence"]),
    step("live", &["terminals"], &["preview"]),
    step("background", &[], &[]),
    step("browser", &[], &[]),
    step("resize", &[], &[]),
    step("blur", &[], &[]),
    step("settings_dispatch", &[], &[]),
    step("github_api", &[], &[]),
    step("plugin_menus", &[], &[]),
    step("panel_overrides", &["terminals"], &[]),
    step("panel_settings", &[], &[]),
    step("session", &[], &[]),
];

/// Failures seen before and written up in `.ai/conventions.md` ("Smoke flakes").
/// Checked in order, so a specific id wins over `window-hidden`.
/// A flake id and whether a failure message is that flake.
type FlakeRule = (&'static str, fn(&str) -> bool);

const KNOWN_FLAKES: &[FlakeRule] = &[
    ("stats-pill", |m| {
        m.starts_with("stats: sampler has CPU but the status bar does not show it")
    }),
    ("few-frames", |m| {
        m.starts_with("only ") && m.ends_with(" frames with show_stats on")
    }),
    ("tui-exit", |m| m.starts_with("tui: ran=")),
    ("preview-cadence", |m| {
        m.starts_with("preview refresh outside 1..=3 snapshots")
    }),
    ("window-hidden", |m| {
        m.contains("window_visible=Some(false)")
    }),
];

/// `known-flake:<id>` for a documented flake, `new` otherwise. A label never changes
/// whether the step failed.
pub fn classify(message: &str) -> String {
    KNOWN_FLAKES
        .iter()
        .find(|(_, matches)| matches(message))
        .map_or_else(|| "new".to_string(), |(id, _)| format!("known-flake:{id}"))
}

/// The steps to run. `None` (variable unset) runs all; a value names steps, and their
/// `requires` and `after` steps are added. Empty values and unknown names are errors.
pub fn select(only: Option<&str>) -> Result<BTreeSet<&'static str>, String> {
    let Some(raw) = only else {
        return Ok(STEPS.iter().map(|s| s.name).collect());
    };
    let names: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return Err(format!("{ONLY_ENV} is set but names no step"));
    }
    let unknown: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| spec(n).is_none())
        .collect();
    if !unknown.is_empty() {
        let known: Vec<&str> = STEPS.iter().map(|s| s.name).collect();
        return Err(format!(
            "{ONLY_ENV} has unknown steps {unknown:?}; known steps: {}",
            known.join(",")
        ));
    }
    let mut selected = BTreeSet::new();
    let mut stack: Vec<&'static str> = names
        .iter()
        .filter_map(|n| spec(n))
        .map(|s| s.name)
        .collect();
    while let Some(name) = stack.pop() {
        if selected.insert(name)
            && let Some(spec) = spec(name)
        {
            stack.extend(spec.requires.iter().chain(spec.after));
        }
    }
    Ok(selected)
}

fn spec(name: &str) -> Option<&'static StepSpec> {
    STEPS.iter().find(|s| s.name == name)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Pass,
    Fail { reason: String, label: String },
    Skipped(String),
}

pub struct SmokeRun {
    selected: BTreeSet<&'static str>,
    pub filter: Option<String>,
    results: Vec<(&'static str, Outcome)>,
}

impl SmokeRun {
    pub fn new(only: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            selected: select(only)?,
            filter: only.map(str::to_string),
            results: Vec::new(),
        })
    }

    /// The filter line printed before any step runs.
    pub fn filter_line(&self) -> String {
        let names: Vec<&str> = STEPS
            .iter()
            .map(|s| s.name)
            .filter(|n| self.selected.contains(n))
            .collect();
        match &self.filter {
            None => format!("smoke: filter none steps={}", names.len()),
            Some(raw) => format!("smoke: filter {ONLY_ENV}={raw:?} steps={}", names.join(",")),
        }
    }

    /// `None` if filtered out. Otherwise whether to run, or why it is skipped.
    pub fn decide(&self, name: &str) -> Option<Result<(), String>> {
        let spec = spec(name).unwrap_or_else(|| panic!("smoke step {name:?} is not in STEPS"));
        if !self.selected.contains(spec.name) {
            return None;
        }
        for needed in spec.requires {
            match self.outcome(needed) {
                Some(Outcome::Pass) => {}
                Some(Outcome::Fail { .. }) => {
                    return Some(Err(format!("needs {needed}, which failed")));
                }
                Some(Outcome::Skipped(_)) => {
                    return Some(Err(format!("needs {needed}, which was skipped")));
                }
                None => return Some(Err(format!("needs {needed}, which did not run"))),
            }
        }
        Some(Ok(()))
    }

    fn outcome(&self, name: &str) -> Option<&Outcome> {
        self.results
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, o)| o)
    }

    /// Records a result and returns its one line.
    pub fn record(&mut self, name: &str, outcome: Outcome) -> String {
        let spec = spec(name).unwrap_or_else(|| panic!("smoke step {name:?} is not in STEPS"));
        let line = match &outcome {
            Outcome::Pass => format!("smoke: step {name} PASS"),
            Outcome::Fail { reason, label } => format!("smoke: step {name} FAIL {label} {reason}"),
            Outcome::Skipped(why) => format!("smoke: step {name} SKIPPED {why}"),
        };
        self.results.push((spec.name, outcome));
        line
    }

    pub fn fail(&mut self, name: &str, reason: String) -> String {
        let label = classify(&reason);
        self.record(name, Outcome::Fail { reason, label })
    }

    /// The summary block: counts, counts per label, then one line per failure and skip.
    pub fn summary(&self) -> Vec<String> {
        let count =
            |want: fn(&Outcome) -> bool| self.results.iter().filter(|(_, o)| want(o)).count();
        let pass = count(|o| matches!(o, Outcome::Pass));
        let fail = count(|o| matches!(o, Outcome::Fail { .. }));
        let skipped = count(|o| matches!(o, Outcome::Skipped(_)));
        let mut lines = vec![format!(
            "smoke: summary pass={pass} fail={fail} skipped={skipped}"
        )];
        let mut labels: Vec<(&str, usize)> = Vec::new();
        for (_, outcome) in &self.results {
            if let Outcome::Fail { label, .. } = outcome {
                match labels.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, n)) => *n += 1,
                    None => labels.push((label, 1)),
                }
            }
        }
        if !labels.is_empty() {
            let parts: Vec<String> = labels.iter().map(|(l, n)| format!("{l}={n}")).collect();
            lines.push(format!("smoke: summary labels {}", parts.join(" ")));
        }
        for (name, outcome) in &self.results {
            match outcome {
                Outcome::Pass => {}
                Outcome::Fail { reason, label } => {
                    lines.push(format!("smoke: summary FAIL {name} {label} {reason}"));
                }
                Outcome::Skipped(why) => {
                    lines.push(format!("smoke: summary SKIPPED {name} {why}"));
                }
            }
        }
        lines
    }

    /// 0 only when every selected step ran and passed. Known flakes count as failures.
    pub fn exit_code(&self) -> i32 {
        let all_passed = self.results.iter().all(|(_, o)| *o == Outcome::Pass);
        let all_ran = self.selected.len() == self.results.len();
        if all_passed && all_ran { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_unique_and_dependencies_come_first() {
        for (i, step) in STEPS.iter().enumerate() {
            assert_eq!(STEPS.iter().filter(|s| s.name == step.name).count(), 1);
            for dep in step.requires.iter().chain(step.after) {
                let at = STEPS.iter().position(|s| s.name == *dep).expect(dep);
                assert!(at < i, "{} depends on later {dep}", step.name);
            }
        }
    }

    #[test]
    fn classify_labels_each_documented_flake() {
        let cases = [
            (
                "stats: sampler has CPU but the status bar does not show it: \"--% CPU\"",
                "known-flake:stats-pill",
            ),
            ("only 1 frames with show_stats on", "known-flake:few-frames"),
            ("tui: ran=false status=[]", "known-flake:tui-exit"),
            (
                "preview refresh outside 1..=3 snapshots: preview_snapshots_in_3s=[0,0] window_visible=Some(true)",
                "known-flake:preview-cadence",
            ),
            (
                "terminals not live at zoom 1.5: [] window_visible=Some(false)",
                "known-flake:window-hidden",
            ),
        ];
        for (message, want) in cases {
            assert_eq!(classify(message), want, "{message}");
        }
    }

    #[test]
    fn classify_calls_anything_else_new() {
        for message in [
            "native order does not match the scene: zorder=[1, 2] scene=[2, 1]",
            "terminals not live at zoom 1.5: [] window_visible=Some(true)",
            "stats: sampler has 1 samples and cpu=None after 5.0s, CPU needs two",
            "no fps after 11 frames",
            "tui marker: No such file",
            "autosave: no session file: None",
            "",
        ] {
            assert_eq!(classify(message), "new", "{message}");
        }
    }

    #[test]
    fn no_filter_selects_every_step() {
        assert_eq!(select(None).unwrap().len(), STEPS.len());
    }

    #[test]
    fn filter_adds_required_setup() {
        let got = select(Some("session, zorder")).unwrap();
        let want: BTreeSet<&str> = ["session", "zorder", "terminals"].into();
        assert_eq!(got, want);
        let got = select(Some("grid")).unwrap();
        let want: BTreeSet<&str> = ["grid", "preview", "preview_cadence", "terminals"].into();
        assert_eq!(got, want);
    }

    #[test]
    fn filter_rejects_unknown_and_empty() {
        let err = select(Some("zorder,sesion")).unwrap_err();
        assert!(
            err.contains("\"sesion\"") && err.contains("known steps"),
            "{err}"
        );
        assert!(select(Some("")).is_err());
        assert!(select(Some(" , ")).is_err());
    }

    #[test]
    fn filtered_out_steps_do_not_run() {
        let run = SmokeRun::new(Some("session")).unwrap();
        assert_eq!(run.decide("zorder"), None);
        assert_eq!(run.decide("session"), Some(Ok(())));
    }

    #[test]
    fn a_failed_requirement_skips_its_dependents_only() {
        let mut run = SmokeRun::new(None).unwrap();
        run.fail("terminals", "no frame within 10 s: x".into());
        assert_eq!(
            run.decide("zorder"),
            Some(Err("needs terminals, which failed".into()))
        );
        run.record(
            "zorder",
            Outcome::Skipped("needs terminals, which failed".into()),
        );
        assert_eq!(run.decide("app_icon"), Some(Ok(())));
        run.record("show_stats", Outcome::Pass);
        run.fail("fps", "only 1 frames with show_stats on".into());
        // A failed fps does not stop stats: both only need the toggle.
        assert_eq!(run.decide("stats"), Some(Ok(())));
    }

    #[test]
    fn summary_counts_and_lists_failures() {
        let mut run = SmokeRun::new(Some("fps,stats,session")).unwrap();
        run.record("show_stats", Outcome::Pass);
        run.fail("fps", "only 1 frames with show_stats on".into());
        run.fail("stats", "stats: implausible cpu=-1 mem=0MB in \"\"".into());
        run.record("session", Outcome::Pass);
        assert_eq!(
            run.summary(),
            vec![
                "smoke: summary pass=2 fail=2 skipped=0",
                "smoke: summary labels known-flake:few-frames=1 new=1",
                "smoke: summary FAIL fps known-flake:few-frames only 1 frames with show_stats on",
                "smoke: summary FAIL stats new stats: implausible cpu=-1 mem=0MB in \"\"",
            ]
        );
    }

    #[test]
    fn exit_code_is_zero_only_when_everything_passed() {
        let mut run = SmokeRun::new(Some("session")).unwrap();
        assert_eq!(run.exit_code(), 1, "nothing ran yet");
        run.record("session", Outcome::Pass);
        assert_eq!(run.exit_code(), 0);

        let mut run = SmokeRun::new(Some("fps")).unwrap();
        run.record("show_stats", Outcome::Pass);
        run.fail("fps", "only 1 frames with show_stats on".into());
        assert_eq!(run.exit_code(), 1, "a known flake is still a failure");

        let mut run = SmokeRun::new(Some("zorder")).unwrap();
        run.fail("terminals", "x".into());
        run.record(
            "zorder",
            Outcome::Skipped("needs terminals, which failed".into()),
        );
        assert_eq!(run.exit_code(), 1);
    }
}
