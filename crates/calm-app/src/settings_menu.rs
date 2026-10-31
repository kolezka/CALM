//! The Settings menu bar menu, its actions and the canvas dot grid.

use std::path::PathBuf;

use calm_core::camera::Camera;
use calm_core::geometry::Point;
use calm_core::settings::{
    BackgroundPreset, GRID_STEP_WORLD, Settings, grid_dot_rgb, grid_screen_spacing,
    save_settings_to,
};
use gpui::{
    Action, App, Bounds, Context, InteractiveElement, Menu, MenuItem, Pixels, Window, actions,
    fill, point, px, rgb, size,
};
use gpui_libghostty::{TerminalConfiguration, TerminalOptions};

use crate::canvas_view::CanvasView;
use crate::dev_menu;

actions!(
    calm,
    [
        ToggleGrid,
        ToggleConfirmClose,
        ToggleTerminalUserConfig,
        OpenConfigFolder,
        ResetSettings
    ]
);

/// Menu items carry the preset, so no JSON form is needed.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = calm, no_json)]
pub struct SetBackground(pub BackgroundPreset);

const GRID_DOT_PX: f32 = 2.0;

/// Settings plus the file they persist to. The smoke points `path` at a temp dir.
pub struct SettingsState {
    pub settings: Settings,
    pub path: PathBuf,
}

/// Every menu bar menu, so the Developer and Settings menus are always set together.
pub fn menus(show_fps: bool, settings: &Settings) -> Vec<Menu> {
    let mut menus = dev_menu::menus(show_fps);
    menus.push(settings_menu(settings));
    menus
}

fn settings_menu(settings: &Settings) -> Menu {
    let backgrounds = BackgroundPreset::all().iter().map(|preset| {
        MenuItem::action(preset.label(), SetBackground(*preset))
            .checked(*preset == settings.background)
    });
    Menu::new("Settings").items([
        MenuItem::submenu(Menu::new("Background").items(backgrounds)),
        MenuItem::action("Show Grid", ToggleGrid).checked(settings.show_grid),
        MenuItem::action("Confirm Before Closing Panels", ToggleConfirmClose)
            .checked(settings.confirm_close),
        MenuItem::action(
            "Terminal Uses Ghostty User Config",
            ToggleTerminalUserConfig,
        )
        .checked(settings.terminal_user_config),
        MenuItem::separator(),
        MenuItem::action("Open Config Folder", OpenConfigFolder),
        MenuItem::action("Reset to Defaults", ResetSettings),
    ])
}

pub fn install(cx: &mut App, settings: &Settings) {
    cx.set_menus(menus(false, settings));
}

/// Attaches the Settings action handlers to the canvas root element.
pub fn bind<E: InteractiveElement>(element: E, cx: &mut Context<CanvasView>) -> E {
    element
        .on_action(cx.listener(|view, action: &SetBackground, window, cx| {
            let preset = action.0;
            view.update_settings(|s| s.background = preset, window, cx);
        }))
        .on_action(cx.listener(|view, _: &ToggleGrid, window, cx| {
            view.update_settings(|s| s.show_grid = !s.show_grid, window, cx);
        }))
        .on_action(cx.listener(|view, _: &ToggleConfirmClose, window, cx| {
            view.update_settings(|s| s.confirm_close = !s.confirm_close, window, cx);
        }))
        .on_action(
            cx.listener(|view, _: &ToggleTerminalUserConfig, window, cx| {
                view.update_settings(
                    |s| s.terminal_user_config = !s.terminal_user_config,
                    window,
                    cx,
                );
            }),
        )
        .on_action(cx.listener(|view, _: &ResetSettings, window, cx| {
            view.update_settings(|s| *s = Settings::default(), window, cx);
        }))
        .on_action(cx.listener(|view, _: &OpenConfigFolder, _, _| {
            open_config_folder(&view.settings.path);
        }))
}

impl CanvasView {
    /// Applies `change`, saves at once and rebuilds the menus so check marks follow.
    pub fn update_settings(
        &mut self,
        change: impl FnOnce(&mut Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        change(&mut self.settings.settings);
        if let Err(err) = save_settings_to(&self.settings.path, &self.settings.settings) {
            eprintln!("calm: {err}");
        }
        let (show_fps, settings) = (self.show_fps, self.settings.settings);
        cx.defer(move |cx| cx.set_menus(menus(show_fps, &settings)));
        window.refresh();
        cx.notify();
    }
}

fn open_config_folder(settings_path: &std::path::Path) {
    let Some(dir) = settings_path.parent().map(PathBuf::from) else {
        eprintln!(
            "calm: settings path {} has no folder",
            settings_path.display()
        );
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("calm: cannot create {}: {err}", dir.display());
        return;
    }
    // `open` returns quickly, but waiting on it still belongs off the UI thread.
    std::thread::spawn(move || {
        match std::process::Command::new("/usr/bin/open")
            .arg(&dir)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("calm: open {} exited with {status}", dir.display()),
            Err(err) => eprintln!("calm: open {} failed: {err}", dir.display()),
        }
    });
}

/// Without the user config Ghostty uses its built-in defaults.
pub fn apply_terminal_config(options: &mut TerminalOptions, settings: &Settings) {
    if !settings.terminal_user_config {
        options.configuration = TerminalConfiguration::Default;
    }
}

/// Dots every `GRID_STEP_WORLD` world units inside `bounds`. Paint it before panels.
pub fn paint_grid(bounds: Bounds<Pixels>, camera: &Camera, background: u32, window: &mut Window) {
    let Some(spacing) = grid_screen_spacing(camera.zoom) else {
        return;
    };
    let width = f64::from(f32::from(bounds.size.width));
    let height = f64::from(f32::from(bounds.size.height));
    let first = camera.screen_to_world(Point::new(0.0, 0.0));
    let start = camera.world_to_screen(Point::new(
        (first.x / GRID_STEP_WORLD).ceil() * GRID_STEP_WORLD,
        (first.y / GRID_STEP_WORLD).ceil() * GRID_STEP_WORLD,
    ));
    let color = rgb(grid_dot_rgb(background));
    let half = f64::from(GRID_DOT_PX) / 2.0;
    let origin_x = f64::from(f32::from(bounds.origin.x));
    let origin_y = f64::from(f32::from(bounds.origin.y));
    let mut y = start.y;
    while y < height {
        let mut x = start.x;
        while x < width {
            let dot = Bounds {
                origin: point(
                    px((origin_x + x - half) as f32),
                    px((origin_y + y - half) as f32),
                ),
                size: size(px(GRID_DOT_PX), px(GRID_DOT_PX)),
            };
            window.paint_quad(fill(dot, color));
            x += spacing;
        }
        y += spacing;
    }
}
