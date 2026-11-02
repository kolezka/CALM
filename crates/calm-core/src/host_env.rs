//! Environment variables CALM must not pass on from the terminal that launched it.
//!
//! Started from iTerm2, CALM inherits `ITERM_SESSION_ID` and friends, and every canvas
//! terminal would too. Tools that ask "which terminal am I in?" then act on the wrong
//! one: Claude Code agent teams split the iTerm2 tab CALM was launched from.

/// Exact names that identify the launching terminal or multiplexer session.
const EXACT: &[&str] = &[
    "TERM_SESSION_ID",
    "TMUX",
    "TMUX_PANE",
    "STY",
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_LISTEN_ON",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "ZELLIJ_PANE_ID",
];

/// Prefixes for families of such names.
const PREFIXES: &[&str] = &["ITERM_", "LC_TERMINAL", "TERM_PROGRAM"];

/// Whether `name` describes the terminal CALM was launched from.
pub fn is_inherited_terminal_var(name: &str) -> bool {
    EXACT.contains(&name) || PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The names in `names` to remove before CALM spawns anything.
pub fn inherited_terminal_vars<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    names
        .into_iter()
        .filter(|name| is_inherited_terminal_var(name.as_ref()))
        .map(|name| name.as_ref().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterm2_identity_is_removed() {
        // Claude Code treats either of the first two as "running in iTerm2".
        for name in [
            "ITERM_SESSION_ID",
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "ITERM_PROFILE",
            "ITERM_INJECT_SHELL_INTEGRATION",
            "LC_TERMINAL",
            "LC_TERMINAL_VERSION",
            "TERM_SESSION_ID",
        ] {
            assert!(is_inherited_terminal_var(name), "{name} must be removed");
        }
    }

    #[test]
    fn multiplexer_identity_is_removed() {
        for name in [
            "TMUX",
            "TMUX_PANE",
            "STY",
            "ZELLIJ",
            "WEZTERM_PANE",
            "KITTY_WINDOW_ID",
        ] {
            assert!(is_inherited_terminal_var(name), "{name} must be removed");
        }
    }

    #[test]
    fn ordinary_and_ghostty_vars_stay() {
        for name in [
            "PATH",
            "HOME",
            "SHELL",
            "TERM",
            "COLORTERM",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "GHOSTTY_RESOURCES_DIR",
            "CALM_HOME",
            "TMUX_TMPDIR",
            "TMUXP_CONFIGDIR",
        ] {
            assert!(!is_inherited_terminal_var(name), "{name} must stay");
        }
    }

    #[test]
    fn filter_keeps_only_terminal_names_in_order() {
        let names = ["PATH", "ITERM_SESSION_ID", "HOME", "TMUX", "LC_TERMINAL"];
        assert_eq!(
            inherited_terminal_vars(names),
            vec!["ITERM_SESSION_ID", "TMUX", "LC_TERMINAL"]
        );
    }
}
