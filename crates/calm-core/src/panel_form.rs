//! The per-panel Settings window as data. Each field is inherited or set; a set field
//! lives in `PanelOverrides`, and a reset puts it back to inherited.

use crate::background::Rgb;
use crate::chrome::{BUILTIN_ICON_PREFIX, ChromeOverrides, PanelIcon, lay_overrides};
use crate::scene::{Panel, PanelId, PanelKind, Scene, set_borderless};
use crate::settings::Settings;
use crate::settings_form::{
    CHROME_SEED, Control, Field, FieldState, FieldValue, FormCommand, FormOutcome, Section, button,
    checkbox, choice, color, field, note, rgb_of, section,
};
use crate::terminal_profile::{
    PALETTE_PRESETS, TerminalColors, TerminalColorsSetting, TerminalIcon, hex,
};

/// What the panel shows now, inherited and set fields alike.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelEffective {
    pub title: String,
    pub icon: Option<String>,
    pub chrome: ChromeOverrides,
    pub colors: Option<TerminalColors>,
    /// A terminal with no colors of its own: Ghostty's config colors it.
    pub live_colors_note: bool,
}

pub fn panel_effective(panel: &Panel, settings: &Settings) -> PanelEffective {
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
                overrides,
            );
            let inherited_icon = resolved.icon.map(|icon| match icon {
                TerminalIcon::Text(text) => text,
                TerminalIcon::Image(path) => path.display().to_string(),
            });
            PanelEffective {
                title,
                // A builtin override reads as text to the resolver; take it as stored.
                icon: overrides.icon.clone().or(inherited_icon),
                chrome: resolved.overrides,
                live_colors_note: resolved.colors.is_none(),
                colors: resolved.colors,
            }
        }
        _ => PanelEffective {
            title,
            icon: overrides.icon.clone(),
            chrome: lay_overrides(ChromeOverrides::default(), overrides),
            colors: None,
            live_colors_note: false,
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

        let set = overrides.terminal_colors.is_some();
        let mut colors = Vec::new();
        if let Some(effective_colors) = &effective.colors {
            colors.push(with_state(
                color(
                    "panel.colors.background",
                    "Background",
                    Rgb(effective_colors.background),
                ),
                set,
            ));
            colors.push(with_state(
                color(
                    "panel.colors.foreground",
                    "Foreground",
                    Rgb(effective_colors.foreground),
                ),
                set,
            ));
            for (index, value) in effective_colors.palette.iter().enumerate() {
                colors.push(with_state(
                    color(
                        &format!("panel.colors.{index}"),
                        &format!("Color {index}"),
                        Rgb(*value),
                    ),
                    set,
                ));
            }
        }
        if effective.live_colors_note {
            colors.push(note(
                "panel.colors.note",
                "",
                "Colors come from your Ghostty config. Load a theme to give this terminal its \
                 own."
                    .to_string(),
            ));
        }
        for (index, preset) in PALETTE_PRESETS.iter().enumerate() {
            colors.push(button(
                &format!("panel.preset.{index}"),
                "",
                &format!("Load {}", preset.label),
            ));
        }
        sections.push(section("Terminal colors", colors));
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

/// Applies one control change to panel `id`. `inherited_colors` is the palette the panel
/// would have with no `terminal_colors` override, the base for editing one color of it.
/// An edit that fails validation changes nothing.
pub fn apply_panel_field(
    scene: &mut Scene,
    id: PanelId,
    field: &str,
    value: FieldValue,
    inherited_colors: Option<&TerminalColors>,
) -> Result<FormOutcome, String> {
    let mut panel = scene
        .get(id)
        .ok_or_else(|| format!("no panel {id:?} for field {field:?}"))?
        .clone();
    let outcome = edit(&mut panel, field, value, inherited_colors)?;
    if let FormOutcome::Command(_) = outcome {
        return Ok(outcome);
    }
    panel.overrides.validate(&panel.kind)?;
    let target = scene
        .get_mut(id)
        .ok_or_else(|| format!("no panel {id:?}"))?;
    target.overrides = panel.overrides;
    target.rect = panel.rect;
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

fn edit(
    panel: &mut Panel,
    field: &str,
    value: FieldValue,
    inherited_colors: Option<&TerminalColors>,
) -> Result<FormOutcome, String> {
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
        overrides.terminal_colors = Some(TerminalColorsSetting::from_colors(&preset.colors));
        return Ok(rebuilt);
    }
    if let Some(part) = field.strip_prefix("panel.colors.") {
        terminal_only()?;
        let slot = match part {
            "background" | "foreground" => None,
            index => Some(
                index
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < 16)
                    .ok_or_else(|| format!("unknown panel field {field:?}"))?,
            ),
        };
        let was_set = overrides.terminal_colors.is_some();
        return match value {
            FieldValue::Reset => {
                overrides.terminal_colors = None;
                Ok(flipped(was_set, false))
            }
            FieldValue::Color(rgb) => {
                let own = overrides
                    .terminal_colors
                    .as_ref()
                    .and_then(|colors| colors.parse("overrides.terminal_colors").ok());
                let mut colors = own.or(inherited_colors.copied()).ok_or_else(|| {
                    format!("panel field {field:?}: no colors to edit, load a theme first")
                })?;
                match (part, slot) {
                    ("background", _) => colors.background = rgb.0,
                    ("foreground", _) => colors.foreground = rgb.0,
                    (_, Some(index)) => colors.palette[index] = rgb.0,
                    _ => return Err(format!("unknown panel field {field:?}")),
                }
                overrides.terminal_colors = Some(TerminalColorsSetting::from_colors(&colors));
                Ok(flipped(was_set, true))
            }
            _ => Err(wrong()),
        };
    }

    match (field, value.clone()) {
        ("panel.title", FieldValue::Text(text)) => {
            let was = overrides.title.is_some();
            overrides.title = text_value(&text);
            Ok(flipped(was, overrides.title.is_some()))
        }
        ("panel.icon", FieldValue::Text(text)) => {
            let was = overrides.icon.is_some();
            overrides.icon = text_value(&text);
            Ok(flipped(was, overrides.icon.is_some()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::scene::content_rect;

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
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
        let effective = panel_effective(panel, settings);
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

    /// Applies with the inherited palette the current form would pass.
    fn apply(
        scene: &mut Scene,
        id: PanelId,
        settings: &Settings,
        field: &str,
        value: FieldValue,
    ) -> Result<FormOutcome, String> {
        let inherited = {
            let mut bare = scene.get(id).unwrap().clone();
            bare.overrides.terminal_colors = None;
            panel_effective(&bare, settings).colors
        };
        apply_panel_field(scene, id, field, value, inherited.as_ref())
    }

    fn with_global_colors() -> Settings {
        let mut settings = Settings::default();
        settings.terminal.defaults.terminal_colors = Some(TerminalColorsSetting::from_colors(
            &PALETTE_PRESETS[1].colors,
        ));
        settings
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
    fn reset_with_no_global_colors_gives_none_and_the_live_note() {
        let settings = Settings::default();
        let (mut scene, id) = scene_with(terminal());
        let (sections, effective) = form(&scene, id, &settings);
        assert!(effective.live_colors_note);
        // No palette to edit one color of until a theme sets one.
        assert!(!ids(&sections).contains(&"panel.colors.background".to_string()));
        assert!(
            apply(
                &mut scene,
                id,
                &settings,
                "panel.colors.0",
                FieldValue::Color(Rgb(1))
            )
            .is_err()
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

        apply(
            &mut scene,
            id,
            &settings,
            "panel.colors.background",
            FieldValue::Reset,
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
            None,
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
            Control::Text { .. } => Some(FieldValue::Text("Hi".into())),
            Control::Note(_) => None,
        }
    }

    #[test]
    fn every_control_in_every_panel_form_applies_resets_and_stays_valid() {
        let settings = with_global_colors();
        for kind in [terminal(), browser()] {
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
}
