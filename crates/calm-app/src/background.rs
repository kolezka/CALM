//! Gradient and image backgrounds. Each change is rendered once off the UI thread into a
//! texture; paint only draws that texture. Blur in particular never runs per frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use calm_core::background::{
    BackgroundKind, BackgroundSettings, Gradient, ImageBackground, ImageFit, gradient_raster_size,
    image_dest_rects, tile_repeat,
};
use calm_core::geometry::{Rect, Size};
use gpui::{Bounds, Context, Pixels, RenderImage, Task, Window, point, px, size};
use image::{Frame, RgbaImage};

use crate::canvas_view::CanvasView;

/// Images are scaled down to this long edge before filtering. Keeps blur and upload cheap.
const MAX_IMAGE_EDGE: u32 = 2048;

/// What a texture was rendered from. A new one is rendered when this changes.
#[derive(Clone, Debug, PartialEq)]
enum RenderKey {
    Gradient {
        gradient: Gradient,
        raster: (u32, u32),
    },
    Image(ImageBackground),
}

impl RenderKey {
    fn wanted(background: &BackgroundSettings, canvas: Size) -> Option<RenderKey> {
        match background.kind {
            BackgroundKind::Solid => None,
            BackgroundKind::Gradient => Some(RenderKey::Gradient {
                gradient: background.gradient.clone(),
                raster: gradient_raster_size(canvas.width, canvas.height),
            }),
            BackgroundKind::Image => background
                .image
                .path
                .is_some()
                .then(|| RenderKey::Image(background.image.clone())),
        }
    }

    fn same_kind(&self, other: &RenderKey) -> bool {
        matches!(
            (self, other),
            (RenderKey::Gradient { .. }, RenderKey::Gradient { .. })
                | (RenderKey::Image(_), RenderKey::Image(_))
        )
    }
}

/// A rendered background, ready to paint.
#[derive(Clone)]
pub struct BackgroundTexture {
    pub image: Arc<RenderImage>,
    /// Pixel size of the texture.
    pub pixels: (u32, u32),
    /// `None` stretches over the canvas (gradients); `Some` places it by fit (images).
    pub fit: Option<ImageFit>,
}

struct Rendered {
    key: RenderKey,
    texture: BackgroundTexture,
}

#[derive(Default)]
pub struct BackgroundRenderer {
    current: Option<Rendered>,
    running: Option<(RenderKey, Task<()>)>,
    /// A key that failed, so it is not retried every frame. Cleared by any change.
    failed: Option<RenderKey>,
    /// The last decoded image, so a filter change does not decode again.
    source: Option<(PathBuf, Arc<RgbaImage>)>,
    /// Old textures to free from the GPU atlas on the next render.
    retired: Vec<Arc<RenderImage>>,
    pub error: Option<String>,
    /// Finished renders, for the smoke.
    pub renders: u64,
}

impl BackgroundRenderer {
    /// The texture to paint for `background`, if one of the right kind is ready. While a
    /// newer one renders, the previous texture of the same kind keeps showing.
    pub fn texture(
        &self,
        background: &BackgroundSettings,
        canvas: Size,
    ) -> Option<BackgroundTexture> {
        let wanted = RenderKey::wanted(background, canvas)?;
        let current = self.current.as_ref()?;
        current
            .key
            .same_kind(&wanted)
            .then(|| current.texture.clone())
    }

    pub fn is_idle(&self) -> bool {
        self.running.is_none()
    }
}

impl CanvasView {
    /// Starts a render when the wanted background differs from what is rendered or running.
    /// One render at a time; when it lands the next render call picks up the latest change.
    pub fn sync_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for old in self.background.retired.drain(..) {
            window.drop_image(old).ok();
        }
        let canvas = Size::new(self.viewport.size.width, self.viewport.size.height);
        let Some(wanted) = RenderKey::wanted(&self.settings.settings.background, canvas) else {
            return;
        };
        let renderer = &mut self.background;
        let up_to_date = renderer.current.as_ref().is_some_and(|r| r.key == wanted);
        if up_to_date || renderer.running.is_some() || renderer.failed.as_ref() == Some(&wanted) {
            return;
        }
        let source = renderer.source.clone();
        let job_key = wanted.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { render(&job_key, source) })
                .await;
            this.update(cx, |view, cx| {
                view.background_finished(result, cx);
            })
            .ok();
        });
        renderer.running = Some((wanted, task));
    }

    fn background_finished(&mut self, result: RenderResult, cx: &mut Context<Self>) {
        let renderer = &mut self.background;
        let Some((key, _task)) = renderer.running.take() else {
            return;
        };
        let had_error = renderer.error.is_some();
        match result {
            Ok((texture, source)) => {
                if let Some(source) = source {
                    renderer.source = Some(source);
                }
                if let Some(old) = renderer.current.take() {
                    renderer.retired.push(old.texture.image);
                }
                renderer.current = Some(Rendered { key, texture });
                renderer.failed = None;
                renderer.error = None;
                renderer.renders += 1;
            }
            Err(error) => {
                eprintln!("calm: background: {error}");
                renderer.failed = Some(key);
                renderer.error = Some(error);
            }
        }
        if had_error != self.background.error.is_some() {
            self.refresh_settings_form();
        }
        cx.notify();
    }
}

type RenderResult = Result<(BackgroundTexture, Option<(PathBuf, Arc<RgbaImage>)>), String>;

/// Runs on a background thread.
fn render(key: &RenderKey, source: Option<(PathBuf, Arc<RgbaImage>)>) -> RenderResult {
    match key {
        RenderKey::Gradient { gradient, raster } => {
            let (width, height) = *raster;
            let rgba = RgbaImage::from_raw(width, height, gradient.rasterize(width, height))
                .ok_or("gradient raster has the wrong size")?;
            Ok((
                BackgroundTexture {
                    image: to_render_image(rgba),
                    pixels: (width, height),
                    fit: None,
                },
                None,
            ))
        }
        RenderKey::Image(background) => {
            let path = background
                .path
                .as_deref()
                .ok_or("no background image chosen")?;
            let (decoded, new_source) = match source {
                Some((cached_path, image)) if cached_path == path => (image, None),
                _ => {
                    let image = Arc::new(decode(path)?);
                    (image.clone(), Some((path.to_path_buf(), image)))
                }
            };
            let filters = &background.filters;
            let mut rgba = if filters.blur > 0.0 {
                image::imageops::fast_blur(decoded.as_ref(), filters.blur as f32)
            } else {
                decoded.as_ref().clone()
            };
            filters.apply_color(&mut rgba);
            if background.fit == ImageFit::Tile {
                rgba = repeat_tile(&rgba);
            }
            let pixels = rgba.dimensions();
            Ok((
                BackgroundTexture {
                    image: to_render_image(rgba),
                    pixels,
                    fit: Some(background.fit),
                },
                new_source,
            ))
        }
    }
}

fn decode(path: &Path) -> Result<RgbaImage, String> {
    let image =
        image::open(path).map_err(|err| format!("cannot read image {}: {err}", path.display()))?;
    let image = if image.width().max(image.height()) > MAX_IMAGE_EDGE {
        image.thumbnail(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE)
    } else {
        image
    };
    Ok(image.into_rgba8())
}

fn repeat_tile(tile: &RgbaImage) -> RgbaImage {
    let (width, height) = tile.dimensions();
    let (across, down) = tile_repeat(width, height);
    if (across, down) == (1, 1) {
        return tile.clone();
    }
    let mut out = RgbaImage::new(width * across, height * down);
    for row in 0..down {
        for column in 0..across {
            image::imageops::replace(
                &mut out,
                tile,
                i64::from(column * width),
                i64::from(row * height),
            );
        }
    }
    out
}

/// GPUI textures are BGRA.
fn to_render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(rgba)]))
}

/// Paints `texture` over `bounds`. Call inside the canvas content mask.
pub fn paint_background(texture: &BackgroundTexture, bounds: Bounds<Pixels>, window: &mut Window) {
    let canvas = Rect::new(
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
    let rects = match texture.fit {
        None => vec![canvas],
        Some(fit) => {
            // One image pixel per device pixel for Center and Tile.
            let scale = f64::from(window.scale_factor()).max(1.0);
            let (width, height) = texture.pixels;
            let image = Size::new(f64::from(width) / scale, f64::from(height) / scale);
            image_dest_rects(fit, image, canvas)
        }
    };
    for rect in rects {
        let image_bounds = Bounds {
            origin: point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
            size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
        };
        if let Err(err) = window.paint_image(
            bounds,
            image_bounds,
            Default::default(),
            texture.image.clone(),
            0,
            false,
        ) {
            eprintln!("calm: background paint: {err:#}");
            return;
        }
    }
}
