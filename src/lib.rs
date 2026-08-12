//! Native GPUI application for AlterSendme.

mod app;
mod locale;
mod transfer;

use gpui::*;
use std::time::{Duration, Instant};

actions!(alter_sendme_gpui, [Quit, Tab, TabPrev]);

/// Starts the native window and installs the same quit bindings used by flash-shot.
pub fn run(started_at: Instant) -> Result<(), Box<dyn std::error::Error>> {
    // Install one process-wide tracing subscriber so sendmer cleanup and connection diagnostics
    // remain visible without writing tickets or file contents to persistent logs.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("alter_sendme_gpui=info,sendmer=info")
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
    // A host embedding GPUI may have already selected a rustls provider. That is compatible
    // with reqwest's rustls-no-provider feature, so provider installation is intentionally
    // best-effort instead of turning a second startup into a fatal error.
    let _ = rustls::crypto::ring::default_provider().install_default();
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
                let (send_result, receive_done) =
                    app_for_quit.update(cx, |app, _| app.take_shutdown_resources());
                async move {
                    let cleanup = async move {
                        if let Some(done) = receive_done {
                            let _ = done.await;
                        }
                        if let Some(result) = send_result {
                            let _ = result.shutdown().await;
                        }
                    };
                    let _ = tokio::time::timeout(Duration::from_secs(2), cleanup).await;
                }
            })
            .detach();
            app
        }) {
            tracing::error!(error = %error, "failed to open AlterSendme window");
            cx.quit();
        }
    });
    Ok(())
}
