//! Native GPUI application for AlterSendme.

mod app;
mod locale;
mod transfer;

use gpui::*;
use std::time::{Duration, Instant};

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
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

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
            // Keep the entity alive until the app-level quit hook has drained transfer resources.
            let app_for_quit = app.clone();
            cx.on_app_quit(move |cx| {
                let send_result = app_for_quit.update(cx, |app, _| app.take_shutdown_resources());
                async move {
                    if let Some(result) = send_result {
                        let _ =
                            tokio::time::timeout(Duration::from_secs(2), result.shutdown()).await;
                    }
                }
            })
            .detach();
            app
        }) {
            log::error!("failed to open AlterSendme window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
