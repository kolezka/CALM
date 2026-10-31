//! Native context menu. An NSMenu draws above Ghostty and WKWebView, a GPUI menu would not.
//!
//! Each action item gets a tag (its index into `ids`) and targets a small helper object
//! whose `calmPick:` method stores that tag. `popUpMenuPositioningItem:atLocation:inView:`
//! tracks the menu modally and sends the action before it returns, so the tag is ready then.

use std::cell::Cell;

use calm_core::geometry::Point;
use calm_core::menu::MenuItem;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSInteger, NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::NativeError;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class adds no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CALMContextMenuTarget"]
    #[ivars = Cell<Option<NSInteger>>]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(calmPick:))]
        fn pick(&self, sender: &NSMenuItem) {
            self.ivars().set(Some(sender.tag()));
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(None));
        // SAFETY: plain NSObject init on a freshly allocated instance with ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The view menus pop up in: the GPUI window's content view, retained.
pub struct HostView {
    view: Retained<NSView>,
}

impl HostView {
    pub fn new(window: &impl HasWindowHandle) -> Result<HostView, NativeError> {
        MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let handle = window
            .window_handle()
            .map_err(|err| NativeError::UnsupportedWindowHandle(err.to_string()))?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return Err(NativeError::UnsupportedWindowHandle(
                "not an AppKit handle".to_string(),
            ));
        };
        // SAFETY: an AppKit window handle's ns_view is a live NSView while the handle is
        // borrowed, and we are on the main thread. Retaining keeps it alive afterwards.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        Ok(HostView {
            view: view.retain(),
        })
    }
}

/// A built NSMenu tree plus the action ids its tags index into.
pub struct ContextMenu {
    menu: Retained<NSMenu>,
    target: Retained<MenuTarget>,
    ids: Vec<String>,
}

impl ContextMenu {
    pub fn build(items: &[MenuItem]) -> Result<ContextMenu, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let target = MenuTarget::new(mtm);
        let mut ids = Vec::new();
        let menu = build_menu(mtm, "", items, &target, &mut ids);
        Ok(ContextMenu { menu, target, ids })
    }

    /// (all NSMenuItems in the tree including separators, submenus), read back from AppKit.
    pub fn counts(&self) -> (usize, usize) {
        count_tree(&self.menu)
    }

    /// Shows the menu with its top-left corner at `point` (top-left based logical points in
    /// the host view) and blocks until it closes. Returns the chosen id.
    pub fn pop_up(&self, point: Point, host: &HostView) -> Option<String> {
        self.target.ivars().set(None);
        let view = &host.view;
        let (x, y) = appkit_point(point, view.bounds().size.height, view.isFlipped());
        self.menu
            .popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(x, y), Some(view));
        let tag = self.target.ivars().take()?;
        usize::try_from(tag)
            .ok()
            .and_then(|index| self.ids.get(index).cloned())
    }
}

/// Builds and pops up a context menu. `None` when dismissed or off the main thread.
pub fn show_context_menu(
    items: &[MenuItem],
    screen_point: Point,
    host: &HostView,
) -> Option<String> {
    ContextMenu::build(items).ok()?.pop_up(screen_point, host)
}

/// A top-left based point in a view's coordinates. Unflipped views count y from the bottom.
pub fn appkit_point(point: Point, view_height: f64, flipped: bool) -> (f64, f64) {
    if flipped {
        (point.x, point.y)
    } else {
        (point.x, view_height - point.y)
    }
}

fn build_menu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[MenuItem],
    target: &MenuTarget,
    ids: &mut Vec<String>,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        let ns_item = match item {
            MenuItem::Separator => NSMenuItem::separatorItem(mtm),
            MenuItem::Action { id, label } => {
                // SAFETY: `calmPick:` is defined on MenuTarget above and takes the item.
                let ns_item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label),
                        Some(sel!(calmPick:)),
                        &NSString::from_str(""),
                    )
                };
                // SAFETY: the item holds the target weakly; ContextMenu owns it for the
                // menu's lifetime.
                unsafe { ns_item.setTarget(Some(target.as_ref() as &AnyObject)) };
                ns_item.setTag(ids.len() as NSInteger);
                ids.push(id.clone());
                ns_item
            }
            MenuItem::Submenu { label, items } => {
                let submenu = build_menu(mtm, label, items, target, ids);
                // SAFETY: a menu item with no action and an empty key equivalent.
                let ns_item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label),
                        None,
                        &NSString::from_str(""),
                    )
                };
                ns_item.setSubmenu(Some(&submenu));
                ns_item
            }
        };
        menu.addItem(&ns_item);
    }
    menu
}

fn count_tree(menu: &NSMenu) -> (usize, usize) {
    let mut items = 0;
    let mut submenus = 0;
    for item in menu.itemArray().iter() {
        items += 1;
        if let Some(submenu) = item.submenu() {
            submenus += 1;
            let (nested_items, nested_submenus) = count_tree(&submenu);
            items += nested_items;
            submenus += nested_submenus;
        }
    }
    (items, submenus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appkit_point_flips_y_for_unflipped_views() {
        assert_eq!(
            appkit_point(Point::new(10.0, 30.0), 800.0, false),
            (10.0, 770.0)
        );
        assert_eq!(
            appkit_point(Point::new(10.0, 30.0), 800.0, true),
            (10.0, 30.0)
        );
    }
}
