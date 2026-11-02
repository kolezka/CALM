//! Behavior tests for the first-party plain HTML pages, run under bun.
//! Set CALM_SKIP_BUN_TESTS=1 to explicitly skip them.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

fn run_page_test(script: &str, page: &str) {
    if std::env::var("CALM_SKIP_BUN_TESTS").as_deref() == Ok("1") {
        return;
    }
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = crate_dir.join("tests").join(script);
    let page = crate_dir.join("../..").join(page);
    let output = Command::new("bun")
        .arg(&script)
        .arg(&page)
        .output()
        .unwrap_or_else(|error| {
            if error.kind() == ErrorKind::NotFound {
                panic!("bun is required; install bun or set CALM_SKIP_BUN_TESTS=1 to explicitly skip: {error}");
            }
            panic!("failed to run bun on {}: {error}", script.display());
        });
    assert!(
        output.status.success(),
        "{} failed ({}):\n{}{}",
        script.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn calm_keys_page_follows_keys_list() {
    run_page_test("calm_keys_page.js", "plugins/calm-keys/index.html");
}
