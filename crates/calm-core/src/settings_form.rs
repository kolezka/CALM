//! The Settings window as data: sections of labeled controls built from `Settings`, and the
//! edit each control makes. The native window only draws this and reports changes back.

use std::ops::RangeInclusive;

use crate::background::{
    BackgroundKind, GradientKind, GradientStop, ImageFit, MAX_BLUR, MAX_FILTER_FACTOR,
    MAX_GRADIENT_STOPS, MIN_GRADIENT_STOPS, Rgb,
};
use crate::grid::GridKind;
use crate::settings::{
    GRID_MAJOR_EVERY_MAX, GRID_MARK_SIZE_RANGE, GRID_SPACING_RANGE, PREVIEW_ZOOM_RANGE,
    SNAP_EDGE_RANGE, SNAP_GRID_STEPS, Settings, TERMINAL_HEIGHT_RANGE, TERMINAL_WIDTH_RANGE,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Checkbox(bool),
    /// The native slider shows the value with `decimals` digits next to it.
    Slider {
        min: f64,
        max: f64,
        value: f64,
        decimals: u8,
    },
    Color(Rgb),
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Button(String),
    /// Read-only text.
    Note(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub control: Control,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub fields: Vec<Field>,
}

/// What a control reports when the user changes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldValue {
    Bool(bool),
    Number(f64),
    Color(Rgb),
    Index(usize),
    Pressed,
}

/// State shown in the form that does not live in `Settings`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormContext {
    pub show_fps: bool,
    pub settings_path: String,
    /// A problem worth showing at the top, such as a settings file that was set aside.
    pub notice: Option<String>,
    /// Why the background image could not be shown, if it could not.
    pub image_error: Option<String>,
}

/// Work the app does itself, because it needs AppKit or state outside `Settings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormCommand {
    ChooseImage,
    SetShowFps(bool),
    OpenConfigFolder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormOutcome {
    /// `Settings` changed. `rebuild` means the set of controls changed too.
    Changed {
        rebuild: bool,
    },
    Command(FormCommand),
}

fn checkbox(id: &str, label: &str, value: bool) -> Field {
    field(id, label, Control::Checkbox(value))
}

fn slider(id: &str, label: &str, value: f64, range: RangeInclusive<f64>, decimals: u8) -> Field {
    field(
        id,
        label,
        Control::Slider {
            min: *range.start(),
            max: *range.end(),
            value,
            decimals,
        },
    )
}

fn color(id: &str, label: &str, value: Rgb) -> Field {
    field(id, label, Control::Color(value))
}

fn choice(id: &str, label: &str, options: Vec<String>, selected: usize) -> Field {
    field(id, label, Control::Choice { options, selected })
}

fn button(id: &str, label: &str, title: &str) -> Field {
    field(id, label, Control::Button(title.to_string()))
}

fn note(id: &str, label: &str, text: String) -> Field {
    field(id, label, Control::Note(text))
}

fn field(id: &str, label: &str, control: Control) -> Field {
    Field {
        id: id.to_string(),
        label: label.to_string(),
        control,
    }
}

fn section(title: &str, fields: Vec<Field>) -> Section {
    Section {
        title: title.to_string(),
        fields,
    }
}

const GRADIENT_KINDS: [GradientKind; 2] = [GradientKind::Linear, GradientKind::Radial];

/// The whole form for `settings`. Only the selected background kind's controls are shown.
pub fn settings_form(settings: &Settings, context: &FormContext) -> Vec<Section> {
    let mut sections = Vec::new();
    if let Some(notice) = &context.notice {
        sections.push(section("Notice", vec![note("notice", "", notice.clone())]));
    }
    sections.push(section("Background", background_fields(settings, context)));

    let grid = &settings.grid;
    let kinds = GridKind::ALL
        .iter()
        .map(|k| k.label().to_string())
        .collect();
    let selected_kind = GridKind::ALL
        .iter()
        .position(|kind| *kind == grid.kind)
        .unwrap_or(0);
    let mut grid_fields = vec![
        checkbox("grid.show", "Show grid", grid.show),
        choice("grid.kind", "Style", kinds, selected_kind),
        slider(
            "grid.spacing",
            "Spacing",
            grid.spacing,
            GRID_SPACING_RANGE,
            0,
        ),
        color("grid.color", "Color", grid.color),
        slider("grid.opacity", "Opacity", grid.opacity, 0.0..=1.0, 2),
    ];
    if matches!(grid.kind, GridKind::Dots | GridKind::Crosses) {
        grid_fields.push(slider(
            "grid.mark_size",
            "Mark size",
            grid.mark_size,
            GRID_MARK_SIZE_RANGE,
            0,
        ));
    }
    grid_fields.push(slider(
        "grid.major_every",
        "Major every (0 off)",
        f64::from(grid.major_every),
        0.0..=f64::from(GRID_MAJOR_EVERY_MAX),
        0,
    ));
    grid_fields.push(slider(
        "grid.major_opacity",
        "Major opacity",
        grid.major_opacity,
        0.0..=1.0,
        2,
    ));
    sections.push(section("Grid", grid_fields));

    let snap = &settings.snap;
    let steps: Vec<String> = SNAP_GRID_STEPS
        .iter()
        .map(|step| format!("{step}"))
        .collect();
    let selected = SNAP_GRID_STEPS
        .iter()
        .position(|step| *step == snap.grid)
        .unwrap_or(0);
    sections.push(section(
        "Canvas and Snapping",
        vec![
            checkbox(
                "canvas.click_empty_clears_focus",
                "Click on empty canvas clears focus",
                settings.canvas.click_empty_clears_focus,
            ),
            checkbox("snap.enabled", "Snap panels", snap.enabled),
            choice("snap.grid", "Snap grid", steps, selected),
            slider(
                "snap.edge",
                "Edge snap distance",
                snap.edge_distance,
                SNAP_EDGE_RANGE,
                0,
            ),
        ],
    ));

    let panels = &settings.panels;
    sections.push(section(
        "Panels",
        vec![
            checkbox(
                "panels.confirm_close",
                "Confirm before closing",
                panels.confirm_close,
            ),
            checkbox(
                "panels.terminal_user_config",
                "Terminals use Ghostty config",
                panels.terminal_user_config,
            ),
            slider(
                "panels.terminal_width",
                "New terminal width",
                panels.default_terminal_width,
                TERMINAL_WIDTH_RANGE,
                0,
            ),
            slider(
                "panels.terminal_height",
                "New terminal height",
                panels.default_terminal_height,
                TERMINAL_HEIGHT_RANGE,
                0,
            ),
            slider(
                "panels.preview_zoom",
                "Preview below zoom",
                panels.preview_below_zoom,
                PREVIEW_ZOOM_RANGE,
                2,
            ),
        ],
    ));

    sections.push(section(
        "Developer",
        vec![
            checkbox("developer.show_fps", "Show FPS", context.show_fps),
            note(
                "developer.path",
                "Settings file",
                context.settings_path.clone(),
            ),
            button("developer.open_folder", "", "Open Config Folder"),
            button("developer.reset", "", "Reset to Defaults"),
        ],
    ));
    sections
}

fn background_fields(settings: &Settings, context: &FormContext) -> Vec<Field> {
    let background = &settings.background;
    let kinds = BackgroundKind::ALL
        .iter()
        .map(|kind| kind.label().to_string())
        .collect();
    let selected = BackgroundKind::ALL
        .iter()
        .position(|kind| *kind == background.kind)
        .unwrap_or(0);
    let mut fields = vec![choice("background.kind", "Type", kinds, selected)];
    match background.kind {
        BackgroundKind::Solid => {
            fields.push(color("background.solid", "Color", background.solid));
        }
        BackgroundKind::Gradient => {
            let gradient = &background.gradient;
            let kinds = vec!["Linear".to_string(), "Radial".to_string()];
            let selected = GRADIENT_KINDS
                .iter()
                .position(|kind| *kind == gradient.kind)
                .unwrap_or(0);
            fields.push(choice("gradient.kind", "Style", kinds, selected));
            if gradient.kind == GradientKind::Linear {
                fields.push(slider(
                    "gradient.angle",
                    "Angle",
                    gradient.angle_deg,
                    0.0..=360.0,
                    0,
                ));
            }
            let removable = gradient.stops.len() > MIN_GRADIENT_STOPS;
            for (index, stop) in gradient.stops.iter().enumerate() {
                let number = index + 1;
                fields.push(color(
                    &format!("gradient.stop.{index}.color"),
                    &format!("Stop {number} color"),
                    stop.color,
                ));
                fields.push(slider(
                    &format!("gradient.stop.{index}.position"),
                    &format!("Stop {number} position"),
                    stop.position,
                    0.0..=1.0,
                    2,
                ));
                if removable {
                    fields.push(button(
                        &format!("gradient.stop.{index}.remove"),
                        "",
                        &format!("Remove Stop {number}"),
                    ));
                }
            }
            if gradient.stops.len() < MAX_GRADIENT_STOPS {
                fields.push(button("gradient.add_stop", "", "Add Stop"));
            }
        }
        BackgroundKind::Image => {
            let image = &background.image;
            let path = image
                .path
                .as_ref()
                .map_or("No image chosen".to_string(), |path| {
                    path.display().to_string()
                });
            fields.push(note("image.path", "File", path));
            if let Some(error) = &context.image_error {
                fields.push(note("image.error", "Problem", error.clone()));
            }
            fields.push(button("image.choose", "", "Choose Image..."));
            if image.path.is_some() {
                fields.push(button("image.clear", "", "Clear Image"));
            }
            let fits = ImageFit::ALL
                .iter()
                .map(|fit| fit.label().to_string())
                .collect();
            let selected = ImageFit::ALL
                .iter()
                .position(|fit| *fit == image.fit)
                .unwrap_or(0);
            fields.push(choice("image.fit", "Fit", fits, selected));
            let filters = &image.filters;
            let factor = 0.0..=MAX_FILTER_FACTOR;
            fields.extend([
                slider("image.blur", "Blur", filters.blur, 0.0..=MAX_BLUR, 0),
                slider(
                    "image.brightness",
                    "Brightness",
                    filters.brightness,
                    factor.clone(),
                    2,
                ),
                slider(
                    "image.contrast",
                    "Contrast",
                    filters.contrast,
                    factor.clone(),
                    2,
                ),
                slider(
                    "image.saturation",
                    "Saturation",
                    filters.saturation,
                    factor,
                    2,
                ),
                checkbox("image.grayscale", "Grayscale", filters.grayscale),
                slider("image.dim", "Dim", filters.dim, 0.0..=1.0, 2),
                color("background.solid", "Fill color", background.solid),
            ]);
        }
    }
    fields
}

/// Applies one control change to `settings`. Unknown ids and mismatched values are errors:
/// they mean the form and this function disagree, which must not pass silently.
pub fn apply_field(
    settings: &mut Settings,
    id: &str,
    value: FieldValue,
) -> Result<FormOutcome, String> {
    let changed = Ok(FormOutcome::Changed { rebuild: false });
    let rebuilt = Ok(FormOutcome::Changed { rebuild: true });
    let mismatch = || Err(format!("settings field {id:?} got {value:?}"));
    let number = |range: RangeInclusive<f64>, decimals: u8| match value {
        FieldValue::Number(n) if n.is_finite() => {
            let scale = 10f64.powi(i32::from(decimals));
            Some(((n * scale).round() / scale).clamp(*range.start(), *range.end()))
        }
        _ => None,
    };
    let background = &mut settings.background;

    if let Some(rest) = id.strip_prefix("gradient.stop.") {
        let (index, part) = rest
            .split_once('.')
            .and_then(|(index, part)| Some((index.parse::<usize>().ok()?, part)))
            .ok_or_else(|| format!("unknown settings field {id:?}"))?;
        let stops = &mut background.gradient.stops;
        if index >= stops.len() {
            return Err(format!("settings field {id:?}: no stop {index}"));
        }
        return match (part, value) {
            ("color", FieldValue::Color(color)) => {
                stops[index].color = color;
                changed
            }
            ("position", FieldValue::Number(_)) => {
                stops[index].position = number(0.0..=1.0, 2).unwrap_or_default();
                changed
            }
            ("remove", FieldValue::Pressed) if stops.len() > MIN_GRADIENT_STOPS => {
                stops.remove(index);
                rebuilt
            }
            ("remove", FieldValue::Pressed) => Err(format!(
                "settings field {id:?}: a gradient keeps at least {MIN_GRADIENT_STOPS} stops"
            )),
            ("color" | "position" | "remove", _) => mismatch(),
            _ => Err(format!("unknown settings field {id:?}")),
        };
    }

    match (id, value) {
        ("background.kind", FieldValue::Index(index)) => {
            background.kind = *BackgroundKind::ALL
                .get(index)
                .ok_or("bad background kind")?;
            rebuilt
        }
        ("background.solid", FieldValue::Color(color)) => {
            background.solid = color;
            changed
        }
        ("gradient.kind", FieldValue::Index(index)) => {
            background.gradient.kind = *GRADIENT_KINDS.get(index).ok_or("bad gradient kind")?;
            rebuilt
        }
        ("gradient.angle", FieldValue::Number(_)) => {
            background.gradient.angle_deg = number(0.0..=360.0, 0).unwrap_or_default();
            changed
        }
        ("gradient.add_stop", FieldValue::Pressed) => {
            add_stop(settings)?;
            rebuilt
        }
        ("image.choose", FieldValue::Pressed) => Ok(FormOutcome::Command(FormCommand::ChooseImage)),
        ("image.clear", FieldValue::Pressed) => {
            background.image.path = None;
            rebuilt
        }
        ("image.fit", FieldValue::Index(index)) => {
            background.image.fit = *ImageFit::ALL.get(index).ok_or("bad image fit")?;
            changed
        }
        ("image.blur", FieldValue::Number(_)) => {
            background.image.filters.blur = number(0.0..=MAX_BLUR, 0).unwrap_or_default();
            changed
        }
        ("image.brightness", FieldValue::Number(_)) => {
            background.image.filters.brightness = number(0.0..=MAX_FILTER_FACTOR, 2).unwrap_or(1.0);
            changed
        }
        ("image.contrast", FieldValue::Number(_)) => {
            background.image.filters.contrast = number(0.0..=MAX_FILTER_FACTOR, 2).unwrap_or(1.0);
            changed
        }
        ("image.saturation", FieldValue::Number(_)) => {
            background.image.filters.saturation = number(0.0..=MAX_FILTER_FACTOR, 2).unwrap_or(1.0);
            changed
        }
        ("image.grayscale", FieldValue::Bool(on)) => {
            background.image.filters.grayscale = on;
            changed
        }
        ("image.dim", FieldValue::Number(_)) => {
            background.image.filters.dim = number(0.0..=1.0, 2).unwrap_or_default();
            changed
        }
        ("grid.show", FieldValue::Bool(on)) => {
            settings.grid.show = on;
            changed
        }
        ("grid.spacing", FieldValue::Number(_)) => {
            settings.grid.spacing = number(GRID_SPACING_RANGE, 0).unwrap_or(64.0);
            changed
        }
        ("grid.color", FieldValue::Color(color)) => {
            settings.grid.color = color;
            changed
        }
        ("grid.opacity", FieldValue::Number(_)) => {
            settings.grid.opacity = number(0.0..=1.0, 2).unwrap_or_default();
            changed
        }
        ("grid.kind", FieldValue::Index(index)) => {
            settings.grid.kind = *GridKind::ALL.get(index).ok_or("bad grid style")?;
            // The mark size control comes and goes with the style.
            rebuilt
        }
        ("grid.mark_size", FieldValue::Number(_)) => {
            settings.grid.mark_size = number(GRID_MARK_SIZE_RANGE, 0).unwrap_or(2.0);
            changed
        }
        ("grid.major_every", FieldValue::Number(_)) => {
            let every = number(0.0..=f64::from(GRID_MAJOR_EVERY_MAX), 0).unwrap_or_default();
            // 1 would make every line major; the slider's low end means off.
            settings.grid.major_every = if every < 2.0 { 0 } else { every as u32 };
            changed
        }
        ("grid.major_opacity", FieldValue::Number(_)) => {
            settings.grid.major_opacity = number(0.0..=1.0, 2).unwrap_or_default();
            changed
        }
        ("snap.enabled", FieldValue::Bool(on)) => {
            settings.snap.enabled = on;
            changed
        }
        ("snap.grid", FieldValue::Index(index)) => {
            settings.snap.grid = *SNAP_GRID_STEPS.get(index).ok_or("bad snap grid")?;
            changed
        }
        ("snap.edge", FieldValue::Number(_)) => {
            settings.snap.edge_distance = number(SNAP_EDGE_RANGE, 0).unwrap_or_default();
            changed
        }
        ("canvas.click_empty_clears_focus", FieldValue::Bool(on)) => {
            settings.canvas.click_empty_clears_focus = on;
            changed
        }
        ("panels.confirm_close", FieldValue::Bool(on)) => {
            settings.panels.confirm_close = on;
            changed
        }
        ("panels.terminal_user_config", FieldValue::Bool(on)) => {
            settings.panels.terminal_user_config = on;
            changed
        }
        ("panels.terminal_width", FieldValue::Number(_)) => {
            settings.panels.default_terminal_width =
                number(TERMINAL_WIDTH_RANGE, 0).unwrap_or(640.0);
            changed
        }
        ("panels.terminal_height", FieldValue::Number(_)) => {
            settings.panels.default_terminal_height =
                number(TERMINAL_HEIGHT_RANGE, 0).unwrap_or(400.0);
            changed
        }
        ("panels.preview_zoom", FieldValue::Number(_)) => {
            settings.panels.preview_below_zoom = number(PREVIEW_ZOOM_RANGE, 2).unwrap_or(0.5);
            changed
        }
        ("developer.show_fps", FieldValue::Bool(on)) => {
            Ok(FormOutcome::Command(FormCommand::SetShowFps(on)))
        }
        ("developer.open_folder", FieldValue::Pressed) => {
            Ok(FormOutcome::Command(FormCommand::OpenConfigFolder))
        }
        ("developer.reset", FieldValue::Pressed) => {
            *settings = Settings::default();
            rebuilt
        }
        (
            "background.kind"
            | "background.solid"
            | "gradient.kind"
            | "gradient.angle"
            | "gradient.add_stop"
            | "image.choose"
            | "image.clear"
            | "image.fit"
            | "image.blur"
            | "image.brightness"
            | "image.contrast"
            | "image.saturation"
            | "image.grayscale"
            | "image.dim"
            | "grid.show"
            | "grid.spacing"
            | "grid.color"
            | "grid.opacity"
            | "grid.kind"
            | "grid.mark_size"
            | "grid.major_every"
            | "grid.major_opacity"
            | "canvas.click_empty_clears_focus"
            | "snap.enabled"
            | "snap.grid"
            | "snap.edge"
            | "panels.confirm_close"
            | "panels.terminal_user_config"
            | "panels.terminal_width"
            | "panels.terminal_height"
            | "panels.preview_zoom"
            | "developer.show_fps"
            | "developer.open_folder"
            | "developer.reset",
            _,
        ) => mismatch(),
        _ => Err(format!("unknown settings field {id:?}")),
    }
}

/// Adds a stop in the middle of the widest gap, colored as the gradient already is there,
/// so adding one does not change the look until it is edited.
fn add_stop(settings: &mut Settings) -> Result<(), String> {
    let gradient = &mut settings.background.gradient;
    if gradient.stops.len() >= MAX_GRADIENT_STOPS {
        return Err(format!("a gradient has at most {MAX_GRADIENT_STOPS} stops"));
    }
    let mut positions: Vec<f64> = gradient.stops.iter().map(|stop| stop.position).collect();
    positions.push(0.0);
    positions.push(1.0);
    positions.sort_by(f64::total_cmp);
    let (low, high) = positions
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
        .unwrap_or((0.0, 1.0));
    let position = ((low + high) / 2.0 * 100.0).round() / 100.0;
    let color = gradient.color_at(position);
    gradient.stops.push(GradientStop { color, position });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ids(sections: &[Section]) -> Vec<String> {
        sections
            .iter()
            .flat_map(|section| section.fields.iter().map(|field| field.id.clone()))
            .collect()
    }

    fn settings_with(kind: BackgroundKind) -> Settings {
        let mut settings = Settings::default();
        settings.background.kind = kind;
        settings.background.image.path = Some(PathBuf::from("/tmp/a.png"));
        settings.background.gradient.stops.push(GradientStop {
            color: Rgb(0xff0000),
            position: 0.5,
        });
        settings
    }

    /// The value a user could send from this control.
    fn sample_value(control: &Control) -> Option<FieldValue> {
        match control {
            Control::Checkbox(on) => Some(FieldValue::Bool(!on)),
            Control::Slider { min, max, .. } => Some(FieldValue::Number((min + max) / 2.0)),
            Control::Color(_) => Some(FieldValue::Color(Rgb(0x123456))),
            Control::Choice { options, .. } => Some(FieldValue::Index(options.len() - 1)),
            Control::Button(_) => Some(FieldValue::Pressed),
            Control::Note(_) => None,
        }
    }

    #[test]
    fn every_control_in_every_form_applies_and_stays_valid() {
        let context = FormContext {
            notice: Some("moved".into()),
            image_error: Some("bad file".into()),
            ..FormContext::default()
        };
        for kind in BackgroundKind::ALL {
            let base = settings_with(kind);
            let form = settings_form(&base, &context);
            let mut seen = std::collections::HashSet::new();
            for field in form.iter().flat_map(|section| &section.fields) {
                assert!(seen.insert(field.id.clone()), "duplicate id {}", field.id);
                let Some(value) = sample_value(&field.control) else {
                    continue;
                };
                let mut settings = base.clone();
                let outcome = apply_field(&mut settings, &field.id, value);
                assert!(outcome.is_ok(), "{kind:?} {}: {outcome:?}", field.id);
                assert_eq!(settings.validate(), Ok(()), "{kind:?} {}", field.id);
            }
        }
    }

    #[test]
    fn form_shows_only_the_selected_background_kind() {
        let context = FormContext::default();
        let solid = ids(&settings_form(
            &settings_with(BackgroundKind::Solid),
            &context,
        ));
        assert!(solid.contains(&"background.solid".to_string()));
        assert!(!solid.iter().any(|id| id.starts_with("gradient.")));
        assert!(!solid.iter().any(|id| id.starts_with("image.")));
        assert!(!solid.contains(&"notice".to_string()));

        let gradient = ids(&settings_form(
            &settings_with(BackgroundKind::Gradient),
            &context,
        ));
        for id in [
            "gradient.kind",
            "gradient.angle",
            "gradient.stop.2.color",
            "gradient.stop.2.position",
            "gradient.stop.0.remove",
            "gradient.add_stop",
        ] {
            assert!(gradient.contains(&id.to_string()), "{id}");
        }

        let image = ids(&settings_form(
            &settings_with(BackgroundKind::Image),
            &context,
        ));
        for id in [
            "image.choose",
            "image.clear",
            "image.fit",
            "image.blur",
            "image.brightness",
            "image.contrast",
            "image.saturation",
            "image.grayscale",
            "image.dim",
        ] {
            assert!(image.contains(&id.to_string()), "{id}");
        }
        for section in ["Grid", "Canvas and Snapping", "Panels", "Developer"] {
            assert!(
                settings_form(&Settings::default(), &context)
                    .iter()
                    .any(|s| s.title == section),
                "{section}"
            );
        }
    }

    #[test]
    fn unknown_ids_and_wrong_values_are_errors() {
        let mut settings = Settings::default();
        let before = settings.clone();
        for (id, value) in [
            ("nope", FieldValue::Pressed),
            ("grid.show", FieldValue::Number(1.0)),
            ("grid.spacing", FieldValue::Bool(true)),
            ("gradient.stop.9.color", FieldValue::Color(Rgb(0))),
            ("gradient.stop.x.color", FieldValue::Color(Rgb(0))),
            ("gradient.stop.0.color", FieldValue::Pressed),
            ("gradient.stop.0.remove", FieldValue::Pressed),
            ("background.kind", FieldValue::Index(7)),
        ] {
            assert!(apply_field(&mut settings, id, value).is_err(), "{id}");
        }
        assert_eq!(settings, before, "a rejected change must not edit settings");
    }

    #[test]
    fn grid_style_controls_edit_the_grid() {
        let context = FormContext::default();
        let mut settings = Settings::default();
        let lines = ids(&settings_form(&settings, &context));
        for id in [
            "grid.kind",
            "grid.color",
            "grid.opacity",
            "grid.major_every",
        ] {
            assert!(lines.contains(&id.to_string()), "{id}");
        }
        assert!(
            !lines.contains(&"grid.mark_size".to_string()),
            "lines have no marks"
        );
        // Index 1 is Dots; the mark size control appears, so the form rebuilds.
        assert_eq!(
            apply_field(&mut settings, "grid.kind", FieldValue::Index(1)),
            Ok(FormOutcome::Changed { rebuild: true })
        );
        assert_eq!(settings.grid.kind, GridKind::Dots);
        let dots = ids(&settings_form(&settings, &context));
        assert!(dots.contains(&"grid.mark_size".to_string()));
        apply_field(&mut settings, "grid.mark_size", FieldValue::Number(3.4)).unwrap();
        assert_eq!(settings.grid.mark_size, 3.0);
        apply_field(&mut settings, "grid.mark_size", FieldValue::Number(99.0)).unwrap();
        assert_eq!(settings.grid.mark_size, 4.0);
        apply_field(&mut settings, "grid.major_every", FieldValue::Number(7.6)).unwrap();
        assert_eq!(settings.grid.major_every, 8);
        apply_field(&mut settings, "grid.major_every", FieldValue::Number(1.0)).unwrap();
        assert_eq!(
            settings.grid.major_every, 0,
            "1 means off, never every line"
        );
        apply_field(
            &mut settings,
            "grid.major_opacity",
            FieldValue::Number(0.333),
        )
        .unwrap();
        assert_eq!(settings.grid.major_opacity, 0.33);
        assert_eq!(settings.validate(), Ok(()));
        let before = settings.clone();
        assert!(apply_field(&mut settings, "grid.kind", FieldValue::Index(4)).is_err());
        assert!(apply_field(&mut settings, "grid.mark_size", FieldValue::Bool(true)).is_err());
        assert_eq!(settings, before);
        // Every grid control applies from every style and keeps the settings valid.
        for kind in GridKind::ALL {
            let mut settings = Settings::default();
            settings.grid.kind = kind;
            for field in settings_form(&settings.clone(), &context)
                .iter()
                .filter(|section| section.title == "Grid")
                .flat_map(|section| &section.fields)
            {
                let value = sample_value(&field.control).unwrap();
                apply_field(&mut settings, &field.id, value).unwrap();
                assert_eq!(settings.validate(), Ok(()), "{kind:?} {}", field.id);
            }
        }
    }

    #[test]
    fn sliders_clamp_and_round() {
        let mut settings = Settings::default();
        apply_field(&mut settings, "grid.opacity", FieldValue::Number(0.456)).unwrap();
        assert_eq!(settings.grid.opacity, 0.46);
        apply_field(&mut settings, "grid.spacing", FieldValue::Number(9999.0)).unwrap();
        assert_eq!(settings.grid.spacing, 256.0);
        apply_field(&mut settings, "image.blur", FieldValue::Number(f64::NAN)).unwrap();
        assert_eq!(settings.background.image.filters.blur, 0.0);
        assert_eq!(settings.validate(), Ok(()));
    }

    #[test]
    fn outcomes_say_when_to_rebuild() {
        let mut settings = Settings::default();
        assert_eq!(
            apply_field(&mut settings, "background.kind", FieldValue::Index(1)),
            Ok(FormOutcome::Changed { rebuild: true })
        );
        assert_eq!(settings.background.kind, BackgroundKind::Gradient);
        assert_eq!(
            apply_field(&mut settings, "background.solid", FieldValue::Color(Rgb(1))),
            Ok(FormOutcome::Changed { rebuild: false })
        );
        assert_eq!(
            apply_field(&mut settings, "image.choose", FieldValue::Pressed),
            Ok(FormOutcome::Command(FormCommand::ChooseImage))
        );
        assert_eq!(
            apply_field(&mut settings, "developer.show_fps", FieldValue::Bool(true)),
            Ok(FormOutcome::Command(FormCommand::SetShowFps(true)))
        );
        settings.grid.show = true;
        apply_field(&mut settings, "developer.reset", FieldValue::Pressed).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn stops_add_in_the_widest_gap_and_respect_limits() {
        let mut settings = Settings::default();
        let before = settings.background.gradient.color_at(0.5);
        apply_field(&mut settings, "gradient.add_stop", FieldValue::Pressed).unwrap();
        let stops = &settings.background.gradient.stops;
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[2].position, 0.5);
        assert_eq!(stops[2].color, before, "adding a stop keeps the look");
        while settings.background.gradient.stops.len() < MAX_GRADIENT_STOPS {
            apply_field(&mut settings, "gradient.add_stop", FieldValue::Pressed).unwrap();
        }
        assert!(apply_field(&mut settings, "gradient.add_stop", FieldValue::Pressed).is_err());
        assert_eq!(settings.validate(), Ok(()));
        while settings.background.gradient.stops.len() > MIN_GRADIENT_STOPS {
            apply_field(&mut settings, "gradient.stop.0.remove", FieldValue::Pressed).unwrap();
        }
        assert!(apply_field(&mut settings, "gradient.stop.0.remove", FieldValue::Pressed).is_err());
    }
}
