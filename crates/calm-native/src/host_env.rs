//! Removes the launching terminal's identity from CALM's own environment.

use calm_core::host_env::inherited_terminal_vars;

/// Unsets every variable `calm_core::host_env` names, so no terminal, plugin or helper
/// CALM spawns inherits them. Returns the names removed.
///
/// Call first thing in `main`, before any thread starts.
pub fn scrub_inherited_terminal_env() -> Vec<String> {
    let names = inherited_terminal_vars(
        std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()),
    );
    for name in &names {
        // SAFETY: `main` calls this before GPUI or anything else starts a thread, so no
        // other thread can read the environment while it changes.
        unsafe { std::env::remove_var(name) };
    }
    names
}
