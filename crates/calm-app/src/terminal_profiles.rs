//! Plain terminals opened from the `terminal` settings: start directory, launch options,
//! Ghostty colors (shared with TUI panels) and image icons from the settings folder.

use std::path::{Path, PathBuf};

use calm_core::terminal_profile::{CwdMode, ResolvedTerminal, TerminalColors, launch_argv};
use calm_core::tui;
use gpui_libghostty::{TerminalColor, TerminalConfiguration, TerminalOptions, TerminalTheme};

use crate::terminal;

/// What CALM remembers about one plain terminal after it started.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalMeta {
    /// The profile asked for. `None` uses the default profile, read again on every change.
    pub profile: Option<String>,
    /// For `{n}`.
    pub index: u64,
    /// Where it started. `{cwd}` and the next "inherit" read it.
    pub cwd: PathBuf,
    /// The colors the surface has now. `None`: whatever its configuration gave it.
    pub applied_colors: Option<TerminalColors>,
    /// The configuration it started with, for smoke: default, user, user+override, custom.
    pub configuration: &'static str,
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// `$SHELL`, else zsh, the macOS default. Apps started from Finder may lack `$SHELL`.
pub fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string())
}

/// The start directory for `mode`. "inherit" without a known focused terminal is home.
pub fn start_dir(mode: &CwdMode, inherited: Option<&Path>, home: &Path) -> PathBuf {
    match mode {
        CwdMode::Home => home.to_path_buf(),
        CwdMode::Inherit => inherited.map_or_else(|| home.to_path_buf(), Path::to_path_buf),
        CwdMode::Fixed(path) => PathBuf::from(tui::expand_tilde(path, &home.to_string_lossy())),
    }
}

/// Launch options for a plain terminal. A `command` stored on the panel wins over the
/// profile's; the env and args then do not apply.
pub fn options(
    resolved: &ResolvedTerminal,
    panel_command: Option<&str>,
    cwd: PathBuf,
) -> TerminalOptions {
    let command = match panel_command {
        Some(command) => command.to_string(),
        None => tui::command_line(&launch_argv(resolved, &login_shell())),
    };
    let mut options = terminal::options(Some(&command), Some(cwd));
    options.configuration = if resolved.use_user_config {
        TerminalConfiguration::UserDefault
    } else {
        TerminalConfiguration::Default
    };
    if let Some(colors) = &resolved.colors {
        apply_colors(&mut options, colors);
    }
    options
}

pub fn theme(colors: &TerminalColors) -> TerminalTheme {
    TerminalTheme::new(
        terminal_color(colors.background),
        terminal_color(colors.foreground),
        colors.palette.map(terminal_color),
    )
}

/// Lays `colors` over whatever config `options` loads: the user's Ghostty config when it is
/// on, else Ghostty's defaults. Call after the base configuration is picked.
pub fn apply_colors(options: &mut TerminalOptions, colors: &TerminalColors) {
    let theme = theme(colors);
    options.configuration = match options.configuration {
        TerminalConfiguration::UserDefault | TerminalConfiguration::UserDefaultWithOverride(_) => {
            TerminalConfiguration::UserDefaultWithOverride(theme)
        }
        TerminalConfiguration::Default | TerminalConfiguration::Custom(_) => {
            TerminalConfiguration::Custom(theme)
        }
    };
}

pub fn configuration_name(configuration: &TerminalConfiguration) -> &'static str {
    match configuration {
        TerminalConfiguration::Default => "default",
        TerminalConfiguration::UserDefault => "user",
        TerminalConfiguration::UserDefaultWithOverride(_) => "user+override",
        TerminalConfiguration::Custom(_) => "custom",
    }
}

/// 0xRRGGBB to a Ghostty color.
fn terminal_color(rgb: u32) -> TerminalColor {
    TerminalColor::new((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// An image icon's file, which must exist inside `settings_dir` after symlinks resolve.
pub fn icon_file(settings_dir: &Path, relative: &Path) -> Result<PathBuf, String> {
    let path = settings_dir.join(relative);
    let real = path
        .canonicalize()
        .map_err(|err| format!("terminal icon {}: {err}", path.display()))?;
    let dir = settings_dir
        .canonicalize()
        .map_err(|err| format!("settings folder {}: {err}", settings_dir.display()))?;
    if !real.starts_with(&dir) {
        return Err(format!(
            "terminal icon {} resolves outside {}",
            path.display(),
            dir.display()
        ));
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_dir_follows_the_mode() {
        let home = Path::new("/Users/me");
        assert_eq!(start_dir(&CwdMode::Home, Some(Path::new("/x")), home), home);
        assert_eq!(start_dir(&CwdMode::Inherit, None, home), home);
        assert_eq!(
            start_dir(&CwdMode::Inherit, Some(Path::new("/x")), home),
            Path::new("/x")
        );
        assert_eq!(
            start_dir(&CwdMode::Fixed("~/src".into()), None, home),
            Path::new("/Users/me/src")
        );
    }

    #[test]
    fn icons_outside_the_settings_folder_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("calm");
        std::fs::create_dir_all(settings.join("icons")).unwrap();
        std::fs::write(settings.join("icons/a.png"), b"x").unwrap();
        std::fs::write(root.path().join("outside.png"), b"x").unwrap();
        std::os::unix::fs::symlink(root.path().join("outside.png"), settings.join("link.png"))
            .unwrap();
        assert!(icon_file(&settings, Path::new("icons/a.png")).is_ok());
        let escaped = icon_file(&settings, Path::new("link.png")).unwrap_err();
        assert!(escaped.contains("resolves outside"), "{escaped}");
        assert!(icon_file(&settings, Path::new("missing.png")).is_err());
    }
}
