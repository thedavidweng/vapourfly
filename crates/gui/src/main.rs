//! GPUI desktop entry for Vapourfly.

use std::path::PathBuf;

use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px,
    size,
};
use gpui_component::Root;
use vapourfly_gui::ui::GuiRoot;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixtures_path = args
        .windows(2)
        .find(|w| w[0] == "--fixtures")
        .map(|w| PathBuf::from(&w[1]));
    let ui_demo = args.iter().any(|a| a == "--ui-demo");
    let offline = args.iter().any(|a| a == "--offline");

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1440.), px(960.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1024.), px(700.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Vapourfly".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| GuiRoot::new(window, cx, fixtures_path, ui_demo, offline));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .ok();
        cx.activate(true);
    });
}
