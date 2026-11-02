use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::value::RawValue;

use crate::camera::Camera;
use crate::chrome::{BUILTIN_ICON_PREFIX, PanelIcon};
use crate::geometry::{Point, Rect, Size};
use crate::resize::ResizePolicy;
use crate::terminal_profile::{
    MAX_TITLE_CHARS, PanelTerminalColors, check_color, check_content_opacity, check_icon,
    check_text,
};

pub const MIN_PANEL_SIZE: Size = Size::new(120.0, 80.0);
/// Height of the title strip at the top of every panel, in world units.
pub const CHROME_HEIGHT: f64 = 28.0;
pub const MINIMIZED_CHIP: Size = Size::new(200.0, 28.0);
/// How far a duplicate sits from its source, in world units. A multiple of the snap grid.
pub const DUPLICATE_OFFSET: f64 = 32.0;
/// Screen points of a pinned borderless panel's top edge that stay inside the viewport.
pub const PINNED_BORDERLESS_BAND: f64 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PanelId(pub u64);

/// Panel kinds as layouts store them. A `type` this build does not know loads as
/// `Unsupported` and keeps its JSON bytes, so a newer file survives a save.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelKind {
    Terminal {
        command: Option<String>,
        cwd: Option<PathBuf>,
        /// Settings profile id the terminal was opened with. `None` means the defaults.
        profile: Option<String>,
        /// The TUI app running in it. An app takes its command from the manifest, so
        /// `command` and `cwd` stay `None` and are never saved for it.
        app: Option<TerminalApp>,
    },
    Browser {
        url: String,
        /// Page zoom chosen by the user. Not applied yet; `None` means the default.
        page_zoom: Option<f64>,
    },
    Plugin {
        plugin_id: String,
        /// Manifest version the panel was saved with. `None` for v1 files.
        plugin_version: Option<String>,
        settings: serde_json::Value,
    },
    /// Drawn as a placeholder and saved back unchanged.
    Unsupported { raw: RawKind },
}

/// A TUI plugin attached to a terminal. Saved as `type=tui` with only the id and version,
/// so layouts never hold the command, its args or its env.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalApp {
    pub plugin_id: String,
    pub plugin_version: Option<String>,
}

/// The exact JSON bytes of a panel kind with an unknown `type`.
#[derive(Clone, Debug)]
pub struct RawKind {
    raw: Box<RawValue>,
    type_name: String,
}

impl RawKind {
    pub fn json(&self) -> &str {
        self.raw.get()
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }
}

impl PartialEq for RawKind {
    fn eq(&self, other: &Self) -> bool {
        self.raw.get() == other.raw.get()
    }
}

const KNOWN_KIND_TYPES: [&str; 4] = ["terminal", "browser", "plugin", "tui"];

/// The serde shape of the known kinds. Kept private so `PanelKind` can route unknown
/// types to `Unsupported` without going through `serde_json::Value`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KnownKind {
    Terminal {
        command: Option<String>,
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    Browser {
        url: String,
        #[serde(default)]
        page_zoom: Option<f64>,
    },
    Plugin {
        plugin_id: String,
        #[serde(default)]
        plugin_version: Option<String>,
        settings: serde_json::Value,
    },
    Tui {
        plugin_id: String,
        #[serde(default)]
        plugin_version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
}

#[derive(Deserialize)]
struct KindTypeProbe {
    #[serde(rename = "type")]
    type_name: String,
}

impl Serialize for PanelKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let known = match self.clone() {
            PanelKind::Terminal {
                profile,
                app: Some(app),
                ..
            } => KnownKind::Tui {
                plugin_id: app.plugin_id,
                plugin_version: app.plugin_version,
                profile,
            },
            PanelKind::Terminal {
                command,
                cwd,
                profile,
                app: None,
            } => KnownKind::Terminal {
                command,
                cwd,
                profile,
            },
            PanelKind::Browser { url, page_zoom } => KnownKind::Browser { url, page_zoom },
            PanelKind::Plugin {
                plugin_id,
                plugin_version,
                settings,
            } => KnownKind::Plugin {
                plugin_id,
                plugin_version,
                settings,
            },
            PanelKind::Unsupported { raw } => return raw.raw.serialize(serializer),
        };
        known.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PanelKind {
    /// Needs a serde_json text deserializer: `RawValue` cannot come from a `Value`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        let probe: KindTypeProbe = serde_json::from_str(raw.get()).map_err(de::Error::custom)?;
        if !KNOWN_KIND_TYPES.contains(&probe.type_name.as_str()) {
            return Ok(PanelKind::Unsupported {
                raw: RawKind {
                    raw,
                    type_name: probe.type_name,
                },
            });
        }
        // A known type with a bad shape stays an error.
        let known: KnownKind = serde_json::from_str(raw.get()).map_err(de::Error::custom)?;
        Ok(match known {
            KnownKind::Terminal {
                command,
                cwd,
                profile,
            } => PanelKind::Terminal {
                command,
                cwd,
                profile,
                app: None,
            },
            KnownKind::Browser { url, page_zoom } => PanelKind::Browser { url, page_zoom },
            KnownKind::Plugin {
                plugin_id,
                plugin_version,
                settings,
            } => PanelKind::Plugin {
                plugin_id,
                plugin_version,
                settings,
            },
            KnownKind::Tui {
                plugin_id,
                plugin_version,
                profile,
            } => PanelKind::Terminal {
                command: None,
                cwd: None,
                profile,
                app: Some(TerminalApp {
                    plugin_id,
                    plugin_version,
                }),
            },
        })
    }
}

/// What the user chose for a panel's look. Always wins over what the panel sets.
/// Loading checks only `content_opacity`; `validate` is for whoever writes them.
/// Unknown keys are ignored so an older CALM opens a file with more override fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 1 to 4 characters, `builtin:<name>` for a `chrome::PanelIcon`, or a .png or .svg
    /// path relative to the settings folder, such as `panel-icons/<hash>.png`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// "#rrggbb" or "#rrggbbaa".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// "#rrggbb" or "#rrggbbaa".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_bar_color: Option<String>,
    /// "#rrggbb" or "#rrggbbaa".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_color: Option<String>,
    /// Terminals only. Each color is set or inherited on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_colors: Option<PanelTerminalColors>,
    /// Terminals only. 0.2 to 1: how opaque the terminal's content is.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::terminal_profile::deserialize_content_opacity"
    )]
    pub content_opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub borderless: Option<bool>,
}

impl PanelOverrides {
    pub fn is_empty(&self) -> bool {
        *self == PanelOverrides::default()
    }

    /// The first bad field, named as `overrides.<field>`. Terminal-only fields on another
    /// kind are an error; an app's terminal counts as a terminal.
    pub fn validate(&self, kind: &PanelKind) -> Result<(), String> {
        if let Some(title) = &self.title {
            check_text("overrides.title", title, MAX_TITLE_CHARS)?;
        }
        if let Some(icon) = &self.icon {
            if icon.starts_with(BUILTIN_ICON_PREFIX) {
                if PanelIcon::from_override(icon).is_none() {
                    return Err(format!(
                        "\"overrides.icon\" is {icon:?}: no such built-in icon"
                    ));
                }
            } else {
                check_icon("overrides.icon", icon)?;
            }
        }
        check_color("overrides.accent_color", &self.accent_color)?;
        check_color("overrides.title_bar_color", &self.title_bar_color)?;
        check_color("overrides.title_color", &self.title_color)?;
        if let Some(colors) = &self.terminal_colors {
            if !kind.is_terminal() {
                return Err(
                    "\"overrides.terminal_colors\" applies only to terminal panels".to_string(),
                );
            }
            colors.validate("overrides.terminal_colors")?;
        }
        self.validate_content_opacity(kind)
    }

    /// The `content_opacity` part of `validate`. Loading a layout runs this one alone.
    pub(crate) fn validate_content_opacity(&self, kind: &PanelKind) -> Result<(), String> {
        if self.content_opacity.is_some() {
            if !kind.is_terminal() {
                return Err(
                    "\"overrides.content_opacity\" applies only to terminal panels".to_string(),
                );
            }
            check_content_opacity("overrides.content_opacity", self.content_opacity)?;
        }
        Ok(())
    }
}

impl PanelKind {
    /// A plain terminal: the profile's command (the login shell by default).
    pub fn terminal() -> Self {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    /// A terminal running a typed command instead of the profile's.
    pub fn command_terminal(command: impl Into<String>) -> Self {
        PanelKind::Terminal {
            command: Some(command.into()),
            cwd: None,
            profile: None,
            app: None,
        }
    }

    /// A terminal running TUI plugin `plugin_id`.
    pub fn app_terminal(plugin_id: impl Into<String>, plugin_version: Option<String>) -> Self {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: Some(TerminalApp {
                plugin_id: plugin_id.into(),
                plugin_version,
            }),
        }
    }

    /// Every terminal, apps included.
    pub fn is_terminal(&self) -> bool {
        matches!(self, PanelKind::Terminal { .. })
    }

    /// The app a terminal runs, if it is an app panel.
    pub fn app(&self) -> Option<&TerminalApp> {
        match self {
            PanelKind::Terminal { app, .. } => app.as_ref(),
            _ => None,
        }
    }

    fn default_title(&self) -> String {
        match self {
            PanelKind::Terminal { app: Some(app), .. } => app.plugin_id.clone(),
            PanelKind::Terminal { .. } => "Terminal".to_string(),
            PanelKind::Browser { url, .. } => url.clone(),
            PanelKind::Plugin { plugin_id, .. } => plugin_id.clone(),
            PanelKind::Unsupported { raw } => format!("Unsupported: {}", raw.type_name()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub id: PanelId,
    pub kind: PanelKind,
    /// World coordinates, chrome strip included. For a pinned panel, coordinates of
    /// `Scene::camera_of` it, so `rect.origin * pin` is its screen origin.
    pub rect: Rect,
    pub title: String,
    pub minimized: bool,
    pub overrides: PanelOverrides,
    /// Not saved in layouts. Set from the kind on add; a plugin's manifest can change it through
    /// `Scene::set_resize_policy`.
    pub resize: ResizePolicy,
    /// The canvas zoom when the panel was pinned. `None` for a panel that pans and zooms.
    pub pin: Option<f64>,
}

impl Panel {
    pub fn is_pinned(&self) -> bool {
        self.pin.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    /// Back to front: the last panel is topmost. Pinned panels all come after unpinned
    /// ones. Change panels through `Scene` methods or
    /// `get_mut`, so the revision moves.
    pub panels: Vec<Panel>,
    /// Direct writes are fine: `sync_revision` notices a changed camera.
    pub camera: Camera,
    next_id: u64,
    /// Bumped by every change a saved layout would show. Autosave compares it.
    revision: u64,
    /// The camera as of the last `sync_revision`.
    camera_seen: Camera,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            panels: Vec::new(),
            camera: Camera::default(),
            next_id: 1,
            revision: 0,
            camera_seen: Camera::default(),
        }
    }
}

/// Equal content is equal; the revision bookkeeping is not content.
impl PartialEq for Scene {
    fn eq(&self, other: &Self) -> bool {
        self.panels == other.panels && self.camera == other.camera && self.next_id == other.next_id
    }
}

impl PanelKind {
    /// Fills `plugin_version` of a plugin or TUI kind from `installed`, which maps a plugin
    /// id to its installed manifest version. Other kinds come back unchanged.
    pub fn with_installed_version(self, installed: impl Fn(&str) -> Option<String>) -> Self {
        match self {
            PanelKind::Plugin {
                plugin_id,
                settings,
                ..
            } => PanelKind::Plugin {
                plugin_version: installed(&plugin_id),
                plugin_id,
                settings,
            },
            PanelKind::Terminal {
                command,
                cwd,
                profile,
                app: Some(app),
            } => PanelKind::Terminal {
                command,
                cwd,
                profile,
                app: Some(TerminalApp {
                    plugin_version: installed(&app.plugin_id),
                    plugin_id: app.plugin_id,
                }),
            },
            other => other,
        }
    }
}

impl Scene {
    /// The current revision, without folding in a camera written since the last sync.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Counts a camera change made through the public field, then returns the revision.
    /// Autosave calls this before it compares revisions.
    pub fn sync_revision(&mut self) -> u64 {
        if self.camera != self.camera_seen {
            self.camera_seen = self.camera;
            self.bump();
        }
        self.revision
    }

    /// Marks a change that went around the `Scene` methods.
    pub fn touch(&mut self) {
        self.bump();
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Sets a panel's title. Only a real change moves the revision, so a program that
    /// repeats its title does not cause saves.
    pub fn set_title(&mut self, id: PanelId, title: String) -> bool {
        let changed = self.get(id).is_some_and(|panel| panel.title != title);
        if changed && let Some(panel) = self.get_mut(id) {
            panel.title = title;
        }
        changed
    }

    /// Adds a panel on top of the unpinned tier. Ids are never reused, even after a remove.
    pub fn add(&mut self, kind: PanelKind, rect: Rect) -> PanelId {
        self.add_with_overrides(kind, rect, PanelOverrides::default())
    }

    /// `add` with saved overrides installed, for restore and duplicate. A borderless
    /// panel has no chrome strip, so its height minimum is the content minimum.
    pub fn add_with_overrides(
        &mut self,
        kind: PanelKind,
        rect: Rect,
        overrides: PanelOverrides,
    ) -> PanelId {
        self.bump();
        let id = PanelId(self.next_id);
        self.next_id += 1;
        let min_height = if overrides.borderless == Some(true) {
            MIN_PANEL_SIZE.height - CHROME_HEIGHT
        } else {
            MIN_PANEL_SIZE.height
        };
        let rect = Rect {
            origin: rect.origin,
            size: Size::new(
                rect.size.width.max(MIN_PANEL_SIZE.width),
                rect.size.height.max(min_height),
            ),
        };
        let title = kind.default_title();
        let resize = ResizePolicy::derive(&kind, None);
        let index = self.first_pinned_index();
        self.panels.insert(
            index,
            Panel {
                id,
                kind,
                rect,
                title,
                minimized: false,
                overrides,
                resize,
                pin: None,
            },
        );
        id
    }

    /// Applies a panel's resize policy. A resizable panel's size moves into the new
    /// limits at once; a locked panel keeps the size it was created with.
    pub fn set_resize_policy(&mut self, id: PanelId, policy: ResizePolicy) {
        // `get_mut` counts this as a change: the size may move.
        if let Some(panel) = self.get_mut(id) {
            panel.resize = policy;
            if policy.resizable {
                panel.rect.size = policy.clamp(panel.rect.size);
            }
        }
    }

    pub fn remove(&mut self, id: PanelId) -> Option<Panel> {
        let index = self.index_of(id)?;
        self.bump();
        Some(self.panels.remove(index))
    }

    pub fn get(&self, id: PanelId) -> Option<&Panel> {
        self.panels.iter().find(|panel| panel.id == id)
    }

    /// Any mutable access counts as a change, so no field write can skip autosave.
    /// Read through `get` when nothing changes.
    pub fn get_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        let index = self.index_of(id)?;
        self.bump();
        self.panels.get_mut(index)
    }

    /// Moves a panel to the top of its tier.
    pub fn bring_to_front(&mut self, id: PanelId) {
        if let Some(index) = self.index_of(id) {
            let panel = self.panels.remove(index);
            let target = if panel.is_pinned() {
                self.panels.len()
            } else {
                self.first_pinned_index()
            };
            self.reinsert(index, target, panel);
        }
    }

    /// Moves a panel to the bottom of its tier.
    pub fn send_to_back(&mut self, id: PanelId) {
        if let Some(index) = self.index_of(id) {
            let panel = self.panels.remove(index);
            let target = if panel.is_pinned() {
                self.first_pinned_index()
            } else {
                0
            };
            self.reinsert(index, target, panel);
        }
    }

    /// Puts a panel taken from `from` back at `to`, bumping only when the order changed.
    fn reinsert(&mut self, from: usize, to: usize, panel: Panel) {
        self.panels.insert(to, panel);
        if to != from {
            self.bump();
        }
    }

    /// The camera a panel is drawn and hit with: the canvas camera, or for a pinned
    /// panel a fixed camera at its pin zoom.
    pub fn camera_of(&self, panel: &Panel) -> Camera {
        match panel.pin {
            Some(zoom) => pin_camera(zoom),
            None => self.camera,
        }
    }

    /// Pins or unpins a panel without moving it on screen. Its size never changes.
    /// The panel goes to the top of its new tier. Returns true if it changed.
    pub fn set_pinned(&mut self, id: PanelId, pinned: bool) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        if self.panels[index].is_pinned() == pinned {
            return false;
        }
        let mut panel = self.panels.remove(index);
        let screen = self.camera_of(&panel).world_to_screen(panel.rect.origin);
        if pinned {
            let zoom = self.camera.zoom;
            panel.pin = Some(zoom);
            panel.rect.origin = pin_camera(zoom).screen_to_world(screen);
            self.panels.push(panel);
        } else {
            panel.pin = None;
            panel.rect.origin = self.camera.screen_to_world(screen);
            let target = self.first_pinned_index();
            self.panels.insert(target, panel);
        }
        self.bump();
        true
    }

    /// Moves each pinned panel so its title strip (its chip when minimized, a top band
    /// when borderless) is fully inside `viewport`. Returns true if any moved.
    pub fn clamp_pinned(&mut self, viewport: Size) -> bool {
        let mut moved = false;
        for panel in self.panels.iter_mut() {
            let Some(zoom) = panel.pin else {
                continue;
            };
            let keep = pinned_keep_rect(panel, zoom);
            let x = clamp_axis(keep.origin.x, keep.size.width, viewport.width);
            let y = clamp_axis(keep.origin.y, keep.size.height, viewport.height);
            if x != keep.origin.x || y != keep.origin.y {
                panel.rect.origin.x += (x - keep.origin.x) / zoom;
                panel.rect.origin.y += (y - keep.origin.y) / zoom;
                moved = true;
            }
        }
        if moved {
            self.bump();
        }
        moved
    }

    fn first_pinned_index(&self) -> usize {
        self.panels
            .iter()
            .position(Panel::is_pinned)
            .unwrap_or(self.panels.len())
    }

    /// Adds a copy of `id` on top, shifted by `DUPLICATE_OFFSET`, never minimized.
    /// The kind, size and overrides are copied; a duplicated terminal gets a fresh process.
    /// `installed` gives a plugin's installed manifest version, stamped on the copy.
    pub fn duplicate(
        &mut self,
        id: PanelId,
        installed: impl Fn(&str) -> Option<String>,
    ) -> Option<PanelId> {
        let source = self.get(id)?;
        let kind = source.kind.clone().with_installed_version(installed);
        let rect = Rect {
            origin: Point::new(
                source.rect.origin.x + DUPLICATE_OFFSET,
                source.rect.origin.y + DUPLICATE_OFFSET,
            ),
            size: source.rect.size,
        };
        let overrides = source.overrides.clone();
        let pin = source.pin;
        let copy = self.add_with_overrides(kind, rect, overrides);
        if let Some(zoom) = pin
            && let Some(index) = self.index_of(copy)
        {
            let mut panel = self.panels.remove(index);
            panel.pin = Some(zoom);
            self.panels.push(panel);
        }
        Some(copy)
    }

    /// Topmost panel under the screen point, each panel tested in its own camera.
    /// A minimized panel only hits its chip.
    pub fn hit_test_screen(&self, screen: Point) -> Option<PanelId> {
        self.panels
            .iter()
            .rev()
            .find(|panel| hit_rect(panel).contains(self.camera_of(panel).screen_to_world(screen)))
            .map(|panel| panel.id)
    }

    /// Smallest world rect covering every unpinned panel as drawn (a minimized panel is
    /// its chip). `None` when there is none.
    pub fn bounds(&self) -> Option<Rect> {
        self.panels
            .iter()
            .filter(|panel| !panel.is_pinned())
            .map(|panel| {
                if panel.minimized {
                    chip_rect(panel)
                } else {
                    panel.rect
                }
            })
            .reduce(|a, b| a.union(&b))
    }

    fn index_of(&self, id: PanelId) -> Option<usize> {
        self.panels.iter().position(|panel| panel.id == id)
    }
}

fn pin_camera(zoom: f64) -> Camera {
    Camera {
        offset: Point::new(0.0, 0.0),
        zoom,
    }
}

fn hit_rect(panel: &Panel) -> Rect {
    if panel.minimized {
        chip_rect(panel)
    } else {
        panel.rect
    }
}

/// The screen rect of a pinned panel that `clamp_pinned` keeps inside the viewport.
fn pinned_keep_rect(panel: &Panel, zoom: f64) -> Rect {
    let camera = pin_camera(zoom);
    if panel.minimized {
        return camera.world_rect_to_screen(chip_rect(panel));
    }
    let strip = chrome_height(panel);
    let mut keep = camera.world_rect_to_screen(Rect {
        origin: panel.rect.origin,
        size: Size::new(panel.rect.size.width, strip),
    });
    if strip == 0.0 {
        keep.size.height = PINNED_BORDERLESS_BAND.min(panel.rect.size.height * zoom);
    }
    keep
}

/// Start of a span of `len` moved into `0..limit`; the start wins when it cannot fit.
fn clamp_axis(start: f64, len: f64, limit: f64) -> f64 {
    start.min(limit - len).max(0.0)
}

pub fn chip_rect(panel: &Panel) -> Rect {
    Rect {
        origin: panel.rect.origin,
        size: MINIMIZED_CHIP,
    }
}

/// Height of the strip above the content: 0 for a borderless panel.
pub fn chrome_height(panel: &Panel) -> f64 {
    if panel.overrides.borderless == Some(true) {
        0.0
    } else {
        CHROME_HEIGHT
    }
}

/// Turns borderless on or off, moving the outer rect so `content_rect` stays put.
/// Off clears the field rather than storing `false`.
pub fn set_borderless(panel: &mut Panel, on: bool) {
    let was = chrome_height(panel) == 0.0;
    panel.overrides.borderless = on.then_some(true);
    if on && !was {
        panel.rect.origin.y += CHROME_HEIGHT;
        panel.rect.size.height = (panel.rect.size.height - CHROME_HEIGHT).max(0.0);
    } else if !on && was {
        panel.rect.origin.y -= CHROME_HEIGHT;
        panel.rect.size.height += CHROME_HEIGHT;
    }
}

pub fn content_rect(panel: &Panel) -> Rect {
    let bar = chrome_height(panel);
    Rect::new(
        panel.rect.origin.x,
        panel.rect.origin.y + bar,
        panel.rect.size.width,
        (panel.rect.size.height - bar).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type Change = Box<dyn FnOnce(&mut Scene, PanelId, PanelId)>;

    /// Runs `change` on a fresh two-panel scene and reports whether the revision moved.
    fn bumps(change: impl FnOnce(&mut Scene, PanelId, PanelId)) -> bool {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 100.0));
        let front = scene.add(terminal(), Rect::new(300.0, 0.0, 200.0, 100.0));
        let before = scene.sync_revision();
        change(&mut scene, back, front);
        scene.sync_revision() != before
    }

    #[test]
    fn toggling_borderless_keeps_the_content_rect() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(10.0, 20.0, 300.0, 200.0));
        let panel = scene.get_mut(id).unwrap();
        let before = content_rect(panel);
        set_borderless(panel, true);
        assert_eq!(panel.overrides.borderless, Some(true));
        assert_eq!(chrome_height(panel), 0.0);
        assert_eq!(content_rect(panel), before);
        set_borderless(panel, false);
        assert_eq!(panel.overrides.borderless, None);
        assert_eq!(chrome_height(panel), CHROME_HEIGHT);
        assert_eq!(content_rect(panel), before);
        assert_eq!(panel.rect, Rect::new(10.0, 20.0, 300.0, 200.0));
    }

    #[test]
    fn every_persisted_mutation_bumps_the_revision() {
        let browser = PanelKind::Browser {
            url: "https://example.com".to_string(),
            page_zoom: None,
        };
        let cases: Vec<(&str, Change)> = vec![
            (
                "add",
                Box::new(|s, _, _| {
                    let _ = s.add(terminal(), Rect::new(0.0, 0.0, 1.0, 1.0));
                }),
            ),
            (
                "remove",
                Box::new(|s, back, _| {
                    let _ = s.remove(back);
                }),
            ),
            (
                "duplicate",
                Box::new(|s, back, _| {
                    let _ = s.duplicate(back, |_| None);
                }),
            ),
            (
                "bring_to_front",
                Box::new(|s, back, _| s.bring_to_front(back)),
            ),
            (
                "send_to_back",
                Box::new(|s, _, front| s.send_to_back(front)),
            ),
            (
                "move",
                Box::new(|s, back, _| s.get_mut(back).unwrap().rect.origin.x = 5.0),
            ),
            (
                "resize",
                Box::new(|s, back, _| s.get_mut(back).unwrap().rect.size.width = 400.0),
            ),
            (
                "minimize",
                Box::new(|s, back, _| s.get_mut(back).unwrap().minimized = true),
            ),
            (
                "title",
                Box::new(|s, back, _| {
                    let _ = s.set_title(back, "new".to_string());
                }),
            ),
            (
                "kind",
                Box::new(move |s, back, _| s.get_mut(back).unwrap().kind = browser),
            ),
            (
                "overrides",
                Box::new(|s, back, _| {
                    s.get_mut(back).unwrap().overrides.title = Some("mine".to_string())
                }),
            ),
            ("camera zoom", Box::new(|s, _, _| s.camera.zoom = 2.0)),
            (
                "camera pan",
                Box::new(|s, _, _| s.camera.pan_by_screen(10.0, 0.0)),
            ),
            ("touch", Box::new(|s, _, _| s.touch())),
            (
                "pin",
                Box::new(|s, back, _| {
                    let _ = s.set_pinned(back, true);
                }),
            ),
            (
                "clamp_pinned",
                Box::new(|s, back, _| {
                    s.set_pinned(back, true);
                    s.sync_revision();
                    s.get_mut(back).unwrap().rect.origin.x = -9000.0;
                    s.sync_revision();
                    // Only the clamp is left to bump here.
                    let before = s.revision();
                    assert!(s.clamp_pinned(Size::new(800.0, 600.0)));
                    assert_ne!(s.revision(), before);
                }),
            ),
        ];
        for (name, change) in cases {
            assert!(bumps(change), "{name} did not bump the revision");
        }
    }

    #[test]
    fn reads_and_no_op_changes_keep_the_revision() {
        let cases: Vec<(&str, Change)> = vec![
            (
                "get",
                Box::new(|s, back, _| {
                    let _ = s.get(back);
                }),
            ),
            (
                "hit_test_screen",
                Box::new(|s, _, _| {
                    let _ = s.hit_test_screen(Point::new(1.0, 1.0));
                }),
            ),
            (
                "front already front",
                Box::new(|s, _, front| s.bring_to_front(front)),
            ),
            (
                "back already back",
                Box::new(|s, back, _| s.send_to_back(back)),
            ),
            (
                "same title",
                Box::new(|s, back, _| {
                    let _ = s.set_title(back, "Terminal".to_string());
                }),
            ),
            (
                "missing id",
                Box::new(|s, _, _| {
                    let _ = s.get_mut(PanelId(99));
                }),
            ),
            (
                "camera written unchanged",
                Box::new(|s, _, _| {
                    s.camera = Camera { ..s.camera };
                }),
            ),
        ];
        for (name, change) in cases {
            assert!(!bumps(change), "{name} bumped the revision");
        }
    }

    #[test]
    fn scene_equality_ignores_the_revision() {
        let mut moved = Scene::default();
        moved.touch();
        assert_eq!(moved, Scene::default());
    }

    #[test]
    fn duplicate_stamps_the_installed_version_on_plugin_and_tui_copies() {
        let installed = |id: &str| (id == "notes").then(|| "2.0.0".to_string());
        let mut scene = Scene::default();
        let rect = Rect::new(0.0, 0.0, 300.0, 200.0);
        let tui = scene.add(
            PanelKind::app_terminal("notes", Some("1.0.0".to_string())),
            rect,
        );
        let plugin = scene.add(
            PanelKind::Plugin {
                plugin_id: "notes".to_string(),
                plugin_version: Some("1.0.0".to_string()),
                settings: serde_json::json!({ "a": 1 }),
            },
            rect,
        );
        let tui_copy = scene.duplicate(tui, installed).unwrap();
        assert_eq!(
            scene.get(tui_copy).unwrap().kind,
            PanelKind::app_terminal("notes", Some("2.0.0".to_string()))
        );
        let plugin_copy = scene.duplicate(plugin, installed).unwrap();
        assert_eq!(
            scene.get(plugin_copy).unwrap().kind,
            PanelKind::Plugin {
                plugin_id: "notes".to_string(),
                plugin_version: Some("2.0.0".to_string()),
                settings: serde_json::json!({ "a": 1 }),
            }
        );
        // The source keeps the version it was saved with.
        assert_eq!(
            scene.get(tui).unwrap().kind,
            PanelKind::app_terminal("notes", Some("1.0.0".to_string()))
        );
    }

    #[test]
    fn with_installed_version_fills_plugin_and_tui_kinds_only() {
        let installed = |id: &str| (id == "notes").then(|| "1.2.3".to_string());
        let plugin = PanelKind::Plugin {
            plugin_id: "notes".to_string(),
            plugin_version: None,
            settings: serde_json::json!({ "a": 1 }),
        };
        assert_eq!(
            plugin.with_installed_version(installed),
            PanelKind::Plugin {
                plugin_id: "notes".to_string(),
                plugin_version: Some("1.2.3".to_string()),
                settings: serde_json::json!({ "a": 1 }),
            }
        );
        let tui = PanelKind::app_terminal("notes", None);
        assert_eq!(
            tui.with_installed_version(installed),
            PanelKind::app_terminal("notes", Some("1.2.3".to_string()))
        );
        let missing = PanelKind::app_terminal("gone", Some("stale".to_string()));
        assert_eq!(
            missing.with_installed_version(installed),
            PanelKind::app_terminal("gone", None)
        );
        assert_eq!(terminal().with_installed_version(installed), terminal());
    }

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    #[test]
    fn terminal_profile_roundtrips_and_is_omitted_when_none() {
        let with = PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: Some("neon".into()),
            app: None,
        };
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["profile"], "neon", "{json}");
        let back: PanelKind = serde_json::from_str(&json.to_string()).unwrap();
        assert_eq!(back, with);

        let json = serde_json::to_value(terminal()).unwrap();
        assert!(json.get("profile").is_none(), "{json}");
        let back: PanelKind = serde_json::from_str(&json.to_string()).unwrap();
        assert_eq!(back, terminal());
    }

    #[test]
    fn validate_rejects_a_malformed_single_terminal_color() {
        use crate::terminal_profile::{ColorSlot, PanelTerminalColors};
        let mut colors = PanelTerminalColors::default();
        colors.set(ColorSlot::Palette(5), Some(0x123456));
        let good = PanelOverrides {
            terminal_colors: Some(colors.clone()),
            ..PanelOverrides::default()
        };
        assert_eq!(good.validate(&terminal()), Ok(()));
        colors.palette[5] = Some("#12345".into());
        let bad = PanelOverrides {
            terminal_colors: Some(colors),
            ..PanelOverrides::default()
        };
        let error = bad.validate(&terminal()).unwrap_err();
        assert!(error.contains("palette[5]"), "{error}");
        let bad_background = PanelOverrides {
            terminal_colors: Some(PanelTerminalColors {
                background: Some("red".into()),
                ..PanelTerminalColors::default()
            }),
            ..PanelOverrides::default()
        };
        let error = bad_background.validate(&terminal()).unwrap_err();
        assert!(error.contains("background"), "{error}");
    }

    #[test]
    fn validate_rejects_terminal_fields_on_a_browser() {
        let browser = PanelKind::Browser {
            url: "https://example.com".into(),
            page_zoom: None,
        };
        let colors = PanelOverrides {
            terminal_colors: Some(crate::terminal_profile::PanelTerminalColors::from_colors(
                &crate::terminal_profile::PALETTE_PRESETS[0].colors,
            )),
            ..PanelOverrides::default()
        };
        // Positive control: the same overrides are fine on a terminal.
        assert_eq!(colors.validate(&terminal()), Ok(()));
        let error = colors.validate(&browser).unwrap_err();
        assert!(error.contains("terminal_colors"), "{error}");

        let shared = PanelOverrides {
            title: Some("Docs".into()),
            borderless: Some(true),
            title_bar_color: Some("#000000".into()),
            ..PanelOverrides::default()
        };
        assert_eq!(shared.validate(&browser), Ok(()));
    }

    #[test]
    fn validate_rejects_bad_color_and_long_icon() {
        let good = PanelOverrides {
            icon: Some("abcd".into()),
            title_color: Some("#123456".into()),
            ..PanelOverrides::default()
        };
        assert_eq!(good.validate(&terminal()), Ok(()));

        let bad_color = PanelOverrides {
            title_color: Some("#12".into()),
            ..good.clone()
        };
        let error = bad_color.validate(&terminal()).unwrap_err();
        assert!(error.contains("title_color"), "{error}");

        let long_icon = PanelOverrides {
            icon: Some("abcde".into()),
            ..good.clone()
        };
        let error = long_icon.validate(&terminal()).unwrap_err();
        assert!(error.contains("icon"), "{error}");

        let image_icon = PanelOverrides {
            icon: Some("icons/logo.svg".into()),
            ..good
        };
        assert_eq!(image_icon.validate(&terminal()), Ok(()));
    }

    #[test]
    fn add_assigns_increasing_ids_and_topmost_last() {
        let mut scene = Scene::default();
        let first = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let second = scene.add(terminal(), Rect::new(50.0, 50.0, 200.0, 200.0));
        assert!(second.0 > first.0);
        let order: Vec<PanelId> = scene.panels.iter().map(|panel| panel.id).collect();
        assert_eq!(order, vec![first, second]);
    }

    #[test]
    fn hit_test_prefers_topmost_overlap() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        assert_eq!(scene.hit_test_screen(Point::new(150.0, 150.0)), Some(front));
        assert_eq!(scene.hit_test_screen(Point::new(50.0, 50.0)), Some(back));
        assert_eq!(scene.hit_test_screen(Point::new(1000.0, 1000.0)), None);
    }

    #[test]
    fn bring_to_front_reorders() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.bring_to_front(back);
        let order: Vec<PanelId> = scene.panels.iter().map(|panel| panel.id).collect();
        assert_eq!(order, vec![front, back]);
        assert_eq!(scene.hit_test_screen(Point::new(150.0, 150.0)), Some(back));
    }

    #[test]
    fn send_to_back_reorders() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.send_to_back(front);
        let order: Vec<PanelId> = scene.panels.iter().map(|panel| panel.id).collect();
        assert_eq!(order, vec![front, back]);
        assert_eq!(scene.hit_test_screen(Point::new(150.0, 150.0)), Some(back));
        scene.send_to_back(PanelId(99));
        assert_eq!(scene.panels.len(), 2);
    }

    #[test]
    fn duplicate_copies_overrides() {
        let mut scene = Scene::default();
        let source = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: Some("neon".into()),
                app: None,
            },
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let overrides = PanelOverrides {
            title: Some("build {n}".into()),
            accent_color: Some("#ff0000".into()),
            borderless: Some(true),
            ..PanelOverrides::default()
        };
        scene.get_mut(source).unwrap().overrides = overrides.clone();
        let copy = scene.duplicate(source, |_| None).unwrap();
        let panel = scene.get(copy).unwrap();
        assert_eq!(panel.overrides, overrides);
        assert!(matches!(
            &panel.kind,
            PanelKind::Terminal { profile: Some(id), .. } if id == "neon"
        ));
        assert_eq!(panel.rect.size, Size::new(300.0, 200.0));
    }

    #[test]
    fn add_with_overrides_keeps_a_small_borderless_size_and_clamps_the_rest() {
        let borderless = PanelOverrides {
            borderless: Some(true),
            ..PanelOverrides::default()
        };
        let content_min = MIN_PANEL_SIZE.height - CHROME_HEIGHT;
        let mut scene = Scene::default();
        let small = Rect::new(0.0, 0.0, MIN_PANEL_SIZE.width, content_min);
        let id = scene.add_with_overrides(terminal(), small, borderless.clone());
        let panel = scene.get(id).unwrap();
        assert_eq!(panel.rect, small);
        assert_eq!(panel.overrides, borderless);
        // Undersized files are still clamped, borderless to the content minimum.
        let id = scene.add_with_overrides(terminal(), Rect::new(0.0, 0.0, 10.0, 10.0), borderless);
        assert_eq!(
            scene.get(id).unwrap().rect.size,
            Size::new(MIN_PANEL_SIZE.width, content_min)
        );
        let id = scene.add_with_overrides(terminal(), small, PanelOverrides::default());
        assert_eq!(scene.get(id).unwrap().rect.size, MIN_PANEL_SIZE);
    }

    #[test]
    fn duplicate_keeps_size_of_a_small_borderless_panel() {
        let mut scene = Scene::default();
        let source = scene.add(
            terminal(),
            Rect::new(0.0, 0.0, MIN_PANEL_SIZE.width, MIN_PANEL_SIZE.height),
        );
        set_borderless(scene.get_mut(source).unwrap(), true);
        let copy = scene.duplicate(source, |_| None).unwrap();
        let source = scene.get(source).unwrap();
        let copy = scene.get(copy).unwrap();
        assert_eq!(copy.rect.size, source.rect.size);
        assert_eq!(content_rect(copy).size, content_rect(source).size);
    }

    #[test]
    fn duplicate_copies_kind_and_size_offset_on_top() {
        let mut scene = Scene::default();
        let kind = PanelKind::Terminal {
            command: Some("htop".into()),
            cwd: Some(PathBuf::from("/tmp")),
            profile: None,
            app: None,
        };
        let source = scene.add(kind.clone(), Rect::new(10.0, 20.0, 300.0, 200.0));
        let _other = scene.add(terminal(), Rect::new(900.0, 0.0, 200.0, 200.0));
        scene.get_mut(source).unwrap().minimized = true;
        let copy = scene.duplicate(source, |_| None).unwrap();
        assert_ne!(copy, source);
        assert_eq!(scene.panels.last().map(|panel| panel.id), Some(copy));
        let panel = scene.get(copy).unwrap();
        assert_eq!(panel.kind, kind);
        assert_eq!(
            panel.rect,
            Rect::new(
                10.0 + DUPLICATE_OFFSET,
                20.0 + DUPLICATE_OFFSET,
                300.0,
                200.0
            )
        );
        assert!(!panel.minimized);
        assert!(scene.get(source).unwrap().minimized);
        assert_eq!(scene.duplicate(PanelId(99), |_| None), None);
    }

    #[test]
    fn minimized_panel_hits_only_chip() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        if let Some(panel) = scene.get_mut(id) {
            panel.minimized = true;
        }
        assert_eq!(scene.hit_test_screen(Point::new(10.0, 10.0)), Some(id));
        assert_eq!(scene.hit_test_screen(Point::new(199.0, 27.0)), Some(id));
        assert_eq!(scene.hit_test_screen(Point::new(10.0, 100.0)), None);
        assert_eq!(scene.hit_test_screen(Point::new(300.0, 10.0)), None);
        let panel = scene.get(id).unwrap();
        assert_eq!(chip_rect(panel), Rect::new(0.0, 0.0, 200.0, 28.0));
    }

    #[test]
    fn remove_returns_panel_and_shrinks() {
        let mut scene = Scene::default();
        let first = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let second = scene.add(
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        let removed = scene.remove(first).unwrap();
        assert_eq!(removed.id, first);
        assert_eq!(scene.panels.len(), 1);
        assert_eq!(scene.panels[0].id, second);
        assert!(scene.remove(first).is_none());
        assert!(scene.get(first).is_none());
    }

    #[test]
    fn add_enforces_min_size() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(5.0, 6.0, 10.0, 10.0));
        let panel = scene.get(id).unwrap();
        assert_eq!(panel.rect, Rect::new(5.0, 6.0, 120.0, 80.0));
        assert_eq!(MIN_PANEL_SIZE, Size::new(120.0, 80.0));
        assert_eq!(
            content_rect(panel),
            Rect::new(5.0, 6.0 + CHROME_HEIGHT, 120.0, 80.0 - CHROME_HEIGHT)
        );
    }

    #[test]
    fn bounds_covers_panels_and_uses_chip_when_minimized() {
        let mut scene = Scene::default();
        assert_eq!(scene.bounds(), None);
        scene.add(terminal(), Rect::new(-100.0, -50.0, 300.0, 200.0));
        let wide = scene.add(terminal(), Rect::new(400.0, 300.0, 1000.0, 500.0));
        assert_eq!(
            scene.bounds(),
            Some(Rect::new(-100.0, -50.0, 1500.0, 850.0))
        );
        scene
            .panels
            .iter_mut()
            .find(|p| p.id == wide)
            .unwrap()
            .minimized = true;
        assert_eq!(scene.bounds(), Some(Rect::new(-100.0, -50.0, 700.0, 378.0)));
    }

    fn kind_round_trip(json: &str) -> String {
        let kind: PanelKind = serde_json::from_str(json).unwrap();
        serde_json::to_string(&kind).unwrap()
    }

    #[test]
    fn a_saved_tui_kind_round_trips_byte_for_byte() {
        for json in [
            r#"{"type":"tui","plugin_id":"top-example","plugin_version":"0.1.0"}"#,
            r#"{"type":"tui","plugin_id":"top-example","plugin_version":null}"#,
        ] {
            assert_eq!(kind_round_trip(json), json);
        }
    }

    #[test]
    fn a_saved_tui_kind_keeps_its_profile() {
        let json =
            r#"{"type":"tui","plugin_id":"top-example","plugin_version":"0.1.0","profile":"dark"}"#;
        assert_eq!(kind_round_trip(json), json);
    }

    #[test]
    fn an_app_panel_never_writes_command_or_cwd() {
        // Even a malformed app kind with a command and cwd saves only the app.
        let kind = PanelKind::Terminal {
            command: Some("rm -rf ~".to_string()),
            cwd: Some(PathBuf::from("/tmp")),
            profile: None,
            app: Some(TerminalApp {
                plugin_id: "top-example".to_string(),
                plugin_version: None,
            }),
        };
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            r#"{"type":"tui","plugin_id":"top-example","plugin_version":null}"#
        );
        let back: PanelKind = serde_json::from_str(&serde_json::to_string(&kind).unwrap()).unwrap();
        assert_eq!(back, PanelKind::app_terminal("top-example", None));
    }

    #[test]
    fn terminal_colors_overrides_are_valid_on_app_panels() {
        let overrides = PanelOverrides {
            terminal_colors: Some(PanelTerminalColors {
                background: Some("#102030".to_string()),
                ..PanelTerminalColors::default()
            }),
            ..PanelOverrides::default()
        };
        assert_eq!(
            overrides.validate(&PanelKind::app_terminal("top-example", None)),
            Ok(())
        );
        let browser = PanelKind::Browser {
            url: "about:blank".to_string(),
            page_zoom: None,
        };
        assert!(overrides.validate(&browser).is_err());
    }

    #[test]
    fn content_opacity_out_of_range_is_rejected_with_its_path() {
        let with = |value| PanelOverrides {
            content_opacity: Some(value),
            ..PanelOverrides::default()
        };
        for bad in [0.19, 1.01, f64::NAN] {
            let error = with(bad).validate(&terminal()).unwrap_err();
            assert!(
                error.contains("overrides.content_opacity"),
                "{bad}: {error}"
            );
        }
        assert_eq!(
            with(0.19).validate(&terminal()),
            Err("\"overrides.content_opacity\" is 0.19, expected 0.2 to 1".to_string())
        );
        for good in [0.2, 1.0] {
            assert_eq!(with(good).validate(&terminal()), Ok(()), "{good}");
        }
    }

    #[test]
    fn content_opacity_on_a_browser_is_rejected() {
        let browser = PanelKind::Browser {
            url: "https://example.com".into(),
            page_zoom: None,
        };
        let overrides = PanelOverrides {
            content_opacity: Some(0.5),
            ..PanelOverrides::default()
        };
        assert_eq!(overrides.validate(&terminal()), Ok(()));
        assert_eq!(
            overrides.validate(&browser),
            Err("\"overrides.content_opacity\" applies only to terminal panels".to_string())
        );
    }

    #[test]
    fn content_opacity_on_an_app_terminal_is_accepted() {
        let overrides = PanelOverrides {
            content_opacity: Some(0.5),
            ..PanelOverrides::default()
        };
        let app = PanelKind::Terminal {
            app: Some(TerminalApp {
                plugin_id: "top-example".into(),
                plugin_version: None,
            }),
            command: None,
            cwd: None,
            profile: None,
        };
        assert_eq!(overrides.validate(&app), Ok(()));
    }

    fn ids(scene: &Scene) -> Vec<PanelId> {
        scene.panels.iter().map(|panel| panel.id).collect()
    }

    /// Every pinned panel comes after every unpinned one.
    fn assert_tiers(scene: &Scene) {
        let first_pinned = scene.panels.iter().position(Panel::is_pinned);
        if let Some(first) = first_pinned {
            assert!(
                scene.panels[first..].iter().all(Panel::is_pinned),
                "unpinned panel above a pinned one: {:?}",
                ids(scene)
            );
        }
    }

    fn screen_rect(scene: &Scene, id: PanelId) -> Rect {
        let panel = scene.get(id).unwrap();
        scene.camera_of(panel).world_rect_to_screen(panel.rect)
    }

    #[test]
    fn pin_keeps_screen_rect_through_pan_and_zoom() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(300.0, 200.0, 400.0, 300.0));
        scene.camera = Camera {
            offset: Point::new(100.0, 50.0),
            zoom: 2.0,
        };
        let before = screen_rect(&scene, id);
        assert!(scene.set_pinned(id, true));
        assert!(scene.get(id).unwrap().is_pinned());
        assert_eq!(scene.get(id).unwrap().pin, Some(2.0));
        assert_eq!(screen_rect(&scene, id), before);
        scene.camera.pan_by_screen(-250.0, 80.0);
        scene.camera.zoom_by(0.5, Point::new(40.0, 40.0));
        assert_eq!(screen_rect(&scene, id), before);
        assert!(!scene.set_pinned(id, true), "pinning twice is no change");
    }

    #[test]
    fn unpin_after_zoom_change_keeps_screen_origin() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(300.0, 200.0, 400.0, 300.0));
        scene.camera = Camera {
            offset: Point::new(100.0, 50.0),
            zoom: 2.0,
        };
        scene.set_pinned(id, true);
        let pinned_origin = screen_rect(&scene, id).origin;
        scene.camera = Camera {
            offset: Point::new(-40.0, 10.0),
            zoom: 0.5,
        };
        assert!(scene.set_pinned(id, false));
        assert_eq!(scene.get(id).unwrap().pin, None);
        assert_eq!(scene.camera_of(scene.get(id).unwrap()), scene.camera);
        let after = screen_rect(&scene, id).origin;
        assert!((after.x - pinned_origin.x).abs() < 1e-9);
        assert!((after.y - pinned_origin.y).abs() < 1e-9);
        assert!(!scene.set_pinned(id, false), "unpinning twice is no change");
    }

    #[test]
    fn pin_and_unpin_never_change_size() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(10.0, 20.0, 410.0, 330.0));
        let size = scene.get(id).unwrap().rect.size;
        scene.camera.zoom = 3.0;
        scene.set_pinned(id, true);
        assert_eq!(scene.get(id).unwrap().rect.size, size);
        scene.camera.zoom = 0.25;
        scene.set_pinned(id, false);
        assert_eq!(scene.get(id).unwrap().rect.size, size);
    }

    #[test]
    fn pinned_panels_stay_above_unpinned() {
        let mut scene = Scene::default();
        let a = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let b = scene.add(terminal(), Rect::new(10.0, 0.0, 200.0, 200.0));
        let c = scene.add(terminal(), Rect::new(20.0, 0.0, 200.0, 200.0));
        assert!(scene.set_pinned(a, true));
        assert_eq!(ids(&scene), vec![b, c, a]);
        assert!(scene.set_pinned(b, true));
        assert_eq!(ids(&scene), vec![c, a, b]);
        let d = scene.add(terminal(), Rect::new(30.0, 0.0, 200.0, 200.0));
        assert_eq!(ids(&scene), vec![c, d, a, b]);
        assert_tiers(&scene);
        scene.bring_to_front(c);
        assert_eq!(ids(&scene), vec![d, c, a, b]);
        scene.send_to_back(b);
        assert_eq!(ids(&scene), vec![d, c, b, a]);
        scene.bring_to_front(b);
        assert_eq!(ids(&scene), vec![d, c, a, b]);
        scene.send_to_back(c);
        assert_eq!(ids(&scene), vec![c, d, a, b]);
        assert_tiers(&scene);
        // Unpin lands on top of the unpinned tier.
        assert!(scene.set_pinned(a, false));
        assert_eq!(ids(&scene), vec![c, d, a, b]);
        assert!(scene.set_pinned(b, false));
        assert_eq!(ids(&scene), vec![c, d, a, b]);
        assert_tiers(&scene);
    }

    #[test]
    fn tier_moves_at_the_tier_edge_keep_the_revision() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let pinned = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        scene.set_pinned(pinned, true);
        let before = scene.sync_revision();
        scene.bring_to_front(back);
        scene.send_to_back(pinned);
        assert_eq!(scene.sync_revision(), before);
        assert_eq!(ids(&scene), vec![back, pinned]);
    }

    #[test]
    fn hit_test_screen_prefers_pinned_over_unpinned_under_it() {
        // The default camera: offset (0, 0), zoom 1.
        let mut scene = Scene::default();
        let pinned = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.set_pinned(pinned, true);
        // Added later, but stays below the pinned panel.
        let under = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(
            scene.hit_test_screen(Point::new(150.0, 150.0)),
            Some(pinned)
        );
        assert_eq!(scene.hit_test_screen(Point::new(50.0, 50.0)), Some(under));
        // Panning moves the unpinned panel away; the pinned one stays.
        scene.camera.pan_by_screen(-1000.0, 0.0);
        assert_eq!(
            scene.hit_test_screen(Point::new(150.0, 150.0)),
            Some(pinned)
        );
        assert_eq!(scene.hit_test_screen(Point::new(50.0, 50.0)), None);
        // A minimized pinned panel hits only its chip.
        scene.get_mut(pinned).unwrap().minimized = true;
        assert_eq!(
            scene.hit_test_screen(Point::new(150.0, 110.0)),
            Some(pinned)
        );
        assert_eq!(scene.hit_test_screen(Point::new(150.0, 250.0)), None);
    }

    #[test]
    fn bounds_ignore_pinned() {
        let mut scene = Scene::default();
        let free = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let pinned = scene.add(terminal(), Rect::new(5000.0, 5000.0, 200.0, 200.0));
        scene.set_pinned(pinned, true);
        assert_eq!(scene.bounds(), Some(scene.get(free).unwrap().rect));
        scene.remove(free);
        assert_eq!(scene.bounds(), None);
    }

    #[test]
    fn clamp_pinned_pulls_offscreen_panel_back() {
        let viewport = Size::new(800.0, 600.0);
        let mut scene = Scene::default();
        scene.camera.zoom = 2.0;
        let right = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 150.0));
        let left = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 150.0));
        let chip = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 150.0));
        let bare = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 150.0));
        let free = scene.add(terminal(), Rect::new(-5000.0, -5000.0, 200.0, 150.0));
        for id in [right, left, chip, bare] {
            scene.set_pinned(id, true);
        }
        // Pinned at zoom 2, so screen = rect * 2.
        scene.get_mut(right).unwrap().rect.origin = Point::new(390.0, 290.0);
        scene.get_mut(left).unwrap().rect.origin = Point::new(-150.0, -40.0);
        scene.get_mut(chip).unwrap().rect.origin = Point::new(395.0, 100.0);
        scene.get_mut(chip).unwrap().minimized = true;
        set_borderless(scene.get_mut(bare).unwrap(), true);
        scene.get_mut(bare).unwrap().rect.origin = Point::new(100.0, 295.0);
        let before = scene.sync_revision();
        assert!(scene.clamp_pinned(viewport));
        assert_ne!(scene.sync_revision(), before);

        // Title strip: 400 x 56 on screen, fully inside.
        let strip = screen_rect(&scene, right);
        assert_eq!(strip.origin, Point::new(400.0, 600.0 - CHROME_HEIGHT * 2.0));
        let strip = screen_rect(&scene, left);
        assert_eq!(strip.origin, Point::new(0.0, 0.0));
        // Chip: 400 x 56 on screen.
        let chip_screen = scene.get(chip).unwrap().rect.origin;
        assert_eq!(chip_screen, Point::new(200.0, 100.0));
        // Borderless: a band of PINNED_BORDERLESS_BAND screen points at the top stays in.
        let bare_screen = screen_rect(&scene, bare);
        assert_eq!(bare_screen.origin.x, 200.0);
        assert_eq!(bare_screen.origin.y, 600.0 - PINNED_BORDERLESS_BAND);
        // Unpinned panels are left alone.
        assert_eq!(
            scene.get(free).unwrap().rect.origin,
            Point::new(-5000.0, -5000.0)
        );

        let settled = scene.sync_revision();
        assert!(!scene.clamp_pinned(viewport));
        assert_eq!(scene.sync_revision(), settled);
    }

    #[test]
    fn duplicate_of_pinned_is_pinned_on_top() {
        let mut scene = Scene::default();
        scene.camera.zoom = 1.5;
        let pinned = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let other = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        scene.set_pinned(pinned, true);
        scene.set_pinned(other, true);
        let free = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let copy = scene.duplicate(pinned, |_| None).unwrap();
        assert_eq!(scene.get(copy).unwrap().pin, Some(1.5));
        assert_eq!(ids(&scene), vec![free, pinned, other, copy]);
        let source = scene.get(pinned).unwrap().rect.origin;
        assert_eq!(
            scene.get(copy).unwrap().rect.origin,
            Point::new(source.x + DUPLICATE_OFFSET, source.y + DUPLICATE_OFFSET)
        );
        let free_copy = scene.duplicate(free, |_| None).unwrap();
        assert_eq!(scene.get(free_copy).unwrap().pin, None);
        assert_eq!(ids(&scene), vec![free, free_copy, pinned, other, copy]);
        assert_tiers(&scene);
    }
}
