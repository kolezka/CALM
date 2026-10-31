use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled, Window,
    WindowBounds, WindowOptions, div, px, rgb, size,
};

struct Root;

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xe0e0e0))
            .text_xl()
            .child("CALM")
    }
}

#[derive(Default)]
struct Args {
    smoke: bool,
    fullscreen: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--smoke" => args.smoke = true,
            "--fullscreen" => args.fullscreen = true,
            other => eprintln!("calm: ignoring unknown argument {other}"),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let window_bounds = if args.fullscreen {
            WindowBounds::Fullscreen(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        };
        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            ..Default::default()
        };
        let smoke = args.smoke;
        cx.open_window(options, move |window, cx| {
            if smoke {
                // Quit only after a real frame, so a passing smoke means the window drew.
                window.on_next_frame(|window, cx| {
                    let viewport = window.viewport_size();
                    println!(
                        "smoke: viewport={}x{} scale={}",
                        f32::from(viewport.width),
                        f32::from(viewport.height),
                        window.scale_factor()
                    );
                    cx.quit();
                });
            }
            cx.new(|_| Root)
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}
