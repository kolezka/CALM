//! Panel chrome geometry in world units: the title bar, its icon, the free middle area
//! and the window buttons. Paint and hit testing both read this, so they cannot drift.

use crate::geometry::{Point, Rect};
use crate::scene::{CHROME_HEIGHT, Panel, PanelId, PanelKind, Scene, chip_rect};

/// Height of the title bar. Same as the strip `content_rect` leaves above the content.
pub const TITLE_BAR_HEIGHT: f64 = CHROME_HEIGHT;
/// Radius of the title bar's top corners and of a minimized chip.
pub const CORNER_RADIUS: f64 = 8.0;
/// Side of each square button hit target.
pub const BUTTON_HIT_SIZE: f64 = 20.0;
/// Inset of the buttons from the bar's top, bottom and right edge, and between them.
pub const BUTTON_MARGIN: f64 = (TITLE_BAR_HEIGHT - BUTTON_HIT_SIZE) / 2.0;
/// Diameter of the visible circle inside each button hit target.
pub const BUTTON_DOT_DIAMETER: f64 = 14.0;
/// Space between the bar's left edge and the icon.
pub const LEADING_PADDING: f64 = 10.0;
pub const ICON_SIZE: f64 = 14.0;
/// Space between the icon and the title, and between the title and the buttons.
pub const ICON_GAP: f64 = 8.0;
/// Below this bar height on screen, in pixels, icon, title and buttons are not drawn.
pub const MIN_DETAIL_PX: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeButton {
    Close,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelIcon {
    Terminal,
    Browser,
    Plugin,
}

impl PanelIcon {
    pub fn for_kind(kind: &PanelKind) -> Self {
        match kind {
            PanelKind::Terminal { .. } | PanelKind::Tui { .. } => PanelIcon::Terminal,
            PanelKind::Browser { .. } => PanelIcon::Browser,
            // A placeholder for a kind from a newer CALM; a plugin icon is the closest fit.
            PanelKind::Plugin { .. } | PanelKind::Unsupported { .. } => PanelIcon::Plugin,
        }
    }
}

/// Where each part of a panel's title bar sits, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBarLayout {
    /// The whole bar. Everything in it except the buttons drags the panel.
    pub bar: Rect,
    pub icon: Rect,
    /// Between the icon and the buttons: the title today, room for an address field later.
    pub middle: Rect,
    /// `None` on a minimized chip, which has no buttons.
    pub minimize: Option<Rect>,
    pub close: Option<Rect>,
}

pub fn title_bar_layout(panel: &Panel) -> TitleBarLayout {
    let bar = if panel.minimized {
        chip_rect(panel)
    } else {
        Rect::new(
            panel.rect.origin.x,
            panel.rect.origin.y,
            panel.rect.size.width,
            TITLE_BAR_HEIGHT,
        )
    };
    let icon = Rect::new(
        bar.origin.x + LEADING_PADDING,
        bar.origin.y + (bar.size.height - ICON_SIZE) / 2.0,
        ICON_SIZE,
        ICON_SIZE,
    );
    let (minimize, close, trailing_edge) = if panel.minimized {
        (None, None, bar.right() - LEADING_PADDING)
    } else {
        let top = bar.origin.y + BUTTON_MARGIN;
        let close_left = bar.right() - BUTTON_MARGIN - BUTTON_HIT_SIZE;
        let minimize_left = close_left - BUTTON_MARGIN - BUTTON_HIT_SIZE;
        (
            Some(Rect::new(
                minimize_left,
                top,
                BUTTON_HIT_SIZE,
                BUTTON_HIT_SIZE,
            )),
            Some(Rect::new(close_left, top, BUTTON_HIT_SIZE, BUTTON_HIT_SIZE)),
            minimize_left - ICON_GAP,
        )
    };
    let middle_left = icon.right() + ICON_GAP;
    let middle = Rect::new(
        middle_left,
        bar.origin.y,
        (trailing_edge - middle_left).max(0.0),
        bar.size.height,
    );
    TitleBarLayout {
        bar,
        icon,
        middle,
        minimize,
        close,
    }
}

/// The visible circle centered in a button's hit target.
pub fn button_dot(hit: Rect) -> Rect {
    let inset = (hit.size.width - BUTTON_DOT_DIAMETER) / 2.0;
    Rect::new(
        hit.origin.x + inset,
        hit.origin.y + inset,
        BUTTON_DOT_DIAMETER,
        BUTTON_DOT_DIAMETER,
    )
}

/// Close is the rightmost button of the bar, Minimize sits to its left.
pub fn chrome_hit(panel: &Panel, world: Point) -> Option<ChromeButton> {
    let layout = title_bar_layout(panel);
    if layout.close.is_some_and(|rect| rect.contains(world)) {
        Some(ChromeButton::Close)
    } else if layout.minimize.is_some_and(|rect| rect.contains(world)) {
        Some(ChromeButton::Minimize)
    } else {
        None
    }
}

/// The panel under the pointer and the button under it, if any. Drives hover styling.
pub fn hover_target(scene: &Scene, world: Point) -> Option<(PanelId, Option<ChromeButton>)> {
    let id = scene.hit_test(world)?;
    let panel = scene.get(id)?;
    Some((id, chrome_hit(panel, world)))
}

/// True when the bar is tall enough on screen for its icon, title and buttons to read.
pub fn details_visible(zoom: f64) -> bool {
    TITLE_BAR_HEIGHT * zoom >= MIN_DETAIL_PX
}

/// Stroke polylines for an icon, in unit square coordinates (0..1, y down).
pub fn icon_strokes(icon: PanelIcon) -> Vec<Vec<(f64, f64)>> {
    match icon {
        // A prompt chevron and a cursor underline.
        PanelIcon::Terminal => vec![
            vec![(0.12, 0.22), (0.46, 0.5), (0.12, 0.78)],
            vec![(0.56, 0.8), (0.9, 0.8)],
        ],
        // A globe: outline, equator and meridian.
        PanelIcon::Browser => {
            let segments = 24;
            let circle = (0..=segments)
                .map(|step| {
                    let angle = f64::from(step) / f64::from(segments) * std::f64::consts::TAU;
                    (0.5 + 0.42 * angle.cos(), 0.5 + 0.42 * angle.sin())
                })
                .collect();
            vec![
                circle,
                vec![(0.08, 0.5), (0.92, 0.5)],
                vec![(0.5, 0.08), (0.5, 0.92)],
            ]
        }
        // A diamond with a center bar: a building block.
        PanelIcon::Plugin => vec![
            vec![
                (0.5, 0.08),
                (0.92, 0.5),
                (0.5, 0.92),
                (0.08, 0.5),
                (0.5, 0.08),
            ],
            vec![(0.34, 0.5), (0.66, 0.5)],
        ],
    }
}

/// Stroke polylines for a button's glyph, in unit square coordinates of its dot.
pub fn button_strokes(button: ChromeButton) -> Vec<Vec<(f64, f64)>> {
    match button {
        ChromeButton::Close => vec![
            vec![(0.33, 0.33), (0.67, 0.67)],
            vec![(0.67, 0.33), (0.33, 0.67)],
        ],
        ChromeButton::Minimize => vec![vec![(0.3, 0.5), (0.7, 0.5)]],
    }
}

/// `#rrggbb` or `#rrggbbaa` to 0xRRGGBBAA.
pub fn parse_hex_color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(if hex.len() == 6 {
        (value << 8) | 0xff
    } else {
        value
    })
}

/// Per-panel chrome colors, 0xRRGGBBAA. `None` falls back to the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChromeOverrides {
    /// Focus ring, join divider next to the focused panel, and icon tint.
    pub accent: Option<u32>,
    pub title_bar: Option<u32>,
    pub title: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeState {
    Idle,
    Focused,
    /// A close confirmation is up. Theme colors win so the warning always reads.
    Confirming,
}

impl ChromeState {
    pub fn of(focused: bool, confirming: bool) -> Self {
        match (confirming, focused) {
            (true, _) => ChromeState::Confirming,
            (false, true) => ChromeState::Focused,
            (false, false) => ChromeState::Idle,
        }
    }
}

/// Theme chrome colors, 0xRRGGBBAA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromePalette {
    pub bar: u32,
    pub bar_focused: u32,
    pub border: u32,
    pub accent: u32,
    pub title: u32,
    pub title_focused: u32,
    pub confirm_bar: u32,
    pub confirm_border: u32,
    pub confirm_title: u32,
}

/// What one panel's chrome is painted with this frame, 0xRRGGBBAA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromeColors {
    pub bar: u32,
    pub border: u32,
    pub title: u32,
}

pub fn chrome_colors(
    state: ChromeState,
    overrides: &ChromeOverrides,
    palette: &ChromePalette,
) -> ChromeColors {
    match state {
        ChromeState::Confirming => ChromeColors {
            bar: palette.confirm_bar,
            border: palette.confirm_border,
            title: palette.confirm_title,
        },
        ChromeState::Focused => ChromeColors {
            bar: overrides.title_bar.unwrap_or(palette.bar_focused),
            border: overrides.accent.unwrap_or(palette.accent),
            title: overrides.title.unwrap_or(palette.title_focused),
        },
        ChromeState::Idle => ChromeColors {
            bar: overrides.title_bar.unwrap_or(palette.bar),
            border: palette.border,
            title: overrides.title.unwrap_or(palette.title),
        },
    }
}

/// The icon's color: the accent if set, else the kind's theme color.
pub fn icon_tint(overrides: &ChromeOverrides, kind_color: u32) -> u32 {
    overrides.accent.unwrap_or(kind_color)
}

/// The divider between two joined panels. A confirmation on either side wins, then the
/// focused side's accent, then the plain border.
pub fn divider_color(sides: [(ChromeState, &ChromeOverrides); 2], palette: &ChromePalette) -> u32 {
    if sides
        .iter()
        .any(|(state, _)| *state == ChromeState::Confirming)
    {
        return palette.confirm_border;
    }
    sides
        .iter()
        .find(|(state, _)| *state == ChromeState::Focused)
        .map_or(palette.border, |(_, overrides)| {
            overrides.accent.unwrap_or(palette.accent)
        })
}

#[cfg(test)]
mod color_tests {
    use super::*;

    const PALETTE: ChromePalette = ChromePalette {
        bar: 0x01,
        bar_focused: 0x02,
        border: 0x03,
        accent: 0x04,
        title: 0x05,
        title_focused: 0x06,
        confirm_bar: 0x07,
        confirm_border: 0x08,
        confirm_title: 0x09,
    };
    const CUSTOM: ChromeOverrides = ChromeOverrides {
        accent: Some(0xa0),
        title_bar: Some(0xb0),
        title: Some(0xc0),
    };
    const NONE: ChromeOverrides = ChromeOverrides {
        accent: None,
        title_bar: None,
        title: None,
    };

    #[test]
    fn no_overrides_use_the_theme() {
        let colors = |state| chrome_colors(state, &NONE, &PALETTE);
        assert_eq!(
            colors(ChromeState::Idle),
            ChromeColors {
                bar: 0x01,
                border: 0x03,
                title: 0x05
            }
        );
        assert_eq!(
            colors(ChromeState::Focused),
            ChromeColors {
                bar: 0x02,
                border: 0x04,
                title: 0x06
            }
        );
        assert_eq!(icon_tint(&NONE, 0x77), 0x77);
        assert_eq!(ChromeOverrides::default(), NONE);
    }

    #[test]
    fn overrides_replace_bar_title_and_focused_ring() {
        assert_eq!(
            chrome_colors(ChromeState::Focused, &CUSTOM, &PALETTE),
            ChromeColors {
                bar: 0xb0,
                border: 0xa0,
                title: 0xc0
            }
        );
        // Unfocused panels keep the plain border: the accent marks focus.
        assert_eq!(
            chrome_colors(ChromeState::Idle, &CUSTOM, &PALETTE),
            ChromeColors {
                bar: 0xb0,
                border: 0x03,
                title: 0xc0
            }
        );
        assert_eq!(icon_tint(&CUSTOM, 0x77), 0xa0);
    }

    #[test]
    fn each_override_is_independent() {
        let only_bar = ChromeOverrides {
            title_bar: Some(0xb0),
            ..NONE
        };
        assert_eq!(
            chrome_colors(ChromeState::Focused, &only_bar, &PALETTE),
            ChromeColors {
                bar: 0xb0,
                border: 0x04,
                title: 0x06
            }
        );
        let only_title = ChromeOverrides {
            title: Some(0xc0),
            ..NONE
        };
        assert_eq!(
            chrome_colors(ChromeState::Idle, &only_title, &PALETTE),
            ChromeColors {
                bar: 0x01,
                border: 0x03,
                title: 0xc0
            }
        );
    }

    #[test]
    fn confirmation_ignores_overrides() {
        assert_eq!(
            chrome_colors(ChromeState::Confirming, &CUSTOM, &PALETTE),
            ChromeColors {
                bar: 0x07,
                border: 0x08,
                title: 0x09
            }
        );
    }

    #[test]
    fn state_of_prefers_confirmation() {
        assert_eq!(ChromeState::of(true, true), ChromeState::Confirming);
        assert_eq!(ChromeState::of(true, false), ChromeState::Focused);
        assert_eq!(ChromeState::of(false, false), ChromeState::Idle);
    }

    #[test]
    fn divider_follows_the_focused_side() {
        use ChromeState::*;
        assert_eq!(
            divider_color([(Idle, &CUSTOM), (Idle, &NONE)], &PALETTE),
            0x03
        );
        assert_eq!(
            divider_color([(Idle, &NONE), (Focused, &CUSTOM)], &PALETTE),
            0xa0
        );
        assert_eq!(
            divider_color([(Focused, &NONE), (Idle, &CUSTOM)], &PALETTE),
            0x04
        );
        assert_eq!(
            divider_color([(Focused, &CUSTOM), (Confirming, &NONE)], &PALETTE),
            0x08
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{MINIMIZED_CHIP, content_rect};

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
        }
    }

    fn scene_with_panel() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(100.0, 50.0, 400.0, 300.0));
        (scene, id)
    }

    #[test]
    fn bar_sits_exactly_above_content() {
        let (scene, id) = scene_with_panel();
        let panel = scene.get(id).unwrap();
        let layout = title_bar_layout(panel);
        let content = content_rect(panel);
        assert_eq!(layout.bar, Rect::new(100.0, 50.0, 400.0, TITLE_BAR_HEIGHT));
        assert_eq!(layout.bar.bottom(), content.origin.y);
        assert_eq!(layout.bar.size.width, content.size.width);
    }

    #[test]
    fn every_part_stays_inside_the_bar_and_parts_do_not_overlap() {
        let (scene, id) = scene_with_panel();
        let layout = title_bar_layout(scene.get(id).unwrap());
        let close = layout.close.unwrap();
        let minimize = layout.minimize.unwrap();
        for part in [layout.icon, layout.middle, close, minimize] {
            assert!(part.origin.x >= layout.bar.origin.x, "{part:?}");
            assert!(part.origin.y >= layout.bar.origin.y, "{part:?}");
            assert!(part.right() <= layout.bar.right(), "{part:?}");
            assert!(part.bottom() <= layout.bar.bottom(), "{part:?}");
        }
        assert!(layout.icon.right() < layout.middle.origin.x);
        assert!(layout.middle.right() < minimize.origin.x);
        assert!(minimize.right() < close.origin.x);
        assert_eq!(close.right(), layout.bar.right() - BUTTON_MARGIN);
    }

    #[test]
    fn hit_matches_the_layout_buttons() {
        let (scene, id) = scene_with_panel();
        let panel = scene.get(id).unwrap();
        let layout = title_bar_layout(panel);
        let center = |rect: Rect| {
            Point::new(
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height / 2.0,
            )
        };
        assert_eq!(
            chrome_hit(panel, center(layout.close.unwrap())),
            Some(ChromeButton::Close)
        );
        assert_eq!(
            chrome_hit(panel, center(layout.minimize.unwrap())),
            Some(ChromeButton::Minimize)
        );
        assert_eq!(chrome_hit(panel, center(layout.middle)), None);
        assert_eq!(chrome_hit(panel, center(layout.icon)), None);
        // The gap between the two buttons is drag area, not a button.
        let gap = Point::new(
            layout.minimize.unwrap().right() + BUTTON_MARGIN / 2.0,
            center(layout.close.unwrap()).y,
        );
        assert_eq!(chrome_hit(panel, gap), None);
    }

    #[test]
    fn dot_is_centered_in_its_hit_target() {
        let hit = Rect::new(10.0, 20.0, BUTTON_HIT_SIZE, BUTTON_HIT_SIZE);
        let dot = button_dot(hit);
        assert_eq!(dot.size.width, BUTTON_DOT_DIAMETER);
        assert!((dot.origin.x - hit.origin.x - (hit.right() - dot.right())).abs() < 1e-9);
        assert!((dot.origin.y - hit.origin.y - (hit.bottom() - dot.bottom())).abs() < 1e-9);
    }

    #[test]
    fn minimized_chip_has_no_buttons_and_wider_middle() {
        let (mut scene, id) = scene_with_panel();
        scene.get_mut(id).unwrap().minimized = true;
        let panel = scene.get(id).unwrap();
        let layout = title_bar_layout(panel);
        assert_eq!(layout.bar.size, MINIMIZED_CHIP);
        assert_eq!(layout.close, None);
        assert_eq!(layout.minimize, None);
        assert_eq!(layout.middle.right(), layout.bar.right() - LEADING_PADDING);
        assert_eq!(
            chrome_hit(panel, Point::new(layout.bar.right() - 5.0, 64.0)),
            None
        );
    }

    #[test]
    fn narrow_panel_never_gets_negative_middle() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let layout = title_bar_layout(scene.get(id).unwrap());
        assert!(layout.middle.size.width >= 0.0);
    }

    #[test]
    fn hover_reports_panel_and_button() {
        let (scene, id) = scene_with_panel();
        let layout = title_bar_layout(scene.get(id).unwrap());
        let close = layout.close.unwrap();
        assert_eq!(
            hover_target(
                &scene,
                Point::new(close.origin.x + 1.0, close.origin.y + 1.0)
            ),
            Some((id, Some(ChromeButton::Close)))
        );
        assert_eq!(
            hover_target(&scene, Point::new(200.0, 200.0)),
            Some((id, None))
        );
        assert_eq!(hover_target(&scene, Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn details_hide_when_the_bar_gets_small_on_screen() {
        assert!(details_visible(1.0));
        assert!(details_visible(MIN_DETAIL_PX / TITLE_BAR_HEIGHT));
        assert!(!details_visible(0.4));
    }

    #[test]
    fn each_kind_has_its_own_icon_inside_the_unit_square() {
        let kinds = [
            terminal(),
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            PanelKind::Plugin {
                plugin_id: "hello".into(),
                plugin_version: None,
                settings: serde_json::Value::Null,
            },
        ];
        let icons: Vec<PanelIcon> = kinds.iter().map(PanelIcon::for_kind).collect();
        assert_eq!(
            icons,
            vec![PanelIcon::Terminal, PanelIcon::Browser, PanelIcon::Plugin]
        );
        let strokes = icons.into_iter().flat_map(icon_strokes).chain(
            [ChromeButton::Close, ChromeButton::Minimize]
                .into_iter()
                .flat_map(button_strokes),
        );
        for stroke in strokes {
            assert!(stroke.len() >= 2);
            for (x, y) in stroke {
                assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
            }
        }
    }
}
