//! The per-panel Settings window as data. Each field is inherited or set; a set field
//! lives in `PanelOverrides`, and a reset puts it back to inherited.

use crate::background::Rgb;
use crate::chrome::{BUILTIN_ICON_PREFIX, ChromeOverrides, PanelIcon, lay_overrides};
use crate::markdown::{
    FONT_SIZE_RANGE, MarkdownSettings, PADDING_RANGE, PanelMarkdownStyle, check_font_family,
};
use crate::scene::{Panel, PanelId, PanelKind, PanelOverrides, Scene, set_borderless};
use crate::settings::Settings;
use crate::settings_form::{
    CHROME_SEED, Control, Field, FieldState, FieldValue, FormCommand, FormOutcome, Section, button,
    checkbox, choice, color, content_opacity_value, field, note, rgb_of, section, slider,
};
pub use crate::terminal_profile::ghostty_default_colors;
use crate::terminal_profile::{
    AppLayer, ColorSlot, DEFAULT_CONTENT_OPACITY, MIN_CONTENT_OPACITY, PALETTE_PRESETS,
    PanelTerminalColors, TerminalColors, TerminalIcon, hex,
};

/// What the panel shows now, inherited and set fields alike.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelEffective {
    pub title: String,
    pub icon: Option<String>,
    pub chrome: ChromeOverrides,
    pub colors: Option<TerminalColors>,
    /// A terminal with no colors of its own: Ghostty's config colors it.
    pub live_colors_note: bool,
    /// The running terminal already shows a theme CALM applied. Set by the app.
    pub live_theme: bool,
    pub content_opacity: f64,
    /// A markdown panel's resolved look; `None` for other kinds.
    pub markdown: Option<MarkdownSettings>,
}

/// `app` is an app panel's manifest layer, filled by the app; `None` for other panels.
pub fn panel_effective(
    panel: &Panel,
    settings: &Settings,
    app: Option<&AppLayer>,
) -> PanelEffective {
    let overrides = &panel.overrides;
    let title = overrides
        .title
        .clone()
        .unwrap_or_else(|| panel.title.clone());
    match &panel.kind {
        PanelKind::Terminal { profile, .. } => {
            let resolved = settings.terminal.resolve_for_panel(
                profile.as_deref(),
                settings.panels.terminal_user_config,
                app,
                overrides,
            );
            let inherited_icon = resolved.icon.map(|icon| match icon {
                TerminalIcon::Text(text) => text,
                TerminalIcon::Image(path) | TerminalIcon::AppImage(path) => {
                    path.display().to_string()
                }
            });
            PanelEffective {
                title,
                // A builtin override reads as text to the resolver; take it as stored.
                icon: overrides.icon.clone().or(inherited_icon),
                chrome: resolved.overrides,
                live_colors_note: resolved.colors.is_none(),
                colors: resolved.colors,
                live_theme: false,
                content_opacity: resolved.content_opacity,
                markdown: None,
            }
        }
        _ => PanelEffective {
            title,
            icon: overrides.icon.clone(),
            chrome: lay_overrides(ChromeOverrides::default(), overrides),
            colors: None,
            live_colors_note: false,
            live_theme: false,
            content_opacity: DEFAULT_CONTENT_OPACITY,
            markdown: matches!(panel.kind, PanelKind::Markdown { .. })
                .then(|| settings.markdown.resolve(overrides.markdown.as_ref())),
        },
    }
}

fn state(set: bool) -> FieldState {
    if set {
        FieldState::Set
    } else {
        FieldState::Inherited
    }
}

fn with_state(mut field: Field, set: bool) -> Field {
    field.state = state(set);
    field
}

pub fn panel_form(panel: &Panel, effective: &PanelEffective) -> Vec<Section> {
    let overrides = &panel.overrides;
    let builtin = overrides.icon.as_deref().and_then(PanelIcon::from_override);
    let icon_text = overrides.icon.clone().filter(|_| builtin.is_none());
    let icon_placeholder = effective
        .icon
        .clone()
        .filter(|icon| !icon.starts_with(BUILTIN_ICON_PREFIX))
        .unwrap_or_default();
    let mut icon_names = vec!["Default".to_string()];
    icon_names.extend(PanelIcon::ALL.iter().map(|icon| icon.label().to_string()));
    let builtin_index = builtin
        .and_then(|icon| PanelIcon::ALL.iter().position(|known| *known == icon))
        .map_or(0, |index| index + 1);

    let general = vec![
        with_state(
            text(
                "panel.title",
                "Title",
                overrides.title.clone().unwrap_or_default(),
                effective.title.clone(),
            ),
            overrides.title.is_some(),
        ),
        with_state(
            text(
                "panel.icon",
                "Icon",
                icon_text.clone().unwrap_or_default(),
                icon_placeholder,
            ),
            icon_text.is_some(),
        ),
        button("panel.icon.file", "Icon image", "Choose PNG or SVG\u{2026}"),
        with_state(
            choice(
                "panel.icon.builtin",
                "Built-in icon",
                icon_names,
                builtin_index,
            ),
            builtin.is_some(),
        ),
        with_state(
            checkbox(
                "panel.borderless",
                "Borderless",
                overrides.borderless == Some(true),
            ),
            overrides.borderless.is_some(),
        ),
    ];
    let mut sections = vec![section("Panel", general)];

    if matches!(panel.kind, PanelKind::Terminal { .. }) {
        let well = |id: &str, label: &str, value: Option<u32>, seed: &str, set: bool| {
            let rgb = value.map_or_else(|| rgb_of(Some(seed)), |rgba| Rgb(rgba >> 8));
            with_state(color(id, label, rgb), set)
        };
        let chrome = &effective.chrome;
        sections.push(section(
            "Title bar colors",
            vec![
                well(
                    "panel.accent",
                    "Accent",
                    chrome.accent,
                    CHROME_SEED[0],
                    overrides.accent_color.is_some(),
                ),
                well(
                    "panel.title_bar",
                    "Title bar",
                    chrome.title_bar,
                    CHROME_SEED[1],
                    overrides.title_bar_color.is_some(),
                ),
                well(
                    "panel.title_color",
                    "Title",
                    chrome.title,
                    CHROME_SEED[2],
                    overrides.title_color.is_some(),
                ),
            ],
        ));

        let own = overrides.terminal_colors.as_ref();
        let set = |slot: ColorSlot| own.is_some_and(|own| own.text(slot).is_some());
        let mut colors = Vec::new();
        // With no palette set anywhere, show the seed the live theme fills in.
        let shown = effective.colors.unwrap_or_else(ghostty_default_colors);
        colors.push(with_state(
            color(
                "panel.colors.background",
                "Background",
                Rgb(shown.background),
            ),
            set(ColorSlot::Background),
        ));
        colors.push(with_state(
            color(
                "panel.colors.foreground",
                "Foreground",
                Rgb(shown.foreground),
            ),
            set(ColorSlot::Foreground),
        ));
        for (index, value) in shown.palette.iter().enumerate() {
            colors.push(with_state(
                color(
                    &format!("panel.colors.{index}"),
                    &format!("Color {index}"),
                    Rgb(*value),
                ),
                set(ColorSlot::Palette(index)),
            ));
        }
        if effective.live_colors_note {
            // libghostty cannot drop a theme it was given, only take a new one.
            let text = if effective.live_theme {
                "This terminal keeps its current colors. The change applies to new terminals."
            } else {
                "Colors come from your Ghostty config. Load a theme to give this terminal its \
                 own."
            };
            colors.push(note("panel.colors.note", "", text.to_string()));
        }
        for (index, preset) in PALETTE_PRESETS.iter().enumerate() {
            colors.push(button(
                &format!("panel.preset.{index}"),
                "",
                &format!("Load {}", preset.label),
            ));
        }
        sections.push(section("Terminal colors", colors));
        sections.push(section(
            "Opacity",
            vec![with_state(
                slider(
                    "panel.content_opacity",
                    "Opacity",
                    effective.content_opacity,
                    MIN_CONTENT_OPACITY..=1.0,
                    2,
                ),
                overrides.content_opacity.is_some(),
            )],
        ));
    }

    if let (PanelKind::Markdown { text: source }, Some(style)) = (&panel.kind, &effective.markdown)
    {
        sections.push(section(
            "Markdown",
            vec![field(
                "panel.markdown.text",
                "Text",
                Control::TextArea(source.clone()),
            )],
        ));
        let own = overrides.markdown.clone().unwrap_or_default();
        sections.push(section(
            "Markdown style",
            vec![
                with_state(
                    color(
                        "panel.markdown.background_color",
                        "Background",
                        style.background_color,
                    ),
                    own.background_color.is_some(),
                ),
                with_state(
                    slider(
                        "panel.markdown.background_opacity",
                        "Background opacity",
                        style.background_opacity,
                        0.0..=1.0,
                        2,
                    ),
                    own.background_opacity.is_some(),
                ),
                with_state(
                    color("panel.markdown.text_color", "Text", style.text_color),
                    own.text_color.is_some(),
                ),
                with_state(
                    color("panel.markdown.link_color", "Links", style.link_color),
                    own.link_color.is_some(),
                ),
                with_state(
                    text(
                        "panel.markdown.font_family",
                        "Font family",
                        own.font_family.clone().unwrap_or_default(),
                        style.font_family.clone(),
                    ),
                    own.font_family.is_some(),
                ),
                with_state(
                    slider(
                        "panel.markdown.font_size",
                        "Font size",
                        style.font_size,
                        FONT_SIZE_RANGE,
                        0,
                    ),
                    own.font_size.is_some(),
                ),
                with_state(
                    slider(
                        "panel.markdown.padding",
                        "Padding",
                        style.padding,
                        PADDING_RANGE,
                        0,
                    ),
                    own.padding.is_some(),
                ),
            ],
        ));
    }

    sections.push(section(
        "Reset",
        vec![button("panel.reset_all", "", "Reset to global")],
    ));
    sections
}

fn text(id: &str, label: &str, value: String, placeholder: String) -> Field {
    field(id, label, Control::Text { value, placeholder })
}

/// Applies one control change to panel `id`. An edit that fails validation changes
/// nothing.
pub fn apply_panel_field(
    scene: &mut Scene,
    id: PanelId,
    field: &str,
    value: FieldValue,
) -> Result<FormOutcome, String> {
    let mut panel = scene
        .get(id)
        .ok_or_else(|| format!("no panel {id:?} for field {field:?}"))?
        .clone();
    let outcome = edit(&mut panel, field, value)?;
    if let FormOutcome::Command(_) = outcome {
        return Ok(outcome);
    }
    panel.overrides.validate(&panel.kind)?;
    let target = scene
        .get_mut(id)
        .ok_or_else(|| format!("no panel {id:?}"))?;
    target.overrides = panel.overrides;
    target.rect = panel.rect;
    target.kind = panel.kind;
    Ok(outcome)
}

/// Empty or blank text means inherited.
fn text_value(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Rebuild when a field flips between inherited and set, so its marker redraws.
fn flipped(before: bool, after: bool) -> FormOutcome {
    FormOutcome::Changed {
        rebuild: before != after,
    }
}

fn edit(panel: &mut Panel, field: &str, value: FieldValue) -> Result<FormOutcome, String> {
    let rebuilt = FormOutcome::Changed { rebuild: true };
    let wrong = || format!("panel field {field:?} got {value:?}");
    let is_terminal = matches!(panel.kind, PanelKind::Terminal { .. });
    let terminal_only = || {
        if is_terminal {
            Ok(())
        } else {
            Err(format!("panel field {field:?} applies only to terminals"))
        }
    };
    if let Some(part) = field.strip_prefix("panel.markdown.") {
        return edit_markdown(panel, part, value);
    }
    let overrides = &mut panel.overrides;

    if let Some(index) = field.strip_prefix("panel.preset.") {
        terminal_only()?;
        let preset = index
            .parse::<usize>()
            .ok()
            .and_then(|index| PALETTE_PRESETS.get(index))
            .ok_or_else(|| format!("unknown panel field {field:?}"))?;
        if value != FieldValue::Pressed {
            return Err(wrong());
        }
        overrides.terminal_colors = Some(PanelTerminalColors::from_colors(&preset.colors));
        return Ok(rebuilt);
    }
    if let Some(part) = field.strip_prefix("panel.colors.") {
        terminal_only()?;
        let slot = match part {
            "background" => ColorSlot::Background,
            "foreground" => ColorSlot::Foreground,
            index => ColorSlot::Palette(
                index
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < 16)
                    .ok_or_else(|| format!("unknown panel field {field:?}"))?,
            ),
        };
        let is_set = |overrides: &PanelOverrides| {
            overrides
                .terminal_colors
                .as_ref()
                .is_some_and(|own| own.text(slot).is_some())
        };
        let was_set = is_set(overrides);
        let value = match value {
            FieldValue::Color(rgb) => Some(rgb.0),
            FieldValue::Reset => None,
            _ => return Err(wrong()),
        };
        let mut own = overrides.terminal_colors.take().unwrap_or_default();
        own.set(slot, value);
        overrides.terminal_colors = (!own.is_empty()).then_some(own);
        return Ok(flipped(was_set, is_set(overrides)));
    }

    match (field, value.clone()) {
        ("panel.title", FieldValue::Text(text)) => {
            let was = overrides.title.is_some();
            overrides.title = text_value(&text);
            Ok(flipped(was, overrides.title.is_some()))
        }
        ("panel.icon", FieldValue::Text(text)) => {
            let old = overrides.icon.clone();
            overrides.icon = text_value(&text);
            let kind = |icon: &Option<String>| {
                icon.as_deref()
                    .map(|value| value.starts_with(BUILTIN_ICON_PREFIX))
            };
            Ok(FormOutcome::Changed {
                rebuild: kind(&old) != kind(&overrides.icon),
            })
        }
        ("panel.title" | "panel.icon" | "panel.icon.builtin", FieldValue::Reset) => {
            if field == "panel.title" {
                overrides.title = None;
            } else {
                overrides.icon = None;
            }
            Ok(rebuilt)
        }
        ("panel.icon.file", FieldValue::Pressed) => {
            Ok(FormOutcome::Command(FormCommand::ChoosePanelIcon(panel.id)))
        }
        ("panel.icon.builtin", FieldValue::Index(0)) => {
            overrides.icon = None;
            Ok(rebuilt)
        }
        ("panel.icon.builtin", FieldValue::Index(index)) => {
            let icon = PanelIcon::ALL
                .get(index - 1)
                .ok_or_else(|| format!("panel field {field:?}: no built-in icon {index}"))?;
            overrides.icon = Some(format!("{BUILTIN_ICON_PREFIX}{}", icon.name()));
            Ok(rebuilt)
        }
        ("panel.borderless", FieldValue::Bool(on)) => {
            let was = overrides.borderless.is_some();
            set_borderless(panel, on);
            Ok(flipped(was, panel.overrides.borderless.is_some()))
        }
        ("panel.borderless", FieldValue::Reset) => {
            set_borderless(panel, false);
            Ok(rebuilt)
        }
        ("panel.accent" | "panel.title_bar" | "panel.title_color", value) => {
            terminal_only()?;
            let slot = match field {
                "panel.accent" => &mut overrides.accent_color,
                "panel.title_bar" => &mut overrides.title_bar_color,
                _ => &mut overrides.title_color,
            };
            let was = slot.is_some();
            *slot = match value {
                FieldValue::Color(rgb) => Some(hex(rgb.0)),
                FieldValue::Reset => None,
                _ => return Err(wrong()),
            };
            Ok(flipped(was, slot.is_some()))
        }
        ("panel.content_opacity", value) => {
            terminal_only()?;
            let was = overrides.content_opacity.is_some();
            overrides.content_opacity = match value {
                FieldValue::Number(number) => Some(content_opacity_value(number)),
                FieldValue::Reset => None,
                _ => return Err(wrong()),
            };
            Ok(flipped(was, overrides.content_opacity.is_some()))
        }
        ("panel.reset_all", FieldValue::Pressed) => {
            set_borderless(panel, false);
            panel.overrides = Default::default();
            Ok(rebuilt)
        }
        (
            "panel.title" | "panel.icon" | "panel.icon.file" | "panel.icon.builtin"
            | "panel.borderless" | "panel.reset_all",
            _,
        ) => Err(wrong()),
        _ => Err(format!("unknown panel field {field:?}")),
    }
}

/// A slider value rounded to `decimals` and clamped to `range`. NaN is refused.
fn rounded(value: f64, range: std::ops::RangeInclusive<f64>, decimals: i32) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let scale = 10f64.powi(decimals);
    Some(((value * scale).round() / scale).clamp(*range.start(), *range.end()))
}

/// The markdown text and style fields. The text is content, so Reset to global keeps it.
fn edit_markdown(panel: &mut Panel, part: &str, value: FieldValue) -> Result<FormOutcome, String> {
    let wrong = || format!("panel field \"panel.markdown.{part}\" got {value:?}");
    if !matches!(panel.kind, PanelKind::Markdown { .. }) {
        return Err(format!(
            "panel field \"panel.markdown.{part}\" applies only to markdown panels"
        ));
    }
    if part == "text" {
        let FieldValue::Text(new) = value else {
            return Err(wrong());
        };
        crate::markdown::check_text(&new)?;
        panel.kind = PanelKind::Markdown { text: new };
        return Ok(FormOutcome::Changed { rebuild: false });
    }
    let mut own = panel.overrides.markdown.take().unwrap_or_default();
    let was = own.clone();
    let reset = value == FieldValue::Reset;
    let result = (|| -> Result<(), String> {
        match (part, value.clone()) {
            ("background_color", FieldValue::Color(rgb)) => own.background_color = Some(rgb),
            ("text_color", FieldValue::Color(rgb)) => own.text_color = Some(rgb),
            ("link_color", FieldValue::Color(rgb)) => own.link_color = Some(rgb),
            ("background_opacity", FieldValue::Number(n)) => {
                own.background_opacity = Some(rounded(n, 0.0..=1.0, 2).ok_or_else(wrong)?)
            }
            ("font_size", FieldValue::Number(n)) => {
                own.font_size = Some(rounded(n, FONT_SIZE_RANGE, 0).ok_or_else(wrong)?)
            }
            ("padding", FieldValue::Number(n)) => {
                own.padding = Some(rounded(n, PADDING_RANGE, 0).ok_or_else(wrong)?)
            }
            ("font_family", FieldValue::Text(family)) => {
                own.font_family = match text_value(&family) {
                    Some(family) => {
                        check_font_family("overrides.markdown.font_family", &family)?;
                        Some(family)
                    }
                    None => None,
                }
            }
            ("background_color", FieldValue::Reset) => own.background_color = None,
            ("text_color", FieldValue::Reset) => own.text_color = None,
            ("link_color", FieldValue::Reset) => own.link_color = None,
            ("background_opacity", FieldValue::Reset) => own.background_opacity = None,
            ("font_size", FieldValue::Reset) => own.font_size = None,
            ("padding", FieldValue::Reset) => own.padding = None,
            ("font_family", FieldValue::Reset) => own.font_family = None,
            (
                "background_color" | "text_color" | "link_color" | "background_opacity"
                | "font_size" | "padding" | "font_family",
                _,
            ) => return Err(wrong()),
            _ => return Err(format!("unknown panel field \"panel.markdown.{part}\"")),
        }
        Ok(())
    })();
    if let Err(err) = result {
        panel.overrides.markdown = (!was.is_empty()).then_some(was);
        return Err(err);
    }
    let flips = field_set(&was, part) != field_set(&own, part);
    panel.overrides.markdown = (!own.is_empty()).then_some(own);
    Ok(FormOutcome::Changed {
        rebuild: flips || reset,
    })
}

fn field_set(style: &PanelMarkdownStyle, part: &str) -> bool {
    match part {
        "background_color" => style.background_color.is_some(),
        "text_color" => style.text_color.is_some(),
        "link_color" => style.link_color.is_some(),
        "background_opacity" => style.background_opacity.is_some(),
        "font_size" => style.font_size.is_some(),
        "padding" => style.padding.is_some(),
        _ => style.font_family.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::scene::content_rect;
    use crate::terminal_profile::TerminalColorsSetting;

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    fn browser() -> PanelKind {
        PanelKind::Browser {
            url: "https://example.com".into(),
            page_zoom: None,
        }
    }

    fn scene_with(kind: PanelKind) -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let id = scene.add(kind, Rect::new(10.0, 20.0, 400.0, 300.0));
        (scene, id)
    }

    fn form(scene: &Scene, id: PanelId, settings: &Settings) -> (Vec<Section>, PanelEffective) {
        let panel = scene.get(id).unwrap();
        let effective = panel_effective(panel, settings, None);
        (panel_form(panel, &effective), effective)
    }

    fn ids(sections: &[Section]) -> Vec<String> {
        sections
            .iter()
            .flat_map(|section| &section.fields)
            .map(|field| field.id.clone())
            .collect()
    }

    fn find<'a>(sections: &'a [Section], id: &str) -> &'a Field {
        sections
            .iter()
            .flat_map(|section| &section.fields)
            .find(|field| field.id == id)
            .unwrap_or_else(|| panic!("no field {id}"))
    }

    /// `settings` is unused by the edit; it keeps the call sites reading as the app's.
    fn apply(
        scene: &mut Scene,
        id: PanelId,
        _settings: &Settings,
        field: &str,
        value: FieldValue,
    ) -> Result<FormOutcome, String> {
        apply_panel_field(scene, id, field, value)
    }

    fn with_global_colors() -> Settings {
        let mut settings = Settings::default();
        settings.terminal.defaults.terminal_colors = Some(TerminalColorsSetting::from_colors(
            &PALETTE_PRESETS[1].colors,
        ));
        settings
    }

    #[test]
    fn panel_opacity_field_sets_clamps_and_resets() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(terminal());
        let opacity = |scene: &Scene| scene.get(id).unwrap().overrides.content_opacity;
        for (input, expected) in [(0.05, 0.2), (0.456, 0.46), (f64::NAN, 0.2), (7.0, 1.0)] {
            let result = apply(
                &mut scene,
                id,
                &settings,
                "panel.content_opacity",
                FieldValue::Number(input),
            );
            assert!(result.is_ok(), "{input}: {result:?}");
            assert_eq!(opacity(&scene), Some(expected), "{input}");
        }
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.content_opacity, 1.0);
        let field = find(&sections, "panel.content_opacity");
        assert_eq!(field.state, FieldState::Set);
        assert_eq!(
            field.control,
            Control::Slider {
                min: MIN_CONTENT_OPACITY,
                max: 1.0,
                value: 1.0,
                decimals: 2,
            }
        );
        let result = apply(
            &mut scene,
            id,
            &settings,
            "panel.content_opacity",
            FieldValue::Reset,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(opacity(&scene), None);
        scene.get_mut(id).unwrap().overrides.content_opacity = Some(0.5);
        let result = apply(
            &mut scene,
            id,
            &settings,
            "panel.reset_all",
            FieldValue::Pressed,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(opacity(&scene), None);
    }

    #[test]
    fn panel_opacity_shows_the_inherited_value() {
        let mut settings = Settings::default();
        settings.terminal.defaults.content_opacity = Some(0.7);
        let (scene, id) = scene_with(terminal());
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.content_opacity, 0.7);
        let field = find(&sections, "panel.content_opacity");
        assert_eq!(field.label, "Opacity");
        assert_eq!(field.state, FieldState::Inherited);
        assert!(matches!(field.control, Control::Slider { value, .. } if value == 0.7));
        assert!(sections.iter().any(|section| {
            section.title == "Opacity"
                && section
                    .fields
                    .iter()
                    .any(|f| f.id == "panel.content_opacity")
        }));
    }

    #[test]
    fn browser_form_has_no_opacity_field() {
        let (mut scene, id) = scene_with(browser());
        let (sections, effective) = form(&scene, id, &Settings::default());
        assert!(!ids(&sections).contains(&"panel.content_opacity".to_string()));
        assert_eq!(effective.content_opacity, DEFAULT_CONTENT_OPACITY);
        let settings = Settings::default();
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.content_opacity",
                FieldValue::Number(0.5),
            )
            .is_err()
        );
        assert_eq!(scene.get(id).unwrap().overrides.content_opacity, None);
    }

    #[test]
    fn browser_form_has_title_icon_borderless_only() {
        let (scene, id) = scene_with(browser());
        let (sections, _) = form(&scene, id, &Settings::default());
        assert_eq!(
            ids(&sections),
            [
                "panel.title",
                "panel.icon",
                "panel.icon.file",
                "panel.icon.builtin",
                "panel.borderless",
                "panel.reset_all",
            ]
        );
        let Control::Choice { options, selected } = &find(&sections, "panel.icon.builtin").control
        else {
            panic!("builtin icon is not a choice");
        };
        assert_eq!(options, &["Default", "Terminal", "Browser", "Plugin"]);
        assert_eq!(*selected, 0);
    }

    #[test]
    fn terminal_form_adds_colors_and_presets() {
        let settings = with_global_colors();
        let (scene, id) = scene_with(terminal());
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.colors, Some(PALETTE_PRESETS[1].colors));
        assert!(!effective.live_colors_note);
        let mut expected: Vec<String> = [
            "panel.title",
            "panel.icon",
            "panel.icon.file",
            "panel.icon.builtin",
            "panel.borderless",
            "panel.accent",
            "panel.title_bar",
            "panel.title_color",
            "panel.colors.background",
            "panel.colors.foreground",
        ]
        .map(String::from)
        .to_vec();
        expected.extend((0..16).map(|index| format!("panel.colors.{index}")));
        expected.extend((0..PALETTE_PRESETS.len()).map(|index| format!("panel.preset.{index}")));
        expected.push("panel.content_opacity".into());
        expected.push("panel.reset_all".into());
        assert_eq!(ids(&sections), expected);
        assert_eq!(
            find(&sections, "panel.colors.background").control,
            Control::Color(Rgb(PALETTE_PRESETS[1].colors.background))
        );
        assert_eq!(
            find(&sections, "panel.preset.0").control,
            Control::Button(format!("Load {}", PALETTE_PRESETS[0].label))
        );
    }

    #[test]
    fn unset_fields_show_inherited_and_set_fields_show_set() {
        let settings = with_global_colors();
        let (mut scene, id) = scene_with(terminal());
        let (sections, _) = form(&scene, id, &settings);
        for field in sections.iter().flat_map(|section| &section.fields) {
            let expected = match field.control {
                Control::Button(_) | Control::Note(_) => FieldState::Plain,
                _ => FieldState::Inherited,
            };
            assert_eq!(field.state, expected, "{}", field.id);
        }

        apply(
            &mut scene,
            id,
            &settings,
            "panel.title",
            FieldValue::Text("Build".into()),
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.preset.0",
            FieldValue::Pressed,
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.accent",
            FieldValue::Color(Rgb(0x445566)),
        )
        .unwrap();
        let (sections, effective) = form(&scene, id, &settings);
        let title = find(&sections, "panel.title");
        assert_eq!(title.state, FieldState::Set);
        assert_eq!(
            title.control,
            Control::Text {
                value: "Build".into(),
                placeholder: "Build".into(),
            }
        );
        assert_eq!(effective.title, "Build");
        assert_eq!(find(&sections, "panel.icon").state, FieldState::Inherited);
        assert_eq!(find(&sections, "panel.accent").state, FieldState::Set);
        assert_eq!(
            find(&sections, "panel.title_bar").state,
            FieldState::Inherited
        );
        assert_eq!(find(&sections, "panel.colors.7").state, FieldState::Set);
        assert_eq!(effective.colors, Some(PALETTE_PRESETS[0].colors));
    }

    #[test]
    fn emoji_icon_is_accepted_and_five_chars_rejected() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(browser());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.icon",
            FieldValue::Text("🚀".into()),
        )
        .unwrap();
        assert_eq!(scene.get(id).unwrap().overrides.icon.as_deref(), Some("🚀"));
        let before = scene.get(id).unwrap().clone();
        let result = apply(
            &mut scene,
            id,
            &settings,
            "panel.icon",
            FieldValue::Text("abcde".into()),
        );
        assert!(result.is_err(), "{result:?}");
        assert_eq!(scene.get(id).unwrap(), &before);
    }

    #[test]
    fn builtin_to_text_icon_requests_rebuild_and_flips_both_controls() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(terminal());
        scene.get_mut(id).unwrap().overrides.icon = Some("builtin:terminal".into());

        assert_eq!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.icon",
                FieldValue::Text("🚀".into()),
            ),
            Ok(FormOutcome::Changed { rebuild: true })
        );
        let (sections, _) = form(&scene, id, &settings);
        assert_eq!(find(&sections, "panel.icon").state, FieldState::Set);
        let builtin = find(&sections, "panel.icon.builtin");
        assert_eq!(builtin.state, FieldState::Inherited);
        assert!(matches!(
            builtin.control,
            Control::Choice { selected: 0, .. }
        ));

        scene.get_mut(id).unwrap().overrides.icon = Some("ab".into());
        assert_eq!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.icon",
                FieldValue::Text("cd".into()),
            ),
            Ok(FormOutcome::Changed { rebuild: false })
        );
    }

    #[test]
    fn empty_icon_text_resets_to_inherited() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(browser());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.icon",
            FieldValue::Text("ab".into()),
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.icon",
            FieldValue::Text("  ".into()),
        )
        .unwrap();
        assert_eq!(scene.get(id).unwrap().overrides.icon, None);
        let (sections, _) = form(&scene, id, &settings);
        assert_eq!(find(&sections, "panel.icon").state, FieldState::Inherited);
    }

    #[test]
    fn builtin_icon_choice_stores_the_named_form_and_default_clears() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(browser());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.icon.builtin",
            FieldValue::Index(1),
        )
        .unwrap();
        assert_eq!(
            scene.get(id).unwrap().overrides.icon.as_deref(),
            Some("builtin:terminal")
        );
        let (sections, _) = form(&scene, id, &settings);
        let builtin = find(&sections, "panel.icon.builtin");
        assert_eq!(builtin.state, FieldState::Set);
        assert!(matches!(
            builtin.control,
            Control::Choice { selected: 1, .. }
        ));
        // The text box does not show the stored builtin form.
        assert_eq!(find(&sections, "panel.icon").state, FieldState::Inherited);
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.icon.builtin",
                FieldValue::Index(9)
            )
            .is_err()
        );
        apply(
            &mut scene,
            id,
            &settings,
            "panel.icon.builtin",
            FieldValue::Index(0),
        )
        .unwrap();
        assert_eq!(scene.get(id).unwrap().overrides.icon, None);
        assert_eq!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.icon.file",
                FieldValue::Pressed
            ),
            Ok(FormOutcome::Command(FormCommand::ChoosePanelIcon(id)))
        );
    }

    #[test]
    fn reset_restores_the_global_value() {
        let mut settings = with_global_colors();
        settings.terminal.defaults.accent_color = Some("#112233".into());
        let (mut scene, id) = scene_with(terminal());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.accent",
            FieldValue::Color(Rgb(0x445566)),
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.3",
            FieldValue::Color(Rgb(0xabcdef)),
        )
        .unwrap();
        let (_, effective) = form(&scene, id, &settings);
        assert_eq!(effective.chrome.accent, Some(0x445566ff));
        let mut edited = PALETTE_PRESETS[1].colors;
        edited.palette[3] = 0xabcdef;
        assert_eq!(effective.colors, Some(edited));

        apply(&mut scene, id, &settings, "panel.accent", FieldValue::Reset).unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.3",
            FieldValue::Reset,
        )
        .unwrap();
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.chrome.accent, Some(0x112233ff));
        assert_eq!(effective.colors, Some(PALETTE_PRESETS[1].colors));
        assert_eq!(find(&sections, "panel.accent").state, FieldState::Inherited);
        assert_eq!(
            find(&sections, "panel.colors.3").state,
            FieldState::Inherited
        );
        assert!(scene.get(id).unwrap().overrides.is_empty());
    }

    #[test]
    fn single_well_edit_with_nothing_set_seeds_from_ghostty_default() {
        let (mut scene, id) = scene_with(terminal());
        let ghostty = PALETTE_PRESETS
            .iter()
            .find(|preset| preset.label == "Ghostty Default")
            .unwrap()
            .colors;
        apply_panel_field(
            &mut scene,
            id,
            "panel.colors.3",
            FieldValue::Color(Rgb(0xabcdef)),
        )
        .unwrap();
        // Only the edited color is stored; the seed is for display and the live theme.
        let stored = scene
            .get(id)
            .unwrap()
            .overrides
            .terminal_colors
            .clone()
            .unwrap();
        let mut only = PanelTerminalColors::default();
        only.set(ColorSlot::Palette(3), Some(0xabcdef));
        assert_eq!(stored, only);
        let (_, effective) = form(&scene, id, &Settings::default());
        let mut expected = ghostty;
        expected.palette[3] = 0xabcdef;
        assert_ne!(ghostty.palette[3], 0xabcdef);
        assert_eq!(effective.colors, Some(expected));
    }

    fn color_well_states(sections: &[Section]) -> Vec<(String, FieldState)> {
        sections
            .iter()
            .flat_map(|section| &section.fields)
            .filter(|field| {
                field.id.starts_with("panel.colors.") && field.id != "panel.colors.note"
            })
            .map(|field| (field.id.clone(), field.state))
            .collect()
    }

    #[test]
    fn one_well_edit_marks_only_that_well_set() {
        let settings = with_global_colors();
        let (mut scene, id) = scene_with(terminal());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.5",
            FieldValue::Color(Rgb(0x123456)),
        )
        .unwrap();
        let (sections, _) = form(&scene, id, &settings);
        let states = color_well_states(&sections);
        assert_eq!(states.len(), 18);
        for (field, state) in states {
            let expected = if field == "panel.colors.5" {
                FieldState::Set
            } else {
                FieldState::Inherited
            };
            assert_eq!(state, expected, "{field}");
        }
    }

    #[test]
    fn a_global_change_reaches_colors_the_panel_did_not_set() {
        let mut settings = with_global_colors();
        let (mut scene, id) = scene_with(terminal());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.5",
            FieldValue::Color(Rgb(0x123456)),
        )
        .unwrap();
        let global = PALETTE_PRESETS[2].colors;
        settings.terminal.defaults.terminal_colors =
            Some(TerminalColorsSetting::from_colors(&global));
        let (_, effective) = form(&scene, id, &settings);
        let mut expected = global;
        expected.palette[5] = 0x123456;
        assert_eq!(effective.colors, Some(expected));
    }

    #[test]
    fn reset_of_one_well_keeps_another_edited_well() {
        let settings = with_global_colors();
        let (mut scene, id) = scene_with(terminal());
        for (field, rgb) in [
            ("panel.colors.5", 0x123456),
            ("panel.colors.background", 0x0a0b0c),
        ] {
            apply(
                &mut scene,
                id,
                &settings,
                field,
                FieldValue::Color(Rgb(rgb)),
            )
            .unwrap();
        }
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.5",
            FieldValue::Reset,
        )
        .unwrap();
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(
            find(&sections, "panel.colors.background").state,
            FieldState::Set
        );
        assert_eq!(
            find(&sections, "panel.colors.5").state,
            FieldState::Inherited
        );
        let mut expected = PALETTE_PRESETS[1].colors;
        expected.background = 0x0a0b0c;
        assert_eq!(effective.colors, Some(expected));
        // The last set well reset leaves no palette of the panel's own.
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.background",
            FieldValue::Reset,
        )
        .unwrap();
        assert_eq!(scene.get(id).unwrap().overrides.terminal_colors, None);
    }

    #[test]
    fn live_theme_note_says_the_change_applies_to_new_terminals() {
        let (scene, id) = scene_with(terminal());
        let panel = scene.get(id).unwrap();
        let mut effective = panel_effective(panel, &Settings::default(), None);
        let note = |effective: &PanelEffective| match &find(
            &panel_form(panel, effective),
            "panel.colors.note",
        )
        .control
        {
            Control::Note(text) => text.clone(),
            other => panic!("note is {other:?}"),
        };
        assert!(note(&effective).contains("Ghostty config"));
        effective.live_theme = true;
        assert!(
            note(&effective).contains("new terminals"),
            "{}",
            note(&effective)
        );
    }

    #[test]
    fn reset_with_no_global_colors_gives_none_and_the_live_note() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(terminal());
        let (sections, effective) = form(&scene, id, &settings);
        assert!(effective.live_colors_note);
        assert_eq!(effective.colors, None);
        // The wells show the seed a one-well edit would keep.
        let well = find(&sections, "panel.colors.background");
        assert_eq!(well.state, FieldState::Inherited);
        assert_eq!(
            well.control,
            Control::Color(Rgb(ghostty_default_colors().background))
        );

        apply(
            &mut scene,
            id,
            &settings,
            "panel.preset.2",
            FieldValue::Pressed,
        )
        .unwrap();
        let (_, effective) = form(&scene, id, &settings);
        assert_eq!(effective.colors, Some(PALETTE_PRESETS[2].colors));
        assert!(!effective.live_colors_note);

        // One reset drops only that color: it falls back to the Ghostty Default seed.
        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.background",
            FieldValue::Reset,
        )
        .unwrap();
        let (_, effective) = form(&scene, id, &settings);
        let mut expected = PALETTE_PRESETS[2].colors;
        expected.background = ghostty_default_colors().background;
        assert_eq!(effective.colors, Some(expected));
        assert!(!effective.live_colors_note);

        apply(
            &mut scene,
            id,
            &settings,
            "panel.reset_all",
            FieldValue::Pressed,
        )
        .unwrap();
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.colors, None);
        assert!(effective.live_colors_note);
        assert!(matches!(
            find(&sections, "panel.colors.note").control,
            Control::Note(_)
        ));
    }

    #[test]
    fn borderless_and_reset_all_keep_the_content_rect() {
        let settings = with_global_colors();
        let (mut scene, id) = scene_with(terminal());
        let content = content_rect(scene.get(id).unwrap());
        apply(
            &mut scene,
            id,
            &settings,
            "panel.borderless",
            FieldValue::Bool(true),
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.title",
            FieldValue::Text("x".into()),
        )
        .unwrap();
        apply(
            &mut scene,
            id,
            &settings,
            "panel.preset.1",
            FieldValue::Pressed,
        )
        .unwrap();
        let panel = scene.get(id).unwrap();
        assert_eq!(panel.overrides.borderless, Some(true));
        assert_eq!(content_rect(panel), content);

        let revision = scene.revision();
        assert_eq!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.reset_all",
                FieldValue::Pressed
            ),
            Ok(FormOutcome::Changed { rebuild: true })
        );
        assert_ne!(
            scene.revision(),
            revision,
            "reset must mark the scene dirty"
        );
        let panel = scene.get(id).unwrap();
        assert!(panel.overrides.is_empty(), "{:?}", panel.overrides);
        assert_eq!(content_rect(panel), content);
        assert_eq!(panel.rect, Rect::new(10.0, 20.0, 400.0, 300.0));
    }

    #[test]
    fn apply_to_missing_panel_is_an_error() {
        let (mut scene, _) = scene_with(browser());
        let result = apply_panel_field(
            &mut scene,
            PanelId(999),
            "panel.title",
            FieldValue::Text("x".into()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn wrong_values_and_terminal_fields_on_a_browser_are_errors() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(browser());
        let before = scene.clone();
        let revision = scene.revision();
        for (field, value) in [
            ("panel.nope", FieldValue::Pressed),
            ("panel.title", FieldValue::Bool(true)),
            ("panel.title", FieldValue::Text("a\u{7}".into())),
            ("panel.icon", FieldValue::Text("builtin:nope".into())),
            ("panel.icon", FieldValue::Text("../x.png".into())),
            ("panel.borderless", FieldValue::Text("x".into())),
            ("panel.accent", FieldValue::Color(Rgb(1))),
            ("panel.preset.0", FieldValue::Pressed),
            ("panel.colors.0", FieldValue::Color(Rgb(1))),
        ] {
            let result = apply(&mut scene, id, &settings, field, value.clone());
            assert!(result.is_err(), "{field} {value:?}: {result:?}");
        }
        assert_eq!(scene, before);
        assert_eq!(
            scene.revision(),
            revision,
            "a rejected edit must not mark dirty"
        );
    }

    fn sample_value(control: &Control) -> Option<FieldValue> {
        match control {
            Control::Checkbox(on) => Some(FieldValue::Bool(!on)),
            Control::Slider { min, max, .. } => Some(FieldValue::Number((min + max) / 2.0)),
            Control::Color(_) => Some(FieldValue::Color(Rgb(0x123456))),
            Control::Choice { options, .. } => Some(FieldValue::Index(options.len() - 1)),
            Control::Button(_) => Some(FieldValue::Pressed),
            Control::Text { .. } | Control::TextArea(_) => Some(FieldValue::Text("Hi".into())),
            Control::Note(_) | Control::Key { .. } => None,
        }
    }

    #[test]
    fn every_control_in_every_panel_form_applies_resets_and_stays_valid() {
        let settings = with_global_colors();
        for kind in [terminal(), browser(), PanelKind::markdown()] {
            let (scene, id) = scene_with(kind.clone());
            let (sections, _) = form(&scene, id, &settings);
            let mut seen = std::collections::HashSet::new();
            for field in sections.iter().flat_map(|section| &section.fields) {
                assert!(seen.insert(field.id.clone()), "duplicate id {}", field.id);
                let Some(value) = sample_value(&field.control) else {
                    continue;
                };
                let mut edited = scene.clone();
                let outcome = apply(&mut edited, id, &settings, &field.id, value);
                assert!(outcome.is_ok(), "{kind:?} {}: {outcome:?}", field.id);
                let panel = edited.get(id).unwrap();
                assert_eq!(
                    panel.overrides.validate(&panel.kind),
                    Ok(()),
                    "{}",
                    field.id
                );
                if field.state != FieldState::Plain {
                    apply(&mut edited, id, &settings, &field.id, FieldValue::Reset).unwrap();
                    let panel = edited.get(id).unwrap();
                    assert!(
                        panel.overrides.is_empty(),
                        "{} reset left {:?}",
                        field.id,
                        panel.overrides
                    );
                }
            }
        }
    }

    #[test]
    fn markdown_form_edits_text_and_each_style_field_inherits_or_sets() {
        let mut settings = Settings::default();
        settings.markdown.font_size = 20.0;
        let (mut scene, id) = scene_with(PanelKind::markdown());
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.markdown.as_ref().unwrap().font_size, 20.0);
        let ids = ids(&sections);
        for expected in [
            "panel.markdown.text",
            "panel.markdown.background_color",
            "panel.markdown.background_opacity",
            "panel.markdown.text_color",
            "panel.markdown.link_color",
            "panel.markdown.font_family",
            "panel.markdown.font_size",
            "panel.markdown.padding",
        ] {
            assert!(ids.contains(&expected.to_string()), "{expected}: {ids:?}");
        }
        assert!(!ids.contains(&"panel.content_opacity".to_string()));
        assert!(matches!(
            &find(&sections, "panel.markdown.text").control,
            Control::TextArea(text) if text.starts_with("# Markdown")
        ));
        assert_eq!(
            find(&sections, "panel.markdown.font_size").state,
            FieldState::Inherited
        );

        let text = "# New\n\nbody\n".to_string();
        let outcome = apply(
            &mut scene,
            id,
            &settings,
            "panel.markdown.text",
            FieldValue::Text(text.clone()),
        );
        assert_eq!(outcome, Ok(FormOutcome::Changed { rebuild: false }));
        assert_eq!(
            scene.get(id).unwrap().kind,
            PanelKind::Markdown { text: text.clone() }
        );

        let outcome = apply(
            &mut scene,
            id,
            &settings,
            "panel.markdown.background_opacity",
            FieldValue::Number(0.0),
        );
        assert_eq!(outcome, Ok(FormOutcome::Changed { rebuild: true }));
        let (sections, effective) = form(&scene, id, &settings);
        assert_eq!(effective.markdown.unwrap().background_opacity, 0.0);
        assert_eq!(
            find(&sections, "panel.markdown.background_opacity").state,
            FieldState::Set
        );

        // A bad font family changes nothing.
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.markdown.font_family",
                FieldValue::Text("a;b".into())
            )
            .is_err()
        );
        assert_eq!(
            scene
                .get(id)
                .unwrap()
                .overrides
                .markdown
                .as_ref()
                .unwrap()
                .font_family,
            None
        );
        // Out of range sliders clamp.
        apply(
            &mut scene,
            id,
            &settings,
            "panel.markdown.font_size",
            FieldValue::Number(500.0),
        )
        .unwrap();
        assert_eq!(
            scene
                .get(id)
                .unwrap()
                .overrides
                .markdown
                .as_ref()
                .unwrap()
                .font_size,
            Some(48.0)
        );

        // Reset to global clears the style, never the text.
        apply(
            &mut scene,
            id,
            &settings,
            "panel.reset_all",
            FieldValue::Pressed,
        )
        .unwrap();
        let panel = scene.get(id).unwrap();
        assert!(panel.overrides.is_empty());
        assert_eq!(panel.kind, PanelKind::Markdown { text });
    }

    #[test]
    fn markdown_fields_on_other_panels_and_oversized_text_are_errors() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(terminal());
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.markdown.text",
                FieldValue::Text("x".into())
            )
            .is_err()
        );
        assert!(!ids(&form(&scene, id, &settings).0).contains(&"panel.markdown.text".to_string()));
        let (mut scene, id) = scene_with(PanelKind::markdown());
        let huge = "a".repeat(crate::markdown::MAX_MARKDOWN_BYTES + 1);
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.markdown.text",
                FieldValue::Text(huge)
            )
            .is_err()
        );
        assert_eq!(scene.get(id).unwrap().kind, PanelKind::markdown());
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.markdown.nope",
                FieldValue::Reset
            )
            .is_err()
        );
        let mut overrides = PanelOverrides::default();
        overrides.markdown = Some(Default::default());
        assert!(overrides.validate(&terminal()).is_err());
    }

    fn manifest_layer() -> AppLayer {
        let mut colors = ghostty_default_colors();
        colors.palette[2] = 0x123456;
        AppLayer {
            chrome: ChromeOverrides {
                accent: None,
                title_bar: Some(0x445566ff),
                title: None,
            },
            colors: Some(colors),
            ..AppLayer::default()
        }
    }

    #[test]
    fn an_app_panel_shows_terminal_sections_with_manifest_values() {
        let (mut scene, id) = scene_with(PanelKind::app_terminal("top-example", None));
        let settings = Settings::default();
        let app = manifest_layer();
        let panel = scene.get(id).unwrap();
        let effective = panel_effective(panel, &settings, Some(&app));
        let sections = panel_form(panel, &effective);
        let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Title bar colors"), "{titles:?}");
        assert!(titles.contains(&"Terminal colors"), "{titles:?}");
        assert_eq!(effective.chrome.title_bar, Some(0x445566ff));
        assert_eq!(effective.colors.unwrap().palette[2], 0x123456);
        // A panel color edit is accepted on an app panel and wins over the manifest.
        apply_panel_field(
            &mut scene,
            id,
            "panel.colors.2",
            FieldValue::Color(Rgb(0x654321)),
        )
        .unwrap();
        let panel = scene.get(id).unwrap();
        let effective = panel_effective(panel, &settings, Some(&app));
        assert_eq!(effective.colors.unwrap().palette[2], 0x654321);
        // Reset falls back to the manifest.
        apply_panel_field(&mut scene, id, "panel.colors.2", FieldValue::Reset).unwrap();
        let panel = scene.get(id).unwrap();
        let effective = panel_effective(panel, &settings, Some(&app));
        assert_eq!(effective.colors.unwrap().palette[2], 0x123456);
    }
}
