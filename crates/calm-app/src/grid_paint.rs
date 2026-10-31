//! Paints the grid plan from `calm_core::grid`. Mark textures are rasterized on the UI
//! thread when a plan needs one that is not cached; they are small and change only when the
//! style, color or zoom bucket changes.

use std::sync::Arc;

use calm_core::camera::Camera;
use calm_core::geometry::{Rect, Size};
use calm_core::grid::{GridPrimitive, TileKey, plan_grid, rasterize_tile};
use calm_core::settings::GridSettings;
use gpui::{Bounds, Pixels, RenderImage, Window, fill, point, px, rgba, size};
use image::RgbaImage;

use crate::background::to_render_image;

/// Enough for the minor and major layers at a few neighboring zoom buckets.
const CACHE_CAPACITY: usize = 8;

#[derive(Default)]
pub struct GridTextures {
    /// Least recently used first.
    cache: Vec<(TileKey, Arc<RenderImage>)>,
    retired: Vec<Arc<RenderImage>>,
    /// Textures rasterized so far, for the smoke.
    pub rasters: u64,
    /// Grid primitives in the latest frame and the most in any frame, for the smoke.
    pub last_primitives: usize,
    pub max_primitives: usize,
}

/// One frame of grid, ready to paint without touching the view.
#[derive(Clone, Default)]
pub struct GridFrame {
    primitives: Vec<GridPrimitive>,
    textures: Vec<(TileKey, Arc<RenderImage>)>,
}

impl GridTextures {
    /// Plans the grid for this frame and makes sure each texture it uses exists.
    pub fn prepare(
        &mut self,
        grid: &GridSettings,
        camera: &Camera,
        canvas: Size,
        window: &mut Window,
    ) -> GridFrame {
        for old in self.retired.drain(..) {
            window.drop_image(old).ok();
        }
        let scale = f64::from(window.scale_factor()).max(1.0);
        let primitives = plan_grid(grid, camera, canvas, scale);
        self.last_primitives = primitives.len();
        self.max_primitives = self.max_primitives.max(primitives.len());
        let mut textures: Vec<(TileKey, Arc<RenderImage>)> = Vec::new();
        for primitive in &primitives {
            let GridPrimitive::Image { key, .. } = primitive else {
                continue;
            };
            if textures.iter().any(|(known, _)| known == key) {
                continue;
            }
            textures.push((*key, self.texture(key)));
        }
        GridFrame {
            primitives,
            textures,
        }
    }

    fn texture(&mut self, key: &TileKey) -> Arc<RenderImage> {
        if let Some(index) = self.cache.iter().position(|(known, _)| known == key) {
            let entry = self.cache.remove(index);
            let image = entry.1.clone();
            self.cache.push(entry);
            return image;
        }
        let pixels = rasterize_tile(key);
        let image = match RgbaImage::from_raw(key.texture_px, key.texture_px, pixels) {
            Some(rgba) => to_render_image(rgba),
            // rasterize_tile always returns texture_px squared pixels; this is a bug.
            None => panic!("grid tile raster has the wrong size for {key:?}"),
        };
        self.rasters += 1;
        if self.cache.len() >= CACHE_CAPACITY {
            let (_, old) = self.cache.remove(0);
            self.retired.push(old);
        }
        self.cache.push((*key, image.clone()));
        image
    }
}

/// Paints `frame` inside `bounds`, one GPUI call per primitive. Call inside the canvas mask.
pub fn paint_grid(frame: &GridFrame, bounds: Bounds<Pixels>, window: &mut Window) {
    let to_bounds = |rect: &Rect| Bounds {
        origin: point(
            bounds.origin.x + px(rect.origin.x as f32),
            bounds.origin.y + px(rect.origin.y as f32),
        ),
        size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
    };
    for primitive in &frame.primitives {
        match primitive {
            GridPrimitive::Quad { rect, rgba: color } => {
                window.paint_quad(fill(to_bounds(rect), rgba(*color)));
            }
            GridPrimitive::Image { rect, key } => {
                let Some((_, image)) = frame.textures.iter().find(|(known, _)| known == key) else {
                    eprintln!("calm: grid texture missing for {key:?}");
                    continue;
                };
                if let Err(err) = window.paint_image(
                    bounds,
                    to_bounds(rect),
                    Default::default(),
                    image.clone(),
                    0,
                    false,
                ) {
                    eprintln!("calm: grid paint: {err:#}");
                    return;
                }
            }
        }
    }
}
