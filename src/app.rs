//! GPUI workbench and its state machine.

use crate::locale::Locale;
use crate::transfer;
use async_channel::{Receiver, Sender};
use gpui::prelude::*;
use gpui::{
    AsyncApp, Bounds, Context, ElementInputHandler, EntityInputHandler, ExternalPaths, FocusHandle,
    FontWeight, KeyDownEvent, PathPromptOptions, Render, Rgba, Role as A11yRole, UTF16Selection,
    Window, WindowAppearance, div, px, rgb,
};
use sendmer::{Role, SendResult, TransferEvent};
use serde::Deserialize;
use std::{
    ops::Range,
    path::PathBuf,
    time::{Duration, Instant},
};

const MAX_TICKET_LEN: usize = 16_384;
const UPDATE_MANIFEST_URL: &str =
    "https://github.com/bruceblink/alter-sendme/releases/latest/download/latest.json";
const GITHUB_RELEASE_URL: &str =
    "https://api.github.com/repos/bruceblink/alter-sendme/releases/latest";
const RELEASES_URL: &str = "https://github.com/bruceblink/alter-sendme/releases/latest";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Send,
    Receive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferPhase {
    Idle,
    Preparing,
    Sharing,
    Connecting,
    Transporting,
    Stopping,
    Completed,
}

#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    processed: u64,
    total: u64,
    speed: f64,
}

impl Progress {
    fn percentage(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            ((self.processed as f32 / self.total as f32) * 100.0).clamp(0.0, 100.0)
        }
    }
}

/// Owns all user-facing state and serializes async transfer completions by generation.
pub struct AlterSendmeApp {
    started_at: Instant,
    tab: Tab,
    theme: Theme,
    locale: Locale,
    send_phase: TransferPhase,
    receive_phase: TransferPhase,
    selected_path: Option<PathBuf>,
    selected_is_dir: bool,
    ticket: Option<String>,
    ticket_input: String,
    ticket_selection: Range<usize>,
    save_path: PathBuf,
    send_result: Option<SendResult>,
    send_abort: Option<tokio::task::AbortHandle>,
    receive_abort: Option<tokio::task::AbortHandle>,
    send_progress: Progress,
    receive_progress: Progress,
    receive_files: Vec<String>,
    status: String,
    error: Option<String>,
    completed_name: Option<String>,
    completed_path: Option<PathBuf>,
    completed_size: u64,
    completed_duration: Duration,
    transfer_started_at: Option<Instant>,
    event_sender: Sender<(u64, TransferEvent)>,
    ticket_focus: FocusHandle,
    generation: u64,
    update_checking: bool,
    update_info: Option<UpdateInfo>,
    update_status: Option<String>,
}

#[derive(Clone, Debug)]
struct UpdateInfo {
    version: String,
    download_url: String,
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    platforms: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

impl AlterSendmeApp {
    /// Creates a ready-to-use workspace and starts a lightweight event pump.
    pub fn new(started_at: Instant, cx: &mut Context<Self>) -> Self {
        let (event_sender, event_receiver) = async_channel::unbounded();
        let save_path = directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let app = Self {
            started_at,
            tab: Tab::Send,
            theme: Theme::System,
            locale: Locale::English,
            send_phase: TransferPhase::Idle,
            receive_phase: TransferPhase::Idle,
            selected_path: None,
            selected_is_dir: false,
            ticket: None,
            ticket_input: String::new(),
            ticket_selection: 0..0,
            save_path,
            send_result: None,
            send_abort: None,
            receive_abort: None,
            send_progress: Progress::default(),
            receive_progress: Progress::default(),
            receive_files: Vec::new(),
            status: "Ready".to_owned(),
            error: None,
            completed_name: None,
            completed_path: None,
            completed_size: 0,
            completed_duration: Duration::ZERO,
            transfer_started_at: None,
            event_sender,
            ticket_focus: cx.focus_handle(),
            generation: 0,
            update_checking: false,
            update_info: None,
            update_status: None,
        };
        let entity = cx.entity().downgrade();
        cx.spawn(move |_this: gpui::WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            let receiver = event_receiver;
            async move { pump_events(entity, receiver, &mut cx).await }
        })
        .detach();
        app
    }

    fn colors(&self, appearance: WindowAppearance) -> Palette {
        let dark = match self.theme {
            Theme::System => matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            Theme::Dark => true,
            Theme::Light => false,
        };
        if !dark {
            Palette {
                background: rgb(0xf2f5f9),
                panel: rgb(0xffffff),
                panel_alt: rgb(0xe7edf3),
                text: rgb(0x17201b),
                muted: rgb(0x617068),
                border: rgb(0xd0d9d4),
                accent: rgb(0x159447),
                accent_alt: rgb(0x2675ce),
                danger: rgb(0xb93c3c),
            }
        } else {
            Palette {
                background: rgb(0x191919),
                panel: rgb(0x242424),
                panel_alt: rgb(0x363636),
                text: rgb(0xf5f7f6),
                muted: rgb(0xa6b2ab),
                border: rgb(0x484f4b),
                accent: rgb(0x25d365),
                accent_alt: rgb(0x4e9bea),
                danger: rgb(0xf06b6b),
            }
        }
    }

    fn copy(&self, key: &str) -> &'static str {
        if let Some(value) = self.locale.ui_copy(key) {
            return value;
        }
        match (self.locale, key) {
            (_, "send") => "Send",
            (_, "receive") => "Receive",
            (_, "ready") => "Ready",
            (_, "select") => "Choose a file or folder",
            (_, "drop") => "Drop files or folders here",
            (_, "start") => "Start sharing",
            (_, "stop") => "Stop sharing",
            (_, "save") => "Save to folder",
            (_, "receive_action") => "Start receiving",
            (_, "copy") => "Copy ticket",
            (_, "new") => "New transfer",
            (_, "theme") => "Theme",
            (_, "language") => "Language",
            _ => "",
        }
    }

    fn text(&self, key: &str) -> String {
        self.locale
            .lookup(key)
            .or_else(|| Locale::English.lookup(key))
            .unwrap_or(key)
            .to_owned()
    }

    fn status_text(&self, key: &str, fallback: &str) -> String {
        let value = self.text(key);
        if value == key {
            fallback.to_owned()
        } else {
            value
        }
    }

    fn select_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.exists() {
            self.selected_is_dir = path.is_dir();
            self.selected_path = Some(path);
            self.error = None;
            self.status = self.status_text(
                if self.selected_is_dir {
                    "path_selected_folder"
                } else {
                    "path_selected_file"
                },
                "Path selected",
            );
            cx.notify();
        }
    }

    fn choose_send_path(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Choose file or folder".into()),
        });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = app.update(&mut cx, |app, cx| app.select_path(path, cx));
                }
            }
        })
        .detach();
    }

    fn choose_save_path(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose download folder".into()),
        });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = app.update(&mut cx, |app, cx| {
                        app.save_path = path;
                        app.status = app.status_text("save", "Download folder selected");
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Opens a completed download in the platform file manager.
    fn reveal_completed_path(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.completed_path.as_ref() {
            cx.reveal_path(path);
            self.status = self.status_text("download_completed", "Opened downloaded item");
            cx.notify();
        }
    }

    /// Checks the signed release manifest without blocking the GPUI event loop.
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.update_checking {
            return;
        }
        self.update_checking = true;
        self.update_status = Some(self.text("update.checking"));
        cx.notify();
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = fetch_update().await;
                let _ = app.update(&mut cx, |app, cx| {
                    app.update_checking = false;
                    match result {
                        Ok(Some(info)) => {
                            app.update_status = Some(
                                app.text("update.found")
                                    .replace("{{version}}", &info.version),
                            );
                            app.update_info = Some(info);
                        }
                        Ok(None) => {
                            app.update_info = None;
                            app.update_status = Some(app.text("update.upToDate"));
                        }
                        Err(error) => {
                            app.update_info = None;
                            app.update_status =
                                Some(format!("{}: {error}", app.text("update.failed")));
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Opens the newest release asset after the manifest check finds a newer version.
    fn open_update_or_check(&mut self, cx: &mut Context<Self>) {
        if let Some(info) = &self.update_info {
            cx.open_url(&info.download_url);
            self.status = self.text("update.installed");
        } else if !self.update_checking {
            self.check_for_updates(cx);
        }
        cx.notify();
    }

    /// Opens the original project's sponsorship page without coupling the core transfer crate to it.
    fn open_sponsor_page(&mut self, cx: &mut Context<Self>) {
        cx.open_url("https://buymeacoffee.com/bruceblink");
        self.status = self.status_text("donate", "Donation page opened");
        cx.notify();
    }

    fn start_sharing(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_path.clone() else {
            self.error = Some(self.text("sender.dropFilesHere"));
            cx.notify();
            return;
        };
        if self.send_phase != TransferPhase::Idle {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.send_phase = TransferPhase::Preparing;
        self.transfer_started_at = Some(Instant::now());
        self.status = self.status_text("preparing", "Preparing encrypted share...");
        self.error = None;
        let events = self.event_sender.clone();
        let task = tokio::spawn(transfer::start_send(path, events, generation));
        self.send_abort = Some(task.abort_handle());
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = match task.await {
                    Ok(result) => result.map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                };
                let _ = app.update(&mut cx, |app, cx| {
                    if app.generation == generation {
                        app.apply_send_ready(result, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn apply_send_ready(&mut self, result: Result<SendResult, String>, cx: &mut Context<Self>) {
        match result {
            Ok(result) => {
                self.send_abort = None;
                self.ticket = Some(result.ticket.to_string());
                self.send_result = Some(result);
                self.send_phase = TransferPhase::Sharing;
                self.status = self.status_text("listening", "Listening for a receiver");
            }
            Err(error) => {
                self.send_abort = None;
                self.send_phase = TransferPhase::Idle;
                self.error = Some(error);
                self.status = self.status_text("transfer_failed", "Sharing failed");
            }
        }
        cx.notify();
    }

    fn stop_sharing(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        let Some(result) = self.send_result.take() else {
            self.send_phase = TransferPhase::Idle;
            self.transfer_started_at = None;
            self.status = self.status_text("ok", "Ready");
            cx.notify();
            return;
        };
        self.send_phase = TransferPhase::Stopping;
        self.status = self.status_text("stopping", "Stopping share...");
        let task =
            tokio::spawn(async move { result.shutdown().await.map_err(|error| error.to_string()) });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let outcome = match task.await {
                    Ok(outcome) => outcome,
                    Err(error) => Err(error.to_string()),
                };
                let _ = app.update(&mut cx, |app, cx| {
                    if app.generation == generation {
                        app.send_phase = TransferPhase::Idle;
                        app.ticket = None;
                        app.transfer_started_at = None;
                        app.status = app.status_text("ok", "Ready");
                        if let Err(error) = outcome {
                            app.error = Some(error);
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn copy_ticket(&mut self, cx: &mut Context<Self>) {
        if let Some(ticket) = self.ticket.clone() {
            cx.write_to_clipboard(ticket.into());
            self.status = self.status_text("ticket_copied", "Ticket copied to clipboard");
            cx.notify();
        }
    }

    fn paste_ticket(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.ticket_input = text;
            self.ticket_selection = self.ticket_input.len()..self.ticket_input.len();
            cx.notify();
        }
    }

    fn start_receiving(&mut self, cx: &mut Context<Self>) {
        let ticket = self.ticket_input.trim().to_owned();
        if ticket.is_empty() {
            self.error = Some(self.text("receiver.pasteTicket"));
            cx.notify();
            return;
        }
        if self.receive_phase != TransferPhase::Idle {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.receive_phase = TransferPhase::Connecting;
        self.transfer_started_at = Some(Instant::now());
        self.status = self.status_text("connecting", "Connecting to sender...");
        self.error = None;
        let task = tokio::spawn(transfer::start_receive(
            ticket,
            self.save_path.clone(),
            self.event_sender.clone(),
            generation,
        ));
        self.receive_abort = Some(task.abort_handle());
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = match task.await {
                    Ok(Ok(result)) => Ok(result),
                    Ok(Err(error)) => Err(error.to_string()),
                    Err(error) => Err(error.to_string()),
                };
                let _ = app.update(&mut cx, |app, cx| {
                    if app.generation == generation {
                        app.receive_abort = None;
                        app.apply_receive_finished(result, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn apply_receive_finished(
        &mut self,
        result: Result<sendmer::ReceiveResult, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(result) => {
                self.receive_phase = TransferPhase::Completed;
                self.completed_name = result
                    .file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string());
                self.completed_path = Some(result.file_path.clone());
                self.completed_size = path_size(&result.file_path);
                self.completed_duration = self
                    .transfer_started_at
                    .unwrap_or(self.started_at)
                    .elapsed();
                self.transfer_started_at = None;
                self.status = self.status_text("download_completed", "Download completed");
            }
            Err(error) => {
                self.receive_phase = TransferPhase::Idle;
                self.transfer_started_at = None;
                self.error = Some(error);
                self.status = self.status_text("receive_failed", "Receive failed");
            }
        }
        cx.notify();
    }

    fn stop_receiving(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if let Some(abort) = self.receive_abort.take() {
            abort.abort();
        }
        self.receive_phase = TransferPhase::Idle;
        self.transfer_started_at = None;
        self.status = self.status_text("ok", "Ready");
        cx.notify();
    }

    /// Aborts receive work and asynchronously releases sender resources when the window closes.
    pub(crate) fn stop_on_exit(&mut self) {
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        if let Some(abort) = self.receive_abort.take() {
            abort.abort();
        }
        if let Some(result) = self.send_result.take() {
            tokio::spawn(async move {
                let _ = result.shutdown().await;
            });
        }
    }

    fn new_transfer(&mut self, cx: &mut Context<Self>) {
        let active = matches!(
            self.send_phase,
            TransferPhase::Preparing
                | TransferPhase::Sharing
                | TransferPhase::Transporting
                | TransferPhase::Stopping
        ) || matches!(
            self.receive_phase,
            TransferPhase::Connecting | TransferPhase::Transporting
        );
        if active {
            self.error = Some(self.text("transfer.stopped"));
            cx.notify();
            return;
        }
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        if let Some(result) = self.send_result.take() {
            tokio::spawn(async move {
                let _ = result.shutdown().await;
            });
        }
        self.generation += 1;
        self.send_phase = TransferPhase::Idle;
        self.receive_phase = TransferPhase::Idle;
        self.selected_path = None;
        self.ticket = None;
        self.ticket_input.clear();
        self.receive_files.clear();
        self.completed_name = None;
        self.completed_path = None;
        self.completed_size = 0;
        self.completed_duration = Duration::ZERO;
        self.transfer_started_at = None;
        self.error = None;
        self.status = self.status_text("ok", "Ready");
        cx.notify();
    }

    fn on_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        if let Some(path) = paths.paths().first() {
            self.select_path(path.clone(), cx);
        }
    }

    fn apply_event(&mut self, event: TransferEvent, cx: &mut Context<Self>) {
        match event {
            TransferEvent::Started { role: Role::Sender } => {
                self.send_phase = TransferPhase::Sharing;
                self.transfer_started_at = Some(Instant::now());
                self.status = self.status_text("listening", "Listening for a receiver");
            }
            TransferEvent::Started {
                role: Role::Receiver,
            } => {
                self.receive_phase = TransferPhase::Transporting;
                self.transfer_started_at = Some(Instant::now());
                self.status = self.status_text("downloading", "Downloading in progress");
            }
            TransferEvent::Progress {
                role: Role::Sender,
                processed,
                total,
                speed,
            } => {
                self.send_phase = TransferPhase::Transporting;
                self.send_progress = Progress {
                    processed,
                    total,
                    speed,
                };
                self.status = self.status_text("sharing", "Sharing in progress");
            }
            TransferEvent::Progress {
                role: Role::Receiver,
                processed,
                total,
                speed,
            } => {
                self.receive_phase = TransferPhase::Transporting;
                self.receive_progress = Progress {
                    processed,
                    total,
                    speed,
                };
                self.status = self.status_text("downloading", "Downloading in progress");
            }
            TransferEvent::FileNames {
                role: Role::Receiver,
                file_names,
            } => self.receive_files = file_names,
            TransferEvent::FileNames {
                role: Role::Sender, ..
            } => {}
            TransferEvent::Completed { role: Role::Sender } => {
                self.send_phase = TransferPhase::Completed;
                self.completed_name = self
                    .selected_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().to_string());
                self.completed_path = self.selected_path.clone();
                self.completed_size = self.send_progress.total;
                self.completed_duration = self
                    .transfer_started_at
                    .unwrap_or(self.started_at)
                    .elapsed();
                self.transfer_started_at = None;
                self.status = self.status_text("transfer_completed", "Transfer completed");
            }
            TransferEvent::Completed {
                role: Role::Receiver,
            } => {
                self.receive_phase = TransferPhase::Transporting;
                self.status = self.status_text("finalizing", "Finalizing download");
            }
            TransferEvent::Failed { message, .. } => {
                self.error = Some(message);
                self.send_phase = TransferPhase::Idle;
                self.receive_phase = TransferPhase::Idle;
                self.transfer_started_at = None;
                self.status = self.status_text("transfer_failed", "Transfer failed");
            }
        }
        cx.notify();
    }

    fn tab_button(
        &self,
        tab: Tab,
        label: &'static str,
        colors: Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let active = self.tab == tab;
        div()
            .id(label)
            .role(A11yRole::Tab)
            .aria_label(label)
            .flex_1()
            .h(px(38.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if active {
                colors.background
            } else {
                colors.muted
            })
            .bg(if active {
                colors.accent
            } else {
                colors.panel_alt
            })
            .cursor_pointer()
            .on_click(cx.listener(move |app, _, _, cx| {
                if app.send_phase == TransferPhase::Idle && app.receive_phase == TransferPhase::Idle
                {
                    app.tab = tab;
                    cx.notify();
                }
            }))
            .child(label)
    }

    fn button(
        &self,
        id: &'static str,
        label: impl Into<String>,
        enabled: bool,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let label = label.into();
        div()
            .id(id)
            .role(A11yRole::Button)
            .aria_label(label.clone())
            .h(px(38.0))
            .px_4()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if enabled {
                colors.accent
            } else {
                colors.border
            })
            .bg(if enabled {
                colors.accent
            } else {
                colors.panel_alt
            })
            .text_color(if enabled {
                colors.background
            } else {
                colors.muted
            })
            .font_weight(FontWeight::SEMIBOLD)
            .text_sm()
            .cursor_pointer()
            .when(enabled, |button| {
                button.on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
            })
            .child(label)
    }

    fn render_send(&mut self, colors: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .selected_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| self.status_text("receiver.noFolderSelected", "No file selected"));
        let ready = self.send_phase == TransferPhase::Idle;
        let completed = self.send_phase == TransferPhase::Completed;
        let sharing = matches!(
            self.send_phase,
            TransferPhase::Sharing | TransferPhase::Transporting
        );
        let progress = self.send_progress;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.copy("send")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(self.text("sender.subtitle")),
            )
            .child(
                div()
                    .id("send-drop-zone")
                    .role(A11yRole::Button)
                    .aria_label(self.copy("drop"))
                    .h(px(190.0))
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.panel)
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _, _, cx| app.choose_send_path(cx)))
                    .on_drop::<ExternalPaths>(cx.listener(move |app, paths, _, cx| {
                        app.on_drop(paths, cx);
                    }))
                    .child(div().text_3xl().text_color(colors.accent).child("+"))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.copy("drop")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(self.text("sender.orBrowse")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .truncate()
                            .max_w(px(660.0))
                            .child(selected),
                    ),
            )
            .when(!sharing && !completed, |view| {
                view.child(self.button(
                    "send-start",
                    self.copy("start"),
                    ready && self.selected_path.is_some(),
                    colors,
                    cx,
                    |app, cx| app.start_sharing(cx),
                ))
            })
            .when(sharing, |view| {
                view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                            if progress.total > 0 {
                                format!(
                                    "{:.1}%  {:.1} MB/s",
                                    progress.percentage(),
                                    progress.speed / 1_000_000.0
                                )
                            } else {
                                self.status.clone()
                            },
                        ))
                        .child(progress_bar(progress, colors))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(self.button(
                                    "send-copy",
                                    self.copy("copy"),
                                    self.ticket.is_some(),
                                    colors,
                                    cx,
                                    |app, cx| app.copy_ticket(cx),
                                ))
                                .child(self.button(
                                    "send-stop",
                                    self.copy("stop"),
                                    true,
                                    colors,
                                    cx,
                                    |app, cx| app.stop_sharing(cx),
                                )),
                        ),
                )
            })
            .when(completed, |view| {
                view.child(completion_card(self, colors, cx))
            })
            .when(self.ticket.is_some() && !completed, |view| {
                view.child(ticket_card(
                    self.ticket.as_deref().unwrap_or_default(),
                    colors,
                    self.copy("copy"),
                    self.text("sender.sendThisTicket"),
                    cx,
                    |app, cx| app.copy_ticket(cx),
                ))
            })
            .into_any_element()
    }

    fn render_receive(&mut self, colors: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let receiving = self.receive_phase != TransferPhase::Idle;
        let completed = self.receive_phase == TransferPhase::Completed;
        let save = self.save_path.display().to_string();
        let progress = self.receive_progress;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.copy("receive")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(self.text("receiver.subtitle")),
            )
            .when(!receiving, |view| {
                view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.text("receiver.pasteTicket")),
                        )
                        .child(ticket_input(self, colors, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(colors.muted)
                                .truncate()
                                .child(format!("{}: {}", self.copy("save"), save)),
                        )
                        .child(self.button(
                            "choose-save",
                            self.text("browse"),
                            true,
                            colors,
                            cx,
                            |app, cx| app.choose_save_path(cx),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.button(
                            "paste-ticket",
                            self.text("receiver.pasteTicket"),
                            true,
                            colors,
                            cx,
                            |app, cx| app.paste_ticket(cx),
                        ))
                        .child(self.button(
                            "receive-start",
                            self.copy("receive_action"),
                            !self.ticket_input.trim().is_empty(),
                            colors,
                            cx,
                            |app, cx| app.start_receiving(cx),
                        )),
                )
            })
            .when(receiving && !completed, |view| {
                view.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.status.clone()),
                )
                .child(progress_bar(progress, colors))
                .child(self.button(
                    "receive-stop",
                    self.text("receiver.stopReceiving"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.stop_receiving(cx),
                ))
            })
            .when(completed, |view| {
                view.child(completion_card(self, colors, cx))
            })
            .into_any_element()
    }
}

async fn pump_events(
    entity: gpui::WeakEntity<AlterSendmeApp>,
    receiver: Receiver<(u64, TransferEvent)>,
    cx: &mut AsyncApp,
) {
    while let Ok((generation, event)) = receiver.recv().await {
        let _ = entity.update(cx, |app, cx| {
            if app.generation == generation {
                app.apply_event(event, cx);
            }
        });
    }
}

impl Render for AlterSendmeApp {
    /// Renders the complete send/receive workspace with a stable, bounded content column.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.colors(_window.appearance());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "v" && event.keystroke.modifiers.platform {
                    app.paste_ticket(cx);
                }
            }))
            .child(
                div()
                    .h(px(64.0))
                    .px_6()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(32.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(colors.accent)
                                    .text_color(colors.background)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("A"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("AlterSendme"),
                                    )
                                    .child(div().text_xs().text_color(colors.muted).child(
                                        format!(
                                            "Native GPUI workspace · {} ms",
                                            self.started_at.elapsed().as_millis()
                                        ),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("theme-toggle")
                                    .role(A11yRole::Button)
                                    .aria_label("Theme")
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(colors.panel_alt)
                                    .text_sm()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.theme = match app.theme {
                                            Theme::System => Theme::Dark,
                                            Theme::Dark => Theme::Light,
                                            Theme::Light => Theme::System,
                                        };
                                        cx.notify();
                                    }))
                                    .child(format!(
                                        "{}: {}",
                                        self.copy("theme"),
                                        match self.theme {
                                            Theme::System => self.text("theme.system"),
                                            Theme::Dark => self.text("theme.dark"),
                                            Theme::Light => self.text("theme.light"),
                                        }
                                    )),
                            )
                            .child(
                                div()
                                    .id("locale-toggle")
                                    .role(A11yRole::Button)
                                    .aria_label("Language")
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(colors.panel_alt)
                                    .text_sm()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        let locales = Locale::all();
                                        let index = locales
                                            .iter()
                                            .position(|locale| *locale == app.locale)
                                            .unwrap_or(0);
                                        app.locale = locales[(index + 1) % locales.len()];
                                        cx.notify();
                                    }))
                                    .child(format!(
                                        "{}: {}",
                                        self.copy("language"),
                                        self.locale.label()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .id("content-scroll")
                            .overflow_y_scroll()
                            .p_6()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(720.0))
                                    .flex()
                                    .flex_col()
                                    .gap_4()
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .p_1()
                                            .rounded_md()
                                            .bg(colors.panel_alt)
                                            .child(self.tab_button(
                                                Tab::Send,
                                                self.copy("send"),
                                                colors,
                                                cx,
                                            ))
                                            .child(self.tab_button(
                                                Tab::Receive,
                                                self.copy("receive"),
                                                colors,
                                                cx,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(colors.border)
                                            .bg(colors.panel)
                                            .p_6()
                                            .flex()
                                            .flex_col()
                                            .gap_4()
                                            .when(self.tab == Tab::Send, |view| {
                                                view.child(self.render_send(colors, cx))
                                            })
                                            .when(self.tab == Tab::Receive, |view| {
                                                view.child(self.render_receive(colors, cx))
                                            })
                                            .when(self.error.is_some(), |view| {
                                                view.child(
                                                    div()
                                                        .rounded_md()
                                                        .border_1()
                                                        .border_color(colors.danger)
                                                        .bg(colors.danger.alpha(0.12))
                                                        .p_3()
                                                        .text_sm()
                                                        .text_color(colors.danger)
                                                        .child(
                                                            self.error.clone().unwrap_or_default(),
                                                        ),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(colors.muted)
                                                    .child(self.status.clone()),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(46.0))
                            .px_6()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_t_1()
                            .border_color(colors.border)
                            .text_xs()
                            .text_color(colors.muted)
                            .child(self.text("appSubtitle"))
                            .child(
                                div()
                                    .flex_1()
                                    .px_3()
                                    .text_center()
                                    .truncate()
                                    .child(self.update_status.clone().unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("footer-new")
                                            .role(A11yRole::Button)
                                            .aria_label(self.copy("new"))
                                            .cursor_pointer()
                                            .on_click(
                                                cx.listener(|app, _, _, cx| app.new_transfer(cx)),
                                            )
                                            .child(self.copy("new")),
                                    )
                                    .child(
                                        div()
                                            .id("footer-sponsor")
                                            .role(A11yRole::Button)
                                            .aria_label("Donate")
                                            .cursor_pointer()
                                            .on_click(cx.listener(|app, _, _, cx| {
                                                app.open_sponsor_page(cx)
                                            }))
                                            .child(self.text("donate")),
                                    )
                                    .child(
                                        div()
                                            .id("footer-update")
                                            .role(A11yRole::Button)
                                            .aria_label("Check for updates")
                                            .cursor_pointer()
                                            .on_click(cx.listener(|app, _, _, cx| {
                                                app.open_update_or_check(cx)
                                            }))
                                            .child(if self.update_checking {
                                                self.text("update.checking")
                                            } else if self.update_info.is_some() {
                                                self.text("update.found")
                                            } else {
                                                self.text("update.checkNow")
                                            }),
                                    )
                                    .child(div().child(format!("v{}", env!("CARGO_PKG_VERSION")))),
                            ),
                    ),
            )
    }
}

impl EntityInputHandler for AlterSendmeApp {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = utf16_to_byte_range(&self.ticket_input, range);
        *adjusted = Some(byte_to_utf16_range(&self.ticket_input, range.clone()));
        Some(self.ticket_input.get(range)?.to_owned())
    }
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: byte_to_utf16_range(&self.ticket_input, self.ticket_selection.clone()),
            reversed: false,
        })
    }
    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }
    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_ticket(range, text);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_ticket(range, text);
        if let Some(selected) = selected {
            self.ticket_selection = utf16_to_byte_range(&self.ticket_input, selected);
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.ticket_input.encode_utf16().count())
    }
    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        true
    }
}

impl AlterSendmeApp {
    fn replace_ticket(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = range
            .map(|range| utf16_to_byte_range(&self.ticket_input, range))
            .unwrap_or_else(|| self.ticket_selection.clone());
        let start = range.start.min(self.ticket_input.len());
        let end = range.end.min(self.ticket_input.len());
        if start <= end && self.ticket_input.len() - (end - start) + text.len() <= MAX_TICKET_LEN {
            self.ticket_input.replace_range(start..end, text);
            self.ticket_selection = start + text.len()..start + text.len();
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    background: Rgba,
    panel: Rgba,
    panel_alt: Rgba,
    text: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
    accent_alt: Rgba,
    danger: Rgba,
}

fn progress_bar(progress: Progress, colors: Palette) -> gpui::Div {
    div()
        .h(px(8.0))
        .w_full()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(
            div()
                .h(px(8.0))
                .w(px(progress.percentage().clamp(0.0, 100.0) * 6.4))
                .rounded_md()
                .bg(colors.accent_alt),
        )
}

async fn fetch_update() -> anyhow::Result<Option<UpdateInfo>> {
    let client = reqwest::Client::new();
    let manifest_response = client
        .get(UPDATE_MANIFEST_URL)
        .header("User-Agent", "AlterSendme-GPUI")
        .send()
        .await?;
    let (version, download_url) = if manifest_response.status().is_success() {
        let manifest = manifest_response.json::<UpdateManifest>().await?;
        let platform = current_platform_key();
        let download_url = manifest
            .platforms
            .get(platform)
            .and_then(|value| value.get("url"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| RELEASES_URL.to_owned());
        (manifest.version, download_url)
    } else {
        let release = client
            .get(GITHUB_RELEASE_URL)
            .header("User-Agent", "AlterSendme-GPUI")
            .send()
            .await?
            .error_for_status()?
            .json::<GitHubRelease>()
            .await?;
        let version = release.tag_name.trim_start_matches('v').to_owned();
        let suffix = current_asset_suffix();
        let download_url = release
            .assets
            .iter()
            .find(|asset| asset.name.contains(suffix))
            .map(|asset| asset.browser_download_url.clone())
            .unwrap_or_else(|| RELEASES_URL.to_owned());
        (version, download_url)
    };
    if !is_newer_version(env!("CARGO_PKG_VERSION"), &version) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version,
        download_url,
    }))
}

fn current_platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x86_64-nsis"
    } else if cfg!(target_os = "macos") {
        "darwin-universal"
    } else {
        "linux-x86_64"
    }
}

fn current_asset_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        "x64-setup.exe"
    } else if cfg!(target_os = "macos") {
        ".dmg"
    } else {
        ".AppImage"
    }
}

fn is_newer_version(current: &str, candidate: &str) -> bool {
    let parse = |version: &str| {
        let mut parts = [0_u64; 3];
        for (index, part) in version
            .trim_start_matches('v')
            .split('.')
            .take(3)
            .enumerate()
        {
            parts[index] = part.parse().unwrap_or(0);
        }
        parts
    };
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod update_tests {
    use super::is_newer_version;

    #[test]
    fn compares_semver_like_versions() {
        assert!(is_newer_version("0.2.0", "0.2.1"));
        assert!(is_newer_version("0.2.0", "1.0.0"));
        assert!(!is_newer_version("0.2.0", "0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.1.99"));
        assert!(is_newer_version("v0.2.0", "v0.3.0"));
    }
}

fn ticket_card(
    ticket: &str,
    colors: Palette,
    copy_label: &'static str,
    ticket_hint: String,
    cx: &mut Context<AlterSendmeApp>,
    action: impl Fn(&mut AlterSendmeApp, &mut Context<AlterSendmeApp>) + 'static,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(div().text_xs().text_color(colors.muted).child(ticket_hint))
        .child(
            div()
                .text_xs()
                .text_color(colors.text)
                .truncate()
                .child(ticket.to_owned()),
        )
        .child(
            div()
                .id("copy-ticket-secondary")
                .h(px(32.0))
                .px_3()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .text_sm()
                .cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
                .child(copy_label),
        )
}

fn completion_card(
    app: &AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.accent)
                .child(app.text("transfer_complete")),
        )
        .child(
            div().text_sm().child(
                app.completed_name
                    .clone()
                    .unwrap_or_else(|| app.text("transfer.file")),
            ),
        )
        .child(div().text_sm().text_color(colors.muted).child(format!(
            "{} bytes · {:.1}s",
            app.completed_size,
            app.completed_duration.as_secs_f32()
        )))
        .child(app.button(
            "receive-reveal",
            app.text("open_folder"),
            app.completed_path.is_some(),
            colors,
            cx,
            |app, cx| app.reveal_completed_path(cx),
        ))
        .child(app.button(
            "receive-new",
            app.copy("new"),
            true,
            colors,
            cx,
            |app, cx| app.new_transfer(cx),
        ))
}

fn ticket_input(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui::Stateful<gpui::Div> {
    let entity = cx.entity();
    let focus = app.ticket_focus.clone();
    let click_focus = focus.clone();
    div()
        .id("ticket-input")
        .role(A11yRole::TextInput)
        .aria_label("Receive ticket")
        .relative()
        .h(px(78.0))
        .w_full()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .cursor_pointer()
        .on_click(cx.listener(move |_, _, window, cx| click_focus.focus(window, cx)))
        .child(gpui::canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
            },
        ))
        .child(
            div()
                .text_xs()
                .text_color(if app.ticket_input.is_empty() {
                    colors.muted
                } else {
                    colors.text
                })
                .child(if app.ticket_input.is_empty() {
                    "sendme receive ticket...".to_owned()
                } else {
                    app.ticket_input.clone()
                }),
        )
}

fn path_size(path: &PathBuf) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    if path.is_dir() {
        return walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| entry.metadata().ok())
            .map(|meta| meta.len())
            .sum();
    }
    0
}

fn utf16_to_byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    let offset = |target: usize| {
        let mut units = 0;
        for (index, ch) in text.char_indices() {
            if units >= target {
                return index;
            }
            units += ch.len_utf16();
        }
        text.len()
    };
    offset(range.start)..offset(range.end)
}
fn byte_to_utf16_range(text: &str, range: Range<usize>) -> Range<usize> {
    text[..range.start].encode_utf16().count()..text[..range.end].encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_never_exceeds_one_hundred_percent() {
        assert_eq!(
            Progress {
                processed: 20,
                total: 10,
                speed: 0.0
            }
            .percentage(),
            100.0
        );
    }
    #[test]
    fn utf16_ranges_handle_non_bmp_ticket_text() {
        let text = "a😀b";
        assert_eq!(utf16_to_byte_range(text, 1..3), 1..5);
        assert_eq!(byte_to_utf16_range(text, 1..5), 1..3);
    }
}
