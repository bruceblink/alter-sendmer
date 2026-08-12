#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Instant;

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    let _guard = runtime.enter();
    if let Err(error) = alter_sendme_gpui::run(Instant::now()) {
        eprintln!("AlterSendme failed to start: {error}");
        std::process::exit(1);
    }
}
