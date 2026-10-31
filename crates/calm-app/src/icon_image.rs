//! Decodes a TUI plugin's image icon once into a small texture for the chrome icon slot.

use std::path::Path;
use std::sync::Arc;

use gpui::RenderImage;
use image::RgbaImage;

use crate::background::to_render_image;

/// Edge of the decoded icon in pixels. The slot is 14 world units, so this stays sharp up
/// to about 4x zoom on a 2x display.
pub const ICON_TEXTURE_PX: u32 = 112;

pub fn load(path: &Path) -> Result<Arc<RenderImage>, String> {
    decode(path).map(to_render_image)
}

fn decode(path: &Path) -> Result<RgbaImage, String> {
    let is_svg = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
    if is_svg {
        return decode_svg(path);
    }
    let image =
        image::open(path).map_err(|err| format!("cannot read icon {}: {err}", path.display()))?;
    Ok(image
        .thumbnail(ICON_TEXTURE_PX, ICON_TEXTURE_PX)
        .into_rgba8())
}

fn decode_svg(path: &Path) -> Result<RgbaImage, String> {
    use resvg::{tiny_skia, usvg};
    let data =
        std::fs::read(path).map_err(|err| format!("cannot read icon {}: {err}", path.display()))?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default())
        .map_err(|err| format!("cannot parse icon {}: {err}", path.display()))?;
    let size = tree.size();
    let scale = ICON_TEXTURE_PX as f32 / size.width().max(size.height());
    let width = ((size.width() * scale).round() as u32).max(1);
    let height = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("icon {} has no area", path.display()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| format!("icon {} decoded to a bad buffer", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_and_svg_to_icon_size() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("icon.png");
        RgbaImage::from_pixel(512, 256, image::Rgba([255, 0, 0, 255]))
            .save(&png)
            .unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!(decoded.dimensions(), (ICON_TEXTURE_PX, ICON_TEXTURE_PX / 2));

        let svg = dir.path().join("icon.svg");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##,
        )
        .unwrap();
        let decoded = decode(&svg).unwrap();
        assert_eq!(decoded.dimensions(), (ICON_TEXTURE_PX, ICON_TEXTURE_PX));
        assert_eq!(decoded.get_pixel(5, 5).0, [0, 255, 0, 255]);
    }

    #[test]
    fn broken_icons_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("bad.svg");
        std::fs::write(&svg, "not svg").unwrap();
        assert!(decode(&svg).unwrap_err().contains("cannot parse icon"));
        let png = dir.path().join("bad.png");
        std::fs::write(&png, "not png").unwrap();
        assert!(decode(&png).unwrap_err().contains("cannot read icon"));
    }
}
