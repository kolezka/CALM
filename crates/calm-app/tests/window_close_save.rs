//! Closing the canvas window must write a settings save and a session save that are still
//! waiting on their debounce. Runs the opt-in `window_close_save` smoke step, then decodes the
//! files the run kept. It opens a real window, so it is ignored by default:
//! `cargo test -p calm-app --test window_close_save -- --ignored`.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Must match the values the smoke step writes in `main.rs`.
const OPACITY: f64 = 0.37;
const PANEL_X: f64 = 1234.0;

/// Removes the kept smoke dir however the test ends.
struct KeptHome(PathBuf);

impl Drop for KeptHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "opens the CALM window; run with --ignored in a GUI session"]
fn pending_saves_survive_closing_the_canvas_window() {
    let output = Command::new(env!("CARGO_BIN_EXE_calm-app"))
        .arg("--smoke")
        .env("CALM_SMOKE_ONLY", "window_close_save")
        .env("CALM_SMOKE_KEEP_HOME", "1")
        .output()
        .expect("run calm-app --smoke");
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    println!("calm-app exit status: {:?}", output.status.code());

    let home = stdout
        .lines()
        .find_map(|line| line.strip_prefix("smoke: kept home="))
        .map(|dir| KeptHome(PathBuf::from(dir)))
        .expect("the smoke printed its kept home");
    assert!(
        stdout.contains("smoke: window_close_save pending settings=true session=true"),
        "setup: both saves must be pending before the close"
    );
    assert!(
        stdout.contains("smoke: window_close_save closed=true"),
        "setup: the step must close the window"
    );
    assert!(
        stdout.contains("smoke: window_close_save view released"),
        "control: the close must release the canvas view"
    );

    let settings = calm_core::settings::load_settings_from(&home.0.join("settings.json"))
        .expect("settings.json decodes");
    assert_eq!(
        settings.grid.opacity, OPACITY,
        "settings.json lacks the change made before the close"
    );

    // Autosave never writes an untouched scene, so only the close can create this file.
    let session = calm_core::layout::load_layout_from(Path::new(&home.0), "session");
    let xs: Vec<f64> = match &session {
        Ok(layout) => layout.panels.iter().map(|p| p.rect.origin.x).collect(),
        Err(err) => panic!("session.json lacks the move made before the close: {err:?}"),
    };
    assert!(
        xs.contains(&PANEL_X),
        "session.json lacks the move made before the close: panel x {xs:?}"
    );
}
