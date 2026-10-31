//! The brand icon for the Dock, cmd-tab, the About panel and NSAlert sheets.
//!
//! A bundled app gets its icon from `CALM.icns` through `CFBundleIconFile`. A bare binary
//! from `cargo run` has no bundle, so the icon is set at runtime from embedded PNG bytes.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationIcon, NSAboutPanelOptionApplicationName,
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionCredits, NSAboutPanelOptionKey,
    NSAboutPanelOptionVersion, NSApplication, NSImage,
};
use objc2_foundation::{NSAttributedString, NSBundle, NSData, NSDictionary, NSString};

use crate::NativeError;

/// 512 px: sharp in the Dock on a 2x display without the 1024 px file's weight.
pub const BRAND_ICON_PNG: &[u8] =
    include_bytes!("../../../assets/brand/CALM.iconset/icon_512x512.png");

/// True once `install_app_icon` set the brand icon and read back a changed icon.
static BRAND_ICON_SET: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIconError {
    /// AppKit could not decode the bytes, or the result has no size or cannot draw.
    Decode(String),
    /// The set did not take effect.
    NotApplied,
    Native(NativeError),
}

impl fmt::Display for AppIconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppIconError::Decode(reason) => write!(f, "app icon did not decode: {reason}"),
            AppIconError::NotApplied => {
                write!(
                    f,
                    "AppKit kept the default icon after setApplicationIconImage"
                )
            }
            AppIconError::Native(err) => write!(f, "app icon: {err}"),
        }
    }
}

impl std::error::Error for AppIconError {}

/// What `install_app_icon` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIconInstall {
    /// The brand image is now the application icon.
    Set,
    /// Running from a bundle that names its own icon, which already has the hand-tuned sizes.
    BundleIcon,
}

/// Decodes PNG bytes into an image. Fails for bytes AppKit cannot read or draw.
pub fn image_from_png(bytes: &[u8]) -> Result<Retained<NSImage>, AppIconError> {
    // NSImage reads only the header up front, so corrupt pixel data would pass its checks
    // and draw as garbage later. A full decode catches it here.
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|err| AppIconError::Decode(err.to_string()))?;
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| AppIconError::Decode(format!("NSImage rejected {} bytes", bytes.len())))?;
    if !image.isValid() {
        return Err(AppIconError::Decode("NSImage is not drawable".into()));
    }
    let size = image.size();
    if size.width <= 0.0 || size.height <= 0.0 {
        return Err(AppIconError::Decode(format!(
            "zero size {}x{}",
            size.width, size.height
        )));
    }
    Ok(image)
}

/// The embedded brand icon.
pub fn brand_icon() -> Result<Retained<NSImage>, AppIconError> {
    image_from_png(BRAND_ICON_PNG)
}

fn bundle_names_icon() -> bool {
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleIconFile"))
        .is_some()
}

/// Sets the brand icon as the application icon unless a bundle icon is present.
/// On error the default icon stays and the caller must log the error.
pub fn install_app_icon() -> Result<AppIconInstall, AppIconError> {
    let mtm = MainThreadMarker::new().ok_or(AppIconError::Native(NativeError::NotMainThread))?;
    if bundle_names_icon() {
        return Ok(AppIconInstall::BundleIcon);
    }
    let image = brand_icon()?;
    let app = NSApplication::sharedApplication(mtm);
    let before = app
        .applicationIconImage()
        .and_then(|icon| icon.TIFFRepresentation());
    // SAFETY: on the main thread (checked above), and `image` is a valid, non-nil NSImage.
    unsafe { app.setApplicationIconImage(Some(&image)) };
    // AppKit stores a re-rendered copy (no name, other bytes), so the check is that the
    // icon changed from the default, read back right after the set.
    let after = app
        .applicationIconImage()
        .and_then(|icon| icon.TIFFRepresentation());
    let changed = match (&before, &after) {
        (Some(before), Some(after)) => !before.isEqualToData(after),
        (None, Some(_)) => true,
        (_, None) => false,
    };
    if !changed {
        return Err(AppIconError::NotApplied);
    }
    BRAND_ICON_SET.store(true, Ordering::Relaxed);
    Ok(AppIconInstall::Set)
}

/// Whether `install_app_icon` set the brand icon and read back a change, and the current
/// icon's size in points.
pub fn app_icon_state() -> Result<(bool, f64, f64), NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let app = NSApplication::sharedApplication(mtm);
    let size = app
        .applicationIconImage()
        .map(|icon| icon.size())
        .unwrap_or_default();
    Ok((
        BRAND_ICON_SET.load(Ordering::Relaxed),
        size.width,
        size.height,
    ))
}

/// Opens the standard About panel with the brand icon, `version` and a short credits line.
pub fn show_about_panel(version: &str, credits: &str) -> Result<(), AppIconError> {
    let mtm = MainThreadMarker::new().ok_or(AppIconError::Native(NativeError::NotMainThread))?;
    let app = NSApplication::sharedApplication(mtm);
    let name = NSString::from_str("CALM");
    let version = NSString::from_str(version);
    let credits = NSAttributedString::initWithString(
        NSAttributedString::alloc(),
        &NSString::from_str(credits),
    );
    let mut keys: Vec<&NSAboutPanelOptionKey> = vec![
        // SAFETY: AppKit's exported option key constants, valid for the process lifetime.
        unsafe { NSAboutPanelOptionApplicationName },
        unsafe { NSAboutPanelOptionApplicationVersion },
        unsafe { NSAboutPanelOptionCredits },
    ];
    let mut objects: Vec<&AnyObject> = vec![&name, &version, &credits];
    // Empty "Version" hides the build number in brackets, which would repeat the version.
    let empty = NSString::from_str("");
    // SAFETY: as above.
    keys.push(unsafe { NSAboutPanelOptionVersion });
    objects.push(&empty);
    let icon = match brand_icon() {
        Ok(icon) => Some(icon),
        Err(err) => {
            eprintln!("calm: About panel uses the default icon: {err}");
            None
        }
    };
    if let Some(icon) = &icon {
        // SAFETY: as above.
        keys.push(unsafe { NSAboutPanelOptionApplicationIcon });
        objects.push(icon);
    }
    let options = NSDictionary::from_slices(&keys, &objects);
    // SAFETY: main thread (checked above); every value has the type AppKit documents for its
    // key: NSString for name and versions, NSAttributedString for credits, NSImage for icon.
    unsafe { app.orderFrontStandardAboutPanelWithOptions(&options) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_brand_icon_decodes_with_a_size() {
        let image = image_from_png(BRAND_ICON_PNG).expect("brand icon decodes");
        let size = image.size();
        assert_eq!((size.width, size.height), (512.0, 512.0));
    }

    #[test]
    fn corrupted_png_is_an_error_not_a_panic() {
        // Garbage, an empty slice, and a real header with the pixel data cut off.
        let truncated = &BRAND_ICON_PNG[..64];
        let mut flipped = BRAND_ICON_PNG.to_vec();
        for byte in flipped.iter_mut().skip(64) {
            *byte ^= 0xA5;
        }
        for (label, bytes) in [
            ("garbage", &b"not a png at all"[..]),
            ("empty", &[][..]),
            ("truncated", truncated),
            ("flipped", &flipped[..]),
        ] {
            assert!(
                matches!(image_from_png(bytes), Err(AppIconError::Decode(_))),
                "{label} bytes must fail to decode"
            );
        }
    }
}
