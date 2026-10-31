//! Canvas background: solid color, gradient or image, plus the pixel math behind them.
//!
//! Everything here is pure. The app rasterizes gradients and filters images off the UI
//! thread once per change and paints the cached result, so none of this runs per frame.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Size};

/// An sRGB color, `0xRRGGBB`. Stored in JSON as `"#rrggbb"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub u32);

impl Rgb {
    pub fn channels(self) -> [u8; 3] {
        [(self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8]
    }

    pub fn from_channels([r, g, b]: [u8; 3]) -> Rgb {
        Rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
    }

    /// Channels in `0.0..=1.0`, as AppKit color wells use them.
    pub fn from_unit(r: f64, g: f64, b: f64) -> Rgb {
        let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb::from_channels([channel(r), channel(g), channel(b)])
    }

    pub fn to_unit(self) -> [f64; 3] {
        self.channels().map(|c| f64::from(c) / 255.0)
    }

    /// `self` drawn at `opacity` over `under`.
    pub fn over(self, under: Rgb, opacity: f64) -> Rgb {
        let opacity = opacity.clamp(0.0, 1.0);
        let top = self.channels();
        let bottom = under.channels();
        let mix = |i: usize| {
            (f64::from(bottom[i]) + (f64::from(top[i]) - f64::from(bottom[i])) * opacity).round()
                as u8
        };
        Rgb::from_channels([mix(0), mix(1), mix(2)])
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:06x}", self.0 & 0xff_ffff)
    }
}

impl From<Rgb> for String {
    fn from(color: Rgb) -> String {
        color.to_string()
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(text: String) -> Result<Rgb, String> {
        let hex = text
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| format!("color {text:?} is not of the form \"#rrggbb\""))?;
        u32::from_str_radix(hex, 16)
            .map(Rgb)
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundKind {
    #[default]
    Solid,
    Gradient,
    Image,
}

impl BackgroundKind {
    pub const ALL: [BackgroundKind; 3] = [
        BackgroundKind::Solid,
        BackgroundKind::Gradient,
        BackgroundKind::Image,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BackgroundKind::Solid => "Solid Color",
            BackgroundKind::Gradient => "Gradient",
            BackgroundKind::Image => "Image",
        }
    }
}

/// All three kinds are kept, so switching kind in Settings does not lose the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundSettings {
    pub kind: BackgroundKind,
    pub solid: Rgb,
    pub gradient: Gradient,
    pub image: ImageBackground,
}

pub const DEFAULT_BACKGROUND: Rgb = Rgb(0x151515);

impl Default for BackgroundSettings {
    fn default() -> Self {
        BackgroundSettings {
            kind: BackgroundKind::Solid,
            solid: DEFAULT_BACKGROUND,
            gradient: Gradient::default(),
            image: ImageBackground::default(),
        }
    }
}

impl BackgroundSettings {
    /// One flat color that stands for the background: drawn under images and while a
    /// gradient or image is still being prepared.
    pub fn base_color(&self) -> Rgb {
        match self.kind {
            BackgroundKind::Gradient => self.gradient.stops[0].color,
            BackgroundKind::Solid | BackgroundKind::Image => self.solid,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: Rgb,
    /// `0.0..=1.0` along the gradient.
    pub position: f64,
}

pub const MIN_GRADIENT_STOPS: usize = 2;
pub const MAX_GRADIENT_STOPS: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Linear only. CSS convention: 0 points up, 90 points right, clockwise.
    pub angle_deg: f64,
    pub stops: Vec<GradientStop>,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient {
            kind: GradientKind::Linear,
            angle_deg: 135.0,
            stops: vec![
                GradientStop {
                    color: Rgb(0x101827),
                    position: 0.0,
                },
                GradientStop {
                    color: Rgb(0x2b2b2e),
                    position: 1.0,
                },
            ],
        }
    }
}

impl Gradient {
    /// Color at `t` in `0..=1`. Stops are used in position order, whatever their list order.
    pub fn color_at(&self, t: f64) -> Rgb {
        let mut stops = self.stops.clone();
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        color_at_sorted(&stops, t)
    }

    /// RGBA8 pixels of the gradient filling a `width` x `height` raster.
    pub fn rasterize(&self, width: u32, height: u32) -> Vec<u8> {
        let mut stops = self.stops.clone();
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        let (w, h) = (f64::from(width), f64::from(height));
        let (dx, dy) = {
            let radians = self.angle_deg.to_radians();
            (radians.sin(), -radians.cos())
        };
        // CSS gradient line length: the corners land exactly on t = 0 and t = 1.
        let half_length = (w * dx.abs() + h * dy.abs()) / 2.0;
        let max_radius = (w * w + h * h).sqrt() / 2.0;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                let px = f64::from(x) + 0.5 - w / 2.0;
                let py = f64::from(y) + 0.5 - h / 2.0;
                let t = match self.kind {
                    GradientKind::Linear if half_length > 0.0 => {
                        (px * dx + py * dy) / (2.0 * half_length) + 0.5
                    }
                    GradientKind::Linear => 0.0,
                    GradientKind::Radial if max_radius > 0.0 => {
                        (px * px + py * py).sqrt() / max_radius
                    }
                    GradientKind::Radial => 0.0,
                };
                let [r, g, b] = color_at_sorted(&stops, t).channels();
                pixels.extend_from_slice(&[r, g, b, 0xff]);
            }
        }
        pixels
    }
}

fn color_at_sorted(stops: &[GradientStop], t: f64) -> Rgb {
    let Some(first) = stops.first() else {
        return DEFAULT_BACKGROUND;
    };
    let t = if t.is_finite() { t } else { 0.0 };
    if t <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.position {
            let span = b.position - a.position;
            let local = if span > 0.0 {
                (t - a.position) / span
            } else {
                1.0
            };
            return b.color.over(a.color, local);
        }
    }
    stops[stops.len() - 1].color
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Cover,
    Contain,
    Tile,
    Center,
}

impl ImageFit {
    pub const ALL: [ImageFit; 4] = [
        ImageFit::Cover,
        ImageFit::Contain,
        ImageFit::Tile,
        ImageFit::Center,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ImageFit::Cover => "Cover",
            ImageFit::Contain => "Contain",
            ImageFit::Tile => "Tile",
            ImageFit::Center => "Center",
        }
    }
}

/// The image is read from `path` where it is; CALM does not copy it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageBackground {
    pub path: Option<PathBuf>,
    pub fit: ImageFit,
    pub filters: ImageFilters,
}

impl Default for ImageBackground {
    fn default() -> Self {
        ImageBackground {
            path: None,
            fit: ImageFit::Cover,
            filters: ImageFilters::default(),
        }
    }
}

/// CSS filter semantics. Identity is blur 0, brightness, contrast and saturation 1, no
/// grayscale, dim 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageFilters {
    /// Blur radius in image pixels, `0..=MAX_BLUR`.
    pub blur: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub grayscale: bool,
    /// Black overlay opacity, `0..=1`. Keeps panels readable over a busy image.
    pub dim: f64,
}

pub const MAX_BLUR: f64 = 40.0;
/// Upper bound for brightness, contrast and saturation.
pub const MAX_FILTER_FACTOR: f64 = 2.0;

impl Default for ImageFilters {
    fn default() -> Self {
        ImageFilters {
            blur: 0.0,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            grayscale: false,
            dim: 0.0,
        }
    }
}

impl ImageFilters {
    /// True when `apply_color` would leave every pixel unchanged.
    pub fn color_is_identity(&self) -> bool {
        let defaults = ImageFilters::default();
        self.brightness == defaults.brightness
            && self.contrast == defaults.contrast
            && self.saturation == defaults.saturation
            && !self.grayscale
            && self.dim == defaults.dim
    }

    /// Applies brightness, contrast, saturation, grayscale and dim to RGBA8 pixels in place,
    /// in that order. Alpha is kept.
    pub fn apply_color(&self, rgba: &mut [u8]) {
        if self.color_is_identity() {
            return;
        }
        let saturation = if self.grayscale { 0.0 } else { self.saturation };
        let keep = 1.0 - self.dim.clamp(0.0, 1.0);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let mut rgb = [pixel[0], pixel[1], pixel[2]].map(|c| f64::from(c) / 255.0);
            for c in &mut rgb {
                *c = ((*c * self.brightness - 0.5) * self.contrast + 0.5).clamp(0.0, 1.0);
            }
            // Rec. 709 luma, as CSS saturate() uses.
            let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
            for (index, c) in rgb.into_iter().enumerate() {
                let saturated = (luma + (c - luma) * saturation).clamp(0.0, 1.0);
                pixel[index] = (saturated * keep * 255.0).round() as u8;
            }
        }
    }
}

/// Where an image of `image` points lands on `canvas`. Several rects for `Tile`.
/// Rects may reach past `canvas`; the caller clips.
pub fn image_dest_rects(fit: ImageFit, image: Size, canvas: Rect) -> Vec<Rect> {
    if !(image.width > 0.0 && image.height > 0.0 && canvas.size.width > 0.0) {
        return Vec::new();
    }
    let centered = |size: Size| {
        Rect::new(
            canvas.origin.x + (canvas.size.width - size.width) / 2.0,
            canvas.origin.y + (canvas.size.height - size.height) / 2.0,
            size.width,
            size.height,
        )
    };
    let scaled = |scale: f64| Size::new(image.width * scale, image.height * scale);
    let scale_x = canvas.size.width / image.width;
    let scale_y = canvas.size.height / image.height;
    match fit {
        ImageFit::Cover => vec![centered(scaled(scale_x.max(scale_y)))],
        ImageFit::Contain => vec![centered(scaled(scale_x.min(scale_y)))],
        ImageFit::Center => vec![centered(image)],
        ImageFit::Tile => {
            let columns = (canvas.size.width / image.width).ceil() as usize;
            let rows = (canvas.size.height / image.height).ceil() as usize;
            let mut rects = Vec::with_capacity(columns * rows);
            for row in 0..rows {
                for column in 0..columns {
                    rects.push(Rect::new(
                        canvas.origin.x + column as f64 * image.width,
                        canvas.origin.y + row as f64 * image.height,
                        image.width,
                        image.height,
                    ));
                }
            }
            rects
        }
    }
}

/// Tiles smaller than this are repeated into one bigger tile before upload, which keeps
/// the per-frame quad count low on a large window.
pub const MIN_TILE_PX: u32 = 256;

/// How many copies along each axis make a tile at least `MIN_TILE_PX` on both sides.
pub fn tile_repeat(width: u32, height: u32) -> (u32, u32) {
    let repeat = |extent: u32| MIN_TILE_PX.div_ceil(extent.max(1)).max(1);
    (repeat(width), repeat(height))
}

/// Gradients are smooth, so a small raster scaled up by the GPU looks the same and keeps
/// a change cheap. Returns the raster size for a canvas of `width` x `height` points.
pub const GRADIENT_RASTER_MAX: f64 = 512.0;

pub fn gradient_raster_size(width: f64, height: f64) -> (u32, u32) {
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return (1, 1);
    }
    let scale = (GRADIENT_RASTER_MAX / width.max(height)).min(1.0);
    let side = |v: f64| ((v * scale).round() as u32).max(1);
    (side(width), side(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(color: u32, position: f64) -> GradientStop {
        GradientStop {
            color: Rgb(color),
            position,
        }
    }

    #[test]
    fn rgb_json_is_hex_and_rejects_garbage() {
        assert_eq!(
            serde_json::to_string(&Rgb(0x0a0b0c)).unwrap(),
            "\"#0a0b0c\""
        );
        let parsed: Rgb = serde_json::from_str("\"#FFa000\"").unwrap();
        assert_eq!(parsed, Rgb(0xffa000));
        for bad in [
            "\"ffa000\"",
            "\"#ffa00\"",
            "\"#ggaa00\"",
            "12",
            "\"#ffa0000\"",
        ] {
            assert!(serde_json::from_str::<Rgb>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rgb_unit_roundtrip_and_blend() {
        assert_eq!(Rgb::from_unit(1.0, 0.5, 0.0), Rgb(0xff8000));
        assert_eq!(Rgb::from_unit(2.0, -1.0, f64::NAN), Rgb(0xff0000));
        assert_eq!(Rgb(0xff8000).to_unit(), [1.0, 128.0 / 255.0, 0.0]);
        // White at 12 % over the old default gives the old 12 % lighter grid line.
        assert_eq!(Rgb(0xffffff).over(Rgb(0x151515), 0.12), Rgb(0x313131));
        assert_eq!(Rgb(0xffffff).over(Rgb(0x000000), 0.0), Rgb(0x000000));
    }

    #[test]
    fn gradient_color_interpolates_between_sorted_stops() {
        let gradient = Gradient {
            stops: vec![
                stop(0xffffff, 1.0),
                stop(0x000000, 0.0),
                stop(0xff0000, 0.5),
            ],
            ..Gradient::default()
        };
        assert_eq!(gradient.color_at(-1.0), Rgb(0x000000));
        assert_eq!(gradient.color_at(0.0), Rgb(0x000000));
        assert_eq!(gradient.color_at(0.25), Rgb(0x800000));
        assert_eq!(gradient.color_at(0.5), Rgb(0xff0000));
        assert_eq!(gradient.color_at(0.75), Rgb(0xff8080));
        assert_eq!(gradient.color_at(2.0), Rgb(0xffffff));
        assert_eq!(gradient.color_at(f64::NAN), Rgb(0x000000));
    }

    #[test]
    fn linear_gradient_follows_angle() {
        let gradient = Gradient {
            kind: GradientKind::Linear,
            angle_deg: 90.0,
            stops: vec![stop(0x000000, 0.0), stop(0xffffff, 1.0)],
        };
        let pixels = gradient.rasterize(4, 2);
        assert_eq!(pixels.len(), 4 * 2 * 4);
        let red = |x: usize, y: usize| pixels[(y * 4 + x) * 4];
        // 90 degrees runs left to right: dark on the left, light on the right, rows equal.
        assert!(red(0, 0) < 64 && red(3, 0) > 191, "{pixels:?}");
        assert_eq!(red(1, 0), red(1, 1));
        assert!(pixels.chunks(4).all(|p| p[3] == 0xff));
        // 180 degrees runs top to bottom.
        let down = Gradient {
            angle_deg: 180.0,
            ..gradient.clone()
        }
        .rasterize(2, 4);
        assert!(down[0] < 64 && down[(3 * 2) * 4] > 191);
        assert_eq!(down[0], down[4]);
    }

    #[test]
    fn radial_gradient_is_dark_in_the_middle() {
        let gradient = Gradient {
            kind: GradientKind::Radial,
            angle_deg: 0.0,
            stops: vec![stop(0x000000, 0.0), stop(0xffffff, 1.0)],
        };
        let pixels = gradient.rasterize(9, 9);
        let red = |x: usize, y: usize| pixels[(y * 9 + x) * 4];
        assert!(red(4, 4) < 20);
        assert!(red(0, 0) > 200);
        assert_eq!(red(0, 4), red(8, 4));
    }

    #[test]
    fn color_filters_follow_css_semantics() {
        let base = [200u8, 100, 50, 255];
        let run = |filters: ImageFilters| {
            let mut pixel = base;
            filters.apply_color(&mut pixel);
            pixel
        };
        assert_eq!(run(ImageFilters::default()), base);
        let darker = run(ImageFilters {
            brightness: 0.5,
            ..ImageFilters::default()
        });
        assert_eq!(darker, [100, 50, 25, 255]);
        let flat = run(ImageFilters {
            contrast: 0.0,
            ..ImageFilters::default()
        });
        assert_eq!(flat, [128, 128, 128, 255]);
        let gray = run(ImageFilters {
            grayscale: true,
            ..ImageFilters::default()
        });
        assert!(gray[0] == gray[1] && gray[1] == gray[2], "{gray:?}");
        assert_eq!(
            gray,
            run(ImageFilters {
                saturation: 0.0,
                ..ImageFilters::default()
            })
        );
        let dimmed = run(ImageFilters {
            dim: 0.5,
            ..ImageFilters::default()
        });
        assert_eq!(dimmed, [100, 50, 25, 255]);
        let black = run(ImageFilters {
            dim: 1.0,
            ..ImageFilters::default()
        });
        assert_eq!(black, [0, 0, 0, 255]);
        // Alpha is never touched.
        let mut translucent = [10u8, 20, 30, 77];
        ImageFilters {
            brightness: 2.0,
            ..ImageFilters::default()
        }
        .apply_color(&mut translucent);
        assert_eq!(translucent, [20, 40, 60, 77]);
    }

    #[test]
    fn dest_rects_for_each_fit() {
        let canvas = Rect::new(10.0, 20.0, 400.0, 200.0);
        let image = Size::new(100.0, 100.0);
        assert_eq!(
            image_dest_rects(ImageFit::Cover, image, canvas),
            vec![Rect::new(10.0, -80.0, 400.0, 400.0)]
        );
        assert_eq!(
            image_dest_rects(ImageFit::Contain, image, canvas),
            vec![Rect::new(110.0, 20.0, 200.0, 200.0)]
        );
        assert_eq!(
            image_dest_rects(ImageFit::Center, image, canvas),
            vec![Rect::new(160.0, 70.0, 100.0, 100.0)]
        );
        let tiles = image_dest_rects(ImageFit::Tile, Size::new(150.0, 150.0), canvas);
        assert_eq!(tiles.len(), 3 * 2);
        assert_eq!(tiles[0], Rect::new(10.0, 20.0, 150.0, 150.0));
        assert_eq!(tiles[5], Rect::new(310.0, 170.0, 150.0, 150.0));
        assert!(image_dest_rects(ImageFit::Tile, Size::new(0.0, 5.0), canvas).is_empty());
    }

    #[test]
    fn tiles_and_rasters_stay_bounded() {
        assert_eq!(tile_repeat(256, 1000), (1, 1));
        assert_eq!(tile_repeat(10, 100), (26, 3));
        assert_eq!(tile_repeat(0, 0), (256, 256));
        assert_eq!(gradient_raster_size(2560.0, 1600.0), (512, 320));
        assert_eq!(gradient_raster_size(300.0, 200.0), (300, 200));
        assert_eq!(gradient_raster_size(0.0, 100.0), (1, 1));
        assert_eq!(gradient_raster_size(f64::INFINITY, 100.0), (1, 1));
    }
}
