//! GPUI desktop entry for Vapourfly.

use std::path::PathBuf;

use gpui_kit::component::Root;
use gpui_kit::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
use vapourfly_gui::ThemeMode;
use vapourfly_gui::platform::{DeviceProfile, window_geometry};
use vapourfly_gui::ui::{GuiRoot, LaunchOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixtures_path = args
        .windows(2)
        .find(|w| w[0] == "--fixtures")
        .map(|w| PathBuf::from(&w[1]));
    let ui_demo = args.iter().any(|a| a == "--ui-demo");
    let offline = args.iter().any(|a| a == "--offline");
    // `--deck` previews the Steam Deck session in a 1280×800 window.
    let simulate_deck = args.iter().any(|a| a == "--deck");
    let theme = args
        .windows(2)
        .find(|w| w[0] == "--theme")
        .and_then(|w| match w[1].as_str() {
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        });
    let start_view = args
        .windows(2)
        .find(|w| w[0] == "--view")
        .map(|w| w[1].clone());
    let device = if simulate_deck {
        DeviceProfile::simulated_deck()
    } else {
        DeviceProfile::detect()
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let display = cx.primary_display().map(|d| {
                let s = d.bounds().size;
                (f32::from(s.width), f32::from(s.height))
            });
            let geometry = window_geometry(display, simulate_deck);
            let bounds = Bounds::centered(None, size(px(geometry.size.0), px(geometry.size.1)), cx);
            // Game Mode (gamescope) shows one fullscreen app at a time.
            let window_bounds = if device.game_mode && !simulate_deck {
                WindowBounds::Fullscreen(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            };
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    window_min_size: Some(size(px(geometry.min.0), px(geometry.min.1))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Vapourfly".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(16.), px(16.))),
                    }),
                    app_id: Some("vapourfly".into()),
                    ..Default::default()
                },
                move |window, cx| {
                    let options = LaunchOptions {
                        fixtures: fixtures_path,
                        ui_demo,
                        offline,
                        theme,
                        start_view,
                        device,
                    };
                    let view = cx.new(|cx| GuiRoot::new(window, cx, options));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .ok();
            cx.activate(true);
        });
}
