mod ui;

use gpui::{prelude::*, *};
use std::{path::PathBuf, time::Instant};

fn main() {
    let startup = Instant::now();
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    gpui_platform::application()
        .with_assets(ui::theme::Assets)
        .run(move |cx| {
            ui::menus(cx);
            cx.set_window_appearance(Some(WindowAppearance::Dark));
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1380.), px(860.)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(900.), px(600.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Plait".into()),
                        ..Default::default()
                    }),
                    app_id: Some("dev.plait.desktop".into()),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| ui::Plait::new(path, startup, window, cx)),
            ) {
                eprintln!("Cannot open Plait: {error}");
                cx.quit();
            }
            cx.activate(true);
        });
}
