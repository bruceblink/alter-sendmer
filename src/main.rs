#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Instant;

fn main() {
    if let Err(error) = alter_sendme_gpui::run(Instant::now()) {
        eprintln!("AlterSendmer failed to start: {error}");
        std::process::exit(1);
    }
}
