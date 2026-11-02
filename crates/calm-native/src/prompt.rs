//! A one-line text prompt: an app-modal NSAlert with a text field.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::NativeError;

const FIELD_WIDTH: f64 = 320.0;
const FIELD_HEIGHT: f64 = 24.0;

/// Asks for one line of text with Run and Cancel. Blocks until the user answers, so call
/// it outside any GPUI update. `None` when cancelled or the text is blank; otherwise the
/// text trimmed.
pub fn prompt_text(
    title: &str,
    message: &str,
    note: &str,
    placeholder: &str,
) -> Result<Option<String>, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    let informative = if message.is_empty() {
        note.to_string()
    } else {
        format!("{message}\n\n{note}")
    };
    alert.setInformativeText(&NSString::from_str(&informative));
    // The first button is the default: Return runs, Escape cancels.
    alert.addButtonWithTitle(&NSString::from_str("Run"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    field.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(FIELD_WIDTH, FIELD_HEIGHT),
    ));
    alert.setAccessoryView(Some(&field));
    alert.layout();
    alert.window().setInitialFirstResponder(Some(&field));
    if alert.runModal() != NSAlertFirstButtonReturn {
        return Ok(None);
    }
    Ok(prompt_value(&field.stringValue().to_string()))
}

/// What a prompt answer means: blank is no answer.
pub fn prompt_value(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_answers_are_none_and_others_are_trimmed() {
        assert_eq!(prompt_value(""), None);
        assert_eq!(prompt_value(" \t\n"), None);
        assert_eq!(prompt_value("  htop -d 5 "), Some("htop -d 5".to_string()));
    }
}
