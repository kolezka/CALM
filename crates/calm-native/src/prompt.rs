//! App-modal NSAlert dialogs: a one-line text prompt, a confirmation, a choice from a list
//! and a plain alert. Each blocks until the user answers, so call them outside any GPUI
//! update.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSPopUpButton, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::NativeError;

const FIELD_WIDTH: f64 = 320.0;
const FIELD_HEIGHT: f64 = 24.0;
const POPUP_HEIGHT: f64 = 26.0;
const CANCEL: &str = "Cancel";
const OK: &str = "OK";

/// What a text prompt shows. `ok` titles the default button; the field starts with `initial`.
pub struct TextPrompt<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub note: &'a str,
    pub placeholder: &'a str,
    pub initial: &'a str,
    pub ok: &'a str,
}

/// Asks for one line of text with `ok` and Cancel. `None` when cancelled or the text is
/// blank; otherwise the text trimmed.
pub fn prompt_text(prompt: &TextPrompt) -> Result<Option<String>, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let informative = if prompt.message.is_empty() {
        prompt.note.to_string()
    } else {
        format!("{}\n\n{}", prompt.message, prompt.note)
    };
    let alert = new_alert(mtm, prompt.title, &informative, &[prompt.ok, CANCEL]);
    let field = NSTextField::textFieldWithString(&NSString::from_str(prompt.initial), mtm);
    field.setPlaceholderString(Some(&NSString::from_str(prompt.placeholder)));
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

/// Asks a yes or no question with `ok` and Cancel. True when the user picks `ok`.
pub fn confirm(title: &str, message: &str, ok: &str) -> Result<bool, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let alert = new_alert(mtm, title, message, &[ok, CANCEL]);
    Ok(alert.runModal() == NSAlertFirstButtonReturn)
}

/// Asks the user to pick one of `options` from a pop-up menu, the first one preselected.
/// The index picked, or `None` on Cancel. Empty `options` is `None` without showing anything.
pub fn choose(
    title: &str,
    message: &str,
    options: &[String],
) -> Result<Option<usize>, NativeError> {
    if options.is_empty() {
        return Ok(None);
    }
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let alert = new_alert(mtm, title, message, &[OK, CANCEL]);
    let frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(FIELD_WIDTH, POPUP_HEIGHT),
    );
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
    for option in options {
        popup.addItemWithTitle(&NSString::from_str(option));
    }
    popup.selectItemAtIndex(0);
    alert.setAccessoryView(Some(&popup));
    alert.layout();
    alert.window().setInitialFirstResponder(Some(&popup));
    if alert.runModal() != NSAlertFirstButtonReturn {
        return Ok(None);
    }
    Ok(usize::try_from(popup.indexOfSelectedItem())
        .ok()
        .filter(|index| *index < options.len()))
}

/// Tells the user something, with one OK button.
pub fn alert(title: &str, message: &str) -> Result<(), NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    new_alert(mtm, title, message, &[OK]).runModal();
    Ok(())
}

/// An alert with its texts and buttons. The first button is the default: Return picks it,
/// and a button titled Cancel answers Escape.
fn new_alert(
    mtm: MainThreadMarker,
    title: &str,
    message: &str,
    buttons: &[&str],
) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    alert
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

    #[test]
    fn choose_with_no_options_is_none() {
        assert_eq!(choose("Load Preset", "Pick one", &[]), Ok(None));
    }

    #[test]
    fn dialogs_refuse_off_the_main_thread() {
        std::thread::spawn(|| {
            let prompt = TextPrompt {
                title: "Save Preset",
                message: "",
                note: "",
                placeholder: "",
                initial: "",
                ok: "Save",
            };
            assert_eq!(prompt_text(&prompt), Err(NativeError::NotMainThread));
            assert_eq!(
                confirm("Load", "Replace?", "Load"),
                Err(NativeError::NotMainThread)
            );
            assert_eq!(
                choose("Load", "Pick one", &["one".to_string()]),
                Err(NativeError::NotMainThread)
            );
            assert_eq!(alert("Saved", "Done"), Err(NativeError::NotMainThread));
        })
        .join()
        .expect("dialog checks panicked");
    }
}
