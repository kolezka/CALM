//! Image icons chosen for one panel. The file is copied into the settings folder, so the
//! override keeps working after the original moves, and only a relative path is stored.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::terminal_profile::ICON_IMAGE_EXTENSIONS;

/// Folder under the settings folder that holds imported panel icons.
pub const PANEL_ICONS_DIR: &str = "panel-icons";

/// Hex digits of the content hash kept in the file name.
const HASH_PREFIX_LEN: usize = 16;

/// Copies a .png or .svg into `<settings_dir>/panel-icons/<sha256 prefix>.<ext>` and returns
/// that path relative to `settings_dir`. The name comes from the content, so the same image
/// imported twice gives one file.
pub fn import_icon(settings_dir: &Path, source: &Path) -> Result<String, String> {
    let ext = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| ICON_IMAGE_EXTENSIONS.contains(&ext.as_str()))
        .ok_or_else(|| {
            format!(
                "panel icon {}: only .png and .svg files can be icons",
                source.display()
            )
        })?;
    let bytes =
        std::fs::read(source).map_err(|err| format!("panel icon {}: {err}", source.display()))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = format!("{}.{ext}", &hash[..HASH_PREFIX_LEN]);
    let dir = settings_dir.join(PANEL_ICONS_DIR);
    let target = dir.join(&name);
    if !target.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("panel icons folder {}: {err}", dir.display()))?;
        // Write aside, then rename, so a reader never sees half an icon.
        let partial = dir.join(format!(".{name}.partial"));
        std::fs::write(&partial, &bytes)
            .and_then(|()| std::fs::rename(&partial, &target))
            .map_err(|err| format!("panel icon {}: {err}", target.display()))?;
    }
    Ok(format!("{PANEL_ICONS_DIR}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_png_and_rejects_jpg() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("logo.PNG");
        std::fs::write(&source, b"png bytes").unwrap();
        let settings = dir.path().join("settings");

        let relative = import_icon(&settings, &source).unwrap();
        let path = Path::new(&relative);
        assert!(path.is_relative(), "{relative}");
        assert!(relative.starts_with("panel-icons/"), "{relative}");
        assert!(relative.ends_with(".png"), "{relative}");
        assert!(crate::terminal_profile::is_image_icon(&relative));
        assert_eq!(std::fs::read(settings.join(path)).unwrap(), b"png bytes");

        let svg = dir.path().join("logo.svg");
        std::fs::write(&svg, b"<svg/>").unwrap();
        assert!(import_icon(&settings, &svg).unwrap().ends_with(".svg"));

        let jpg = dir.path().join("photo.jpg");
        std::fs::write(&jpg, b"jpg bytes").unwrap();
        let err = import_icon(&settings, &jpg).unwrap_err();
        assert!(err.contains("photo.jpg"), "{err}");
        assert_eq!(
            std::fs::read_dir(settings.join(PANEL_ICONS_DIR))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn same_file_twice_gives_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.png");
        let copy = dir.path().join("b.png");
        let other = dir.path().join("c.png");
        std::fs::write(&first, b"same").unwrap();
        std::fs::write(&copy, b"same").unwrap();
        std::fs::write(&other, b"different").unwrap();

        let a = import_icon(dir.path(), &first).unwrap();
        assert_eq!(import_icon(dir.path(), &first).unwrap(), a);
        assert_eq!(import_icon(dir.path(), &copy).unwrap(), a);
        assert_ne!(import_icon(dir.path(), &other).unwrap(), a);
        assert_eq!(
            std::fs::read_dir(dir.path().join(PANEL_ICONS_DIR))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn a_missing_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = import_icon(dir.path(), &dir.path().join("gone.png")).unwrap_err();
        assert!(err.contains("gone.png"), "{err}");
    }
}
