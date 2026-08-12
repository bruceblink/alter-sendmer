//! Native GPUI application for AlterSendme.

mod app;
mod locale;
mod transfer;

use gpui::*;
use std::time::Instant;

actions!(alter_sendme_gpui, [Quit, Tab, TabPrev]);

/// Starts the native window and installs the same quit bindings used by flash-shot.
pub fn run(started_at: Instant) -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "rustls crypto provider was already installed")?;
    // Keep a Tokio runtime entered for the GPUI event loop because transfer actions
    // create Tokio tasks while sendmer performs network and filesystem work.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _runtime_guard = runtime.enter();
    gpui_platform::application().run(move |cx| {
        cx.set_menus(vec![Menu {
            name: "AlterSendme".into(),
            items: vec![MenuItem::action("Quit AlterSendme", Quit)],
            disabled: false,
        }]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("alt-f4", Quit, None),
            KeyBinding::new("tab", Tab, None),
            KeyBinding::new("shift-tab", TabPrev, None),
        ]);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1024.0), px(640.0)), cx)),
            window_min_size: Some(size(px(760.0), px(560.0))),
            is_resizable: true,
            is_minimizable: true,
            titlebar: Some(TitlebarOptions {
                title: Some("AlterSendme".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        if let Err(error) = cx.open_window(options, move |window, cx| {
            let app = cx.new(|cx| app::AlterSendmeApp::new(started_at, window, cx));
            let close_app = app.clone();
            window.on_window_should_close(cx, move |_, cx| {
                close_app.update(cx, |app, _| app.stop_on_exit());
                true
            });
            app
        }) {
            log::error!("failed to open AlterSendme window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
