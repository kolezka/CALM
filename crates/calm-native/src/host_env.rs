//! Removes the launching terminal's identity from CALM's own environment.

use calm_core::host_env::inherited_terminal_vars;

/// Unsets every variable `calm_core::host_env` names, so no terminal, plugin or helper
/// CALM spawns inherits them. Returns the names removed.
///
/// # Safety
///
/// No other thread may read or write the environment during the call, including native
/// `getenv` calls. Call it first thing in `main`, before CALM starts any thread.
pub unsafe fn scrub_inherited_terminal_env() -> Vec<String> {
    let names = inherited_terminal_vars(
        std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()),
    );
    for name in &names {
        // SAFETY: the caller guarantees no other thread touches the environment now.
        unsafe { std::env::remove_var(name) };
    }
    names
}
