//! Grid drawing plan: what the canvas paints for the grid each frame, as a short list of
//! primitives. Lines are quads. Dots and crosses come from a small cached texture drawn
//! tiled, so the per-frame draw count stays bounded however many marks are on screen.

use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::geometry::{Point, Rect, Size};
use crate::settings::{GridSettings, MIN_GRID_SPACING_PX, grid_line_offsets};

/// A tile is at least this many screen points on an edge, which caps the tile count.
pub const GRID_TILE_MIN_PX: f64 = 256.0;
/// Textures are rendered for cell sizes on a log scale with this many steps per doubling.
/// A cached tile is stretched by at most 2^(1/12), about 6 %, before a new one is needed.
pub const GRID_BUCKETS_PER_OCTAVE: f64 = 12.0;
/// The minor grid fades in between `MIN_GRID_SPACING_PX` and this screen spacing.
pub const MINOR_FADE_FULL_PX: f64 = 16.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridKind {
    #[default]
    Lines,
    Dots,
    /// A small plus at each intersection.
    Crosses,
    None,
}

impl GridKind {
    pub const ALL: [GridKind; 4] = [
        GridKind::Lines,
        GridKind::Dots,
        GridKind::Crosses,
        GridKind::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GridKind::Lines => "Lines",
            GridKind::Dots => "Dots",
            GridKind::Crosses => "Crosses",
            GridKind::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkShape {
    Dot,
    Cross,
}

/// Everything a grid texture is rendered from. Equal keys give identical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub shape: MarkShape,
    /// `0xRRGGBBAA`, straight alpha.
    pub rgba: u32,
    /// Mark size in device pixels.
    pub mark_px: u32,
    /// Marks per tile edge. 1 means the texture holds one mark and is drawn per intersection.
    pub cells: u32,
    /// Texture edge in device pixels.
    pub texture_px: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridPrimitive {
    /// A filled rect, `0xRRGGBBAA`.
    Quad { rect: Rect, rgba: u32 },
    /// The texture for `key`, stretched over `rect`.
    Image { rect: Rect, key: TileKey },
}

/// The most primitives a dot or cross grid draws on a `viewport` canvas: two layers (minor
/// and major) of tiles at least `GRID_TILE_MIN_PX` on an edge. It depends on the canvas size
/// only, never on how many marks are on screen.
pub fn mark_primitive_bound(viewport: Size) -> usize {
    let along = |extent: f64| (extent / GRID_TILE_MIN_PX).ceil().max(0.0) as usize + 1;
    2 * along(viewport.width) * along(viewport.height)
}

/// Fraction of the minor grid's opacity shown at `cell_px` screen spacing.
pub fn minor_fade(cell_px: f64) -> f64 {
    ((cell_px - MIN_GRID_SPACING_PX) / (MINOR_FADE_FULL_PX - MIN_GRID_SPACING_PX)).clamp(0.0, 1.0)
}

/// `cell_px` rounded down to its log bucket, so small zoom changes reuse one texture.
pub fn bucket_cell_px(cell_px: f64) -> f64 {
    let step = (cell_px.log2() * GRID_BUCKETS_PER_OCTAVE).floor();
    (step / GRID_BUCKETS_PER_OCTAVE).exp2()
}

/// `0xRRGGBBAA` from `0xRRGGBB` and an opacity in `0..=1`.
pub fn rgba_with_alpha(rgb: u32, alpha: f64) -> u32 {
    let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    (rgb & 0xff_ffff) << 8 | alpha
}

/// The grid primitives for one frame, in canvas-local screen points. `scale` is the
/// display's device pixels per point.
pub fn plan_grid(
    grid: &GridSettings,
    camera: &Camera,
    viewport: Size,
    scale: f64,
) -> Vec<GridPrimitive> {
    let mut out = Vec::new();
    if !grid.show
        || grid.kind == GridKind::None
        || !(viewport.width > 0.0 && viewport.height > 0.0)
        || !(scale.is_finite() && scale > 0.0)
    {
        return out;
    }
    let zero = camera.world_to_screen(Point::new(0.0, 0.0));
    let major = (grid.major_every >= 2).then(|| grid.spacing * f64::from(grid.major_every));
    let color = grid.color.0;
    match grid.kind {
        GridKind::Lines => {
            let minor_cell = grid.spacing * camera.zoom;
            let minor_alpha = grid.opacity * minor_fade(minor_cell);
            let skip_every = grid.major_every.max(1);
            if minor_alpha > 0.0 {
                let rgba = rgba_with_alpha(color, minor_alpha);
                push_lines(
                    &mut out,
                    zero,
                    grid.spacing,
                    camera.zoom,
                    viewport,
                    rgba,
                    skip_every,
                );
            }
            if let Some(major_step) = major {
                let rgba = rgba_with_alpha(color, grid.major_opacity);
                push_lines(&mut out, zero, major_step, camera.zoom, viewport, rgba, 1);
            }
        }
        GridKind::Dots | GridKind::Crosses => {
            let shape = if grid.kind == GridKind::Dots {
                MarkShape::Dot
            } else {
                MarkShape::Cross
            };
            let layer = MarkLayer {
                shape,
                mark_size: grid.mark_size,
                zero,
                zoom: camera.zoom,
                viewport,
                scale,
            };
            let minor_cell = grid.spacing * camera.zoom;
            if minor_cell.is_finite() && minor_cell >= MIN_GRID_SPACING_PX {
                // Fade from the bucket, not the exact spacing, so the texture key holds still.
                let alpha = grid.opacity * minor_fade(bucket_cell_px(minor_cell));
                if alpha > 0.0 {
                    layer.push(&mut out, grid.spacing, rgba_with_alpha(color, alpha));
                }
            }
            if let Some(major_step) = major {
                layer.push(
                    &mut out,
                    major_step,
                    rgba_with_alpha(color, grid.major_opacity),
                );
            }
        }
        GridKind::None => {}
    }
    out
}

/// One quad per line. Lines whose world index is a multiple of `skip_every` (above 1) are
/// left for the major layer.
fn push_lines(
    out: &mut Vec<GridPrimitive>,
    zero: Point,
    step: f64,
    zoom: f64,
    viewport: Size,
    rgba: u32,
    skip_every: u32,
) {
    let spacing = step * zoom;
    let keep = |offset: f64, zero: f64| {
        skip_every <= 1
            || (((offset - zero) / spacing).round() as i64).rem_euclid(i64::from(skip_every)) != 0
    };
    for x in grid_line_offsets(zero.x, step, zoom, viewport.width) {
        if keep(x, zero.x) {
            let rect = Rect::new(x, 0.0, 1.0, viewport.height);
            out.push(GridPrimitive::Quad { rect, rgba });
        }
    }
    for y in grid_line_offsets(zero.y, step, zoom, viewport.height) {
        if keep(y, zero.y) {
            let rect = Rect::new(0.0, y, viewport.width, 1.0);
            out.push(GridPrimitive::Quad { rect, rgba });
        }
    }
}

struct MarkLayer {
    shape: MarkShape,
    mark_size: f64,
    zero: Point,
    zoom: f64,
    viewport: Size,
    scale: f64,
}

impl MarkLayer {
    /// Marks every `step` world units. Small cells are drawn as tiles of many marks, large
    /// cells as one small texture per intersection; both keep the count near
    /// `(viewport / GRID_TILE_MIN_PX + 1)^2`.
    fn push(&self, out: &mut Vec<GridPrimitive>, step: f64, rgba: u32) {
        let cell = step * self.zoom;
        if !cell.is_finite() || cell < MIN_GRID_SPACING_PX {
            return;
        }
        let mark_px = (self.mark_size * self.scale).round().max(1.0) as u32;
        if cell >= GRID_TILE_MIN_PX {
            let texture_px = mark_extent(self.shape, mark_px).ceil() as u32 + 2;
            let key = TileKey {
                shape: self.shape,
                rgba,
                mark_px,
                cells: 1,
                texture_px,
            };
            let edge = f64::from(texture_px) / self.scale;
            // Same tiling walk with tiles one cell wide, centered on each intersection.
            for x in tile_starts(self.zero.x, cell, cell, self.viewport.width) {
                for y in tile_starts(self.zero.y, cell, cell, self.viewport.height) {
                    let center = Point::new(x + cell / 2.0, y + cell / 2.0);
                    let rect = Rect::new(center.x - edge / 2.0, center.y - edge / 2.0, edge, edge);
                    out.push(GridPrimitive::Image { rect, key });
                }
            }
            return;
        }
        let bucket = bucket_cell_px(cell);
        let cells = (GRID_TILE_MIN_PX / bucket).ceil() as u32;
        let tile = f64::from(cells) * cell;
        let key = TileKey {
            shape: self.shape,
            rgba,
            mark_px,
            cells,
            texture_px: (f64::from(cells) * bucket * self.scale).round() as u32,
        };
        for x in tile_starts(self.zero.x, cell, tile, self.viewport.width) {
            for y in tile_starts(self.zero.y, cell, tile, self.viewport.height) {
                let rect = Rect::new(x, y, tile, tile);
                out.push(GridPrimitive::Image { rect, key });
            }
        }
    }
}

/// Left edges of `tile`-wide tiles covering `[0, extent)`. Each tile starts half a cell
/// before an intersection that is a multiple of the tile, so the pattern stays put in world
/// space while panning. The first start lies in `(-tile, 0]`.
fn tile_starts(zero: f64, cell: f64, tile: f64, extent: f64) -> impl Iterator<Item = f64> {
    let rem = (zero - cell / 2.0).rem_euclid(tile);
    let first = if rem > 0.0 { rem - tile } else { rem };
    let count = if first.is_finite() && extent > 0.0 {
        ((extent - first) / tile).ceil().max(0.0) as usize
    } else {
        0
    };
    (0..count).map(move |index| first + index as f64 * tile)
}

/// The full width of a mark in device pixels.
fn mark_extent(shape: MarkShape, mark_px: u32) -> f64 {
    match shape {
        MarkShape::Dot => f64::from(mark_px),
        MarkShape::Cross => f64::from(4 * mark_px + 1),
    }
}

/// Coverage-antialiased RGBA pixels (straight alpha) for `key`, `texture_px` square. Each
/// cell holds one mark at its center, so tiles join without seams.
pub fn rasterize_tile(key: &TileKey) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let size = key.texture_px as usize;
    let mut pixels = vec![0u8; size * size * 4];
    if size == 0 || key.cells == 0 {
        return pixels;
    }
    let [r, g, b, a] = key.rgba.to_be_bytes();
    let cell = key.texture_px as f64 / f64::from(key.cells);
    let mark = mark_extent(key.shape, key.mark_px).min(cell - 1.0).max(1.0);
    let thickness = match key.shape {
        MarkShape::Dot => mark,
        MarkShape::Cross => (f64::from(key.mark_px) / 2.0).round().max(1.0),
    };
    let half = mark / 2.0;
    let inside = |dx: f64, dy: f64| match key.shape {
        MarkShape::Dot => dx * dx + dy * dy <= half * half,
        MarkShape::Cross => {
            let (dx, dy) = (dx.abs(), dy.abs());
            let arm = thickness / 2.0;
            (dx <= arm && dy <= half) || (dy <= arm && dx <= half)
        }
    };
    for row in 0..key.cells {
        for column in 0..key.cells {
            let center_x = (f64::from(column) + 0.5) * cell;
            let center_y = (f64::from(row) + 0.5) * cell;
            let x0 = (center_x - half - 1.0).floor().max(0.0) as usize;
            let x1 = ((center_x + half + 1.0).ceil() as usize).min(size);
            let y0 = (center_y - half - 1.0).floor().max(0.0) as usize;
            let y1 = ((center_y + half + 1.0).ceil() as usize).min(size);
            for y in y0..y1 {
                for x in x0..x1 {
                    let mut hits = 0;
                    for sy in 0..SAMPLES {
                        for sx in 0..SAMPLES {
                            let px = x as f64 + (f64::from(sx) + 0.5) / f64::from(SAMPLES);
                            let py = y as f64 + (f64::from(sy) + 0.5) / f64::from(SAMPLES);
                            if inside(px - center_x, py - center_y) {
                                hits += 1;
                            }
                        }
                    }
                    if hits > 0 {
                        let coverage = f64::from(hits) / f64::from(SAMPLES * SAMPLES);
                        let index = (y * size + x) * 4;
                        pixels[index..index + 4].copy_from_slice(&[
                            r,
                            g,
                            b,
                            (f64::from(a) * coverage).round() as u8,
                        ]);
                    }
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Size = Size {
        width: 1280.0,
        height: 800.0,
    };
    /// The draw count the grid may never exceed on a 1280x800 canvas, for any mark kind.
    const MAX_MARK_PRIMITIVES: usize = 64;

    fn grid(kind: GridKind, spacing: f64, major_every: u32) -> GridSettings {
        GridSettings {
            show: true,
            kind,
            spacing,
            major_every,
            ..GridSettings::default()
        }
    }

    fn camera(zoom: f64, offset: Point) -> Camera {
        Camera { offset, zoom }
    }

    /// Marks a grid of this kind puts on screen: intersections inside the viewport.
    fn visible_marks(grid: &GridSettings, camera: &Camera) -> usize {
        let zero = camera.world_to_screen(Point::new(0.0, 0.0));
        let xs = grid_line_offsets(zero.x, grid.spacing, camera.zoom, VIEWPORT.width).len();
        let ys = grid_line_offsets(zero.y, grid.spacing, camera.zoom, VIEWPORT.height).len();
        xs * ys
    }

    #[test]
    fn mark_grids_draw_a_bounded_count_independent_of_the_mark_count() {
        let offsets = [Point::new(0.0, 0.0), Point::new(-37.5, 913.25)];
        let mut most_marks = 0;
        for kind in [GridKind::Dots, GridKind::Crosses] {
            for spacing in [16.0, 64.0, 256.0] {
                for major_every in [0, 4, 8] {
                    for zoom in [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0, 8.0] {
                        for offset in offsets {
                            let grid = grid(kind, spacing, major_every);
                            let camera = camera(zoom, offset);
                            let plan = plan_grid(&grid, &camera, VIEWPORT, 2.0);
                            most_marks = most_marks.max(visible_marks(&grid, &camera));
                            assert!(
                                plan.len() <= mark_primitive_bound(VIEWPORT),
                                "{kind:?} spacing {spacing} major {major_every} zoom {zoom}: \
                                 {} primitives for {} marks",
                                plan.len(),
                                visible_marks(&grid, &camera)
                            );
                        }
                    }
                }
            }
        }
        // The sweep includes grids with thousands of marks, so the bound is not trivial.
        assert!(most_marks > 5000, "{most_marks}");
        assert_eq!(mark_primitive_bound(VIEWPORT), 60);
        assert!(mark_primitive_bound(VIEWPORT) < MAX_MARK_PRIMITIVES);
    }

    #[test]
    fn report_grid_primitive_counts() {
        // Prints the table quoted in the pull request. Run with --nocapture.
        for (kind, spacing) in GridKind::ALL
            .into_iter()
            .flat_map(|k| [(k, 16.0), (k, 64.0)])
        {
            for major_every in [0, 8] {
                let counts: Vec<String> = [0.25, 1.0, 4.0]
                    .iter()
                    .map(|&zoom| {
                        let grid = grid(kind, spacing, major_every);
                        let camera = camera(zoom, Point::new(0.0, 0.0));
                        let plan = plan_grid(&grid, &camera, VIEWPORT, 2.0);
                        let marks = if matches!(kind, GridKind::Dots | GridKind::Crosses) {
                            visible_marks(&grid, &camera)
                        } else {
                            0
                        };
                        format!("zoom {zoom}: {} primitives ({marks} marks)", plan.len())
                    })
                    .collect();
                println!(
                    "grid_primitives kind={} spacing={spacing} major={major_every} {}",
                    kind.label(),
                    counts.join(", ")
                );
            }
        }
    }

    #[test]
    fn report_largest_tile_raster_time() {
        // Prints the cost of the biggest texture a plan can ask for. Run with --nocapture.
        let key = TileKey {
            shape: MarkShape::Cross,
            rgba: 0xffffff1f,
            mark_px: 8,
            cells: 2,
            texture_px: 1020,
        };
        let started = std::time::Instant::now();
        let pixels = rasterize_tile(&key);
        println!(
            "grid_raster texture_px=1020 cells=2 micros={}",
            started.elapsed().as_micros()
        );
        assert_eq!(pixels.len(), 1020 * 1020 * 4);
    }

    #[test]
    fn lines_draw_one_quad_per_line_and_leave_majors_to_the_major_layer() {
        let camera = camera(1.0, Point::new(0.0, 0.0));
        let plain = plan_grid(&grid(GridKind::Lines, 64.0, 0), &camera, VIEWPORT, 2.0);
        // 1280 / 64 = 20 vertical lines, 800 / 64 = 12.5, so 13 horizontal.
        assert_eq!(plain.len(), 33);
        let with_major = plan_grid(&grid(GridKind::Lines, 64.0, 4), &camera, VIEWPORT, 2.0);
        assert_eq!(
            with_major.len(),
            33,
            "majors replace minors, never double up"
        );
        let strong = rgba_with_alpha(0xffffff, GridSettings::default().major_opacity);
        let majors: Vec<_> = with_major
            .iter()
            .filter(|p| matches!(p, GridPrimitive::Quad { rgba, .. } if *rgba == strong))
            .collect();
        // Every 4th line: x 0, 256, ..., 1024 (5) and y 0, 256, 512, 768 (4).
        assert_eq!(majors.len(), 9);
    }

    #[test]
    fn minor_grid_fades_then_skips_while_major_stays() {
        assert_eq!(minor_fade(8.0), 0.0);
        assert_eq!(minor_fade(12.0), 0.5);
        assert_eq!(minor_fade(16.0), 1.0);
        assert_eq!(minor_fade(400.0), 1.0);
        // Spacing 16 at zoom 0.25 is 4 px: minor gone, major (every 8, 32 px) still drawn.
        let camera = camera(0.25, Point::new(0.0, 0.0));
        for kind in [GridKind::Lines, GridKind::Dots] {
            assert!(plan_grid(&grid(kind, 16.0, 0), &camera, VIEWPORT, 2.0).is_empty());
            assert!(!plan_grid(&grid(kind, 16.0, 8), &camera, VIEWPORT, 2.0).is_empty());
        }
    }

    #[test]
    fn hidden_none_and_degenerate_inputs_draw_nothing() {
        let camera = camera(1.0, Point::new(0.0, 0.0));
        let mut hidden = grid(GridKind::Dots, 64.0, 0);
        hidden.show = false;
        assert!(plan_grid(&hidden, &camera, VIEWPORT, 2.0).is_empty());
        assert!(plan_grid(&grid(GridKind::None, 64.0, 8), &camera, VIEWPORT, 2.0).is_empty());
        let dots = grid(GridKind::Dots, 64.0, 0);
        assert!(plan_grid(&dots, &camera, Size::new(0.0, 800.0), 2.0).is_empty());
        assert!(plan_grid(&dots, &camera, VIEWPORT, f64::NAN).is_empty());
        assert!(
            plan_grid(
                &dots,
                &self::camera(f64::NAN, Point::new(0.0, 0.0)),
                VIEWPORT,
                2.0
            )
            .is_empty()
        );
    }

    #[test]
    fn tiles_cover_the_viewport_and_stay_fixed_in_world_space() {
        let grid = grid(GridKind::Dots, 16.0, 0);
        let plan = plan_grid(&grid, &camera(1.0, Point::new(0.0, 0.0)), VIEWPORT, 2.0);
        let rects: Vec<Rect> = plan
            .iter()
            .map(|p| match p {
                GridPrimitive::Image { rect, .. } => *rect,
                GridPrimitive::Quad { .. } => panic!("dots drew a quad"),
            })
            .collect();
        let left = rects.iter().map(|r| r.origin.x).fold(f64::MAX, f64::min);
        let top = rects.iter().map(|r| r.origin.y).fold(f64::MAX, f64::min);
        let right = rects.iter().map(|r| r.right()).fold(f64::MIN, f64::max);
        let bottom = rects.iter().map(|r| r.bottom()).fold(f64::MIN, f64::max);
        assert!(left <= 0.0 && top <= 0.0 && right >= 1280.0 && bottom >= 800.0);
        // A tile starts half a cell before an intersection: world 0 is at screen 0, so tile
        // edges sit at -8 + n * 256.
        assert!(
            rects
                .iter()
                .all(|r| ((r.origin.x + 8.0) / 256.0).fract().abs() < 1e-9)
        );
        // Panning by a whole tile gives the same rects shifted, and the same texture.
        let panned = plan_grid(&grid, &camera(1.0, Point::new(256.0, 0.0)), VIEWPORT, 2.0);
        assert_eq!(panned.len(), plan.len());
        assert_eq!(panned[0], plan[0]);
    }

    #[test]
    fn small_zoom_changes_reuse_one_texture() {
        let grid = grid(GridKind::Crosses, 64.0, 0);
        let key =
            |zoom: f64| match plan_grid(&grid, &camera(zoom, Point::new(0.0, 0.0)), VIEWPORT, 2.0)
                [0]
            {
                GridPrimitive::Image { key, .. } => key,
                GridPrimitive::Quad { .. } => panic!("crosses drew a quad"),
            };
        assert_eq!(key(1.0), key(1.02));
        assert_ne!(key(1.0), key(1.2));
        // Textures stay small: at most two tile minimums on an edge at 2x.
        for zoom in [0.15, 0.3, 0.7, 1.0, 2.0, 3.9] {
            assert!(
                key(zoom).texture_px <= (4.0 * GRID_TILE_MIN_PX) as u32,
                "{zoom}"
            );
        }
    }

    #[test]
    fn rasterized_marks_sit_at_cell_centers_in_the_grid_color() {
        let key = TileKey {
            shape: MarkShape::Dot,
            rgba: 0x336699ff,
            mark_px: 4,
            cells: 2,
            texture_px: 64,
        };
        let pixels = rasterize_tile(&key);
        assert_eq!(pixels.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        // Centers at 16 and 48: the pixels around them are fully covered.
        for (x, y) in [(15, 15), (16, 16), (47, 15), (48, 48)] {
            assert_eq!(at(x, y), [0x33, 0x66, 0x99, 0xff], "({x}, {y})");
        }
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(32, 32)[3], 0);
        let covered = pixels.chunks(4).filter(|p| p[3] > 0).count();
        // Four dots of diameter 4: about 4 * pi * 4 = 50 px with antialiased edges.
        assert!((40..=80).contains(&covered), "{covered}");

        let cross = TileKey {
            shape: MarkShape::Cross,
            ..key
        };
        let pixels = rasterize_tile(&cross);
        let at = |x: usize, y: usize| pixels[(y * 64 + x) * 4 + 3];
        // Arms reach 8 px from the center of the 17 px plus; diagonals stay empty.
        assert_eq!(at(16 + 7, 16), 0xff);
        assert_eq!(at(16, 16 - 8), 0xff);
        assert_eq!(at(16 + 5, 16 + 5), 0);
    }
}
