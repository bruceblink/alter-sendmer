//! GPUI workbench and its state machine.

use crate::locale::Locale;
use crate::transfer;
use async_channel::{Receiver, Sender};
use gpui::prelude::*;
use gpui::{
    AsyncApp, Bounds, Context, ElementInputHandler, EntityInputHandler, ExternalPaths, FocusHandle,
    FontWeight, KeyDownEvent, PathPromptOptions, Render, Rgba, Role as A11yRole, UTF16Selection,
    Window, WindowAppearance, div, px, relative, rgb,
};
use sendmer::{Role, SendResult, TransferEvent};
use serde::{Deserialize, Serialize};
use std::{
    fs,
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
    Failed,
}

impl TransferPhase {
    fn is_active(self) -> bool {
        matches!(
            self,
            Self::Preparing
                | Self::Sharing
                | Self::Connecting
                | Self::Transporting
                | Self::Stopping
        )
    }
}

fn state_label(phase: TransferPhase, locale: Locale) -> String {
    let key = match phase {
        TransferPhase::Idle => "state.idle",
        TransferPhase::Preparing => "state.preparing",
        TransferPhase::Sharing => "state.listening",
        TransferPhase::Connecting => "state.preparing",
        TransferPhase::Transporting => "state.active",
        TransferPhase::Stopping => "state.stopping",
        TransferPhase::Completed => "state.completed",
        TransferPhase::Failed => "state.failed",
    };
    locale
        .lookup(key)
        .or_else(|| Locale::English.lookup(key))
        .unwrap_or(key)
        .to_owned()
}

#[derive(Clone, Debug, Default)]
struct Progress {
    processed: u64,
    total: u64,
    speed: f64,
    current_name: Option<String>,
    completed_files: u32,
    total_files: u32,
}

impl Progress {
    fn percentage(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            ((self.processed as f32 / self.total as f32) * 100.0).clamp(0.0, 100.0)
        }
    }

    fn files_label(&self, unit: &str) -> String {
        if self.total_files == 0 {
            return String::new();
        }
        format!("{}/{} {unit}", self.completed_files, self.total_files)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct HistoryEntry {
    role: String,
    path: String,
    size: u64,
    duration_ms: u128,
    speed: u64,
    date: String,
    outcome: String,
    ticket: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Preferences {
    relay: String,
    retry_limit: u32,
    download_limit_mb: u32,
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
    receive_cancel: Option<tokio::sync::watch::Sender<bool>>,
    receive_done: Option<tokio::sync::oneshot::Receiver<()>>,
    receive_task_generation: Option<u64>,
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
    root_focus: FocusHandle,
    ticket_focus: FocusHandle,
    generation: u64,
    update_checking: bool,
    update_info: Option<UpdateInfo>,
    update_status: Option<String>,
    history: Vec<HistoryEntry>,
    history_open: bool,
    preferences: Preferences,
    diagnostics: Option<String>,
    receiver_help_open: bool,
    completed_stopped: bool,
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
    pub fn new(started_at: Instant, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (event_sender, event_receiver) = async_channel::unbounded();
        let save_path = directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let root_focus = cx.focus_handle();
        window.focus(&root_focus, cx);
        let app = Self {
            started_at,
            tab: Tab::Send,
            theme: load_theme(),
            locale: load_locale(),
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
            receive_cancel: None,
            receive_done: None,
            receive_task_generation: None,
            send_progress: Progress::default(),
            receive_progress: Progress::default(),
            receive_files: Vec::new(),
            status: load_locale().lookup("ready").unwrap_or("Ready").to_owned(),
            error: None,
            completed_name: None,
            completed_path: None,
            completed_size: 0,
            completed_duration: Duration::ZERO,
            transfer_started_at: None,
            event_sender,
            root_focus,
            ticket_focus: cx.focus_handle(),
            generation: 0,
            update_checking: false,
            update_info: None,
            update_status: None,
            history: load_history(),
            history_open: false,
            preferences: load_preferences(),
            diagnostics: None,
            receiver_help_open: false,
            completed_stopped: false,
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

    fn theme_label(&self) -> String {
        match self.theme {
            Theme::System => self.text("theme.system"),
            Theme::Dark => self.text("theme.dark"),
            Theme::Light => self.text("theme.light"),
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

    /// Reports whether the sender completion card represents an interrupted transfer.
    fn is_send_stopped(&self) -> bool {
        self.completed_stopped
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
        let prompt = self.text("sender.browseFile");
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
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
        let prompt = self.text("receiver.saveToFolder");
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
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
            let reveal_target = if path.is_dir() {
                path.clone()
            } else {
                path.parent().unwrap_or(path).to_path_buf()
            };
            cx.reveal_path(&reveal_target);
            self.status = self.status_text("download_completed", "Opened folder");
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

    /// Shows a local diagnostic summary so support can identify the app, relay, and storage setup.
    fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diagnostics = Some(format!(
            "{} {} | relay={} | history={} | sendmer=0.6",
            self.text("diagnostics.client"),
            env!("CARGO_PKG_VERSION"),
            self.preferences.relay,
            self.history.len()
        ));
        self.status = self.text("diagnostics.completed");
        cx.notify();
    }

    fn save_ticket(&mut self, cx: &mut Context<Self>) {
        let Some(ticket) = self.ticket.clone() else {
            return;
        };
        let path = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| format!("{}.sendme.ticket", name.to_string_lossy()))
            .unwrap_or_else(|| "alter-sendme.ticket".to_owned());
        let destination = self
            .selected_path
            .as_ref()
            .and_then(|source| source.parent())
            .unwrap_or(&self.save_path)
            .join(path);
        match fs::write(&destination, ticket) {
            Ok(()) => self.status = self.text("sender.ticketSaved"),
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn open_shared_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.selected_path.clone() {
            let target = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(&path).to_path_buf()
            };
            cx.reveal_path(&target);
            self.status = self.text("sender.folderOpened");
            cx.notify();
        }
    }

    #[allow(dead_code)]
    fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.history.clear();
        persist_history(&self.history);
        self.status = self.text("history.cleared");
        cx.notify();
    }

    fn show_history(&mut self, cx: &mut Context<Self>) {
        self.history_open = !self.history_open;
        self.status = if self.history.is_empty() {
            self.text("history.empty")
        } else {
            format!("{}: {}", self.text("history.title"), self.history.len())
        };
        cx.notify();
    }

    fn copy_history_ticket(&mut self, ticket: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ticket.into());
        self.status = self.text("ticket_copied");
        cx.notify();
    }

    fn cycle_relay(&mut self, cx: &mut Context<Self>) {
        self.preferences.relay = if self.preferences.relay == "default" {
            "disabled".to_owned()
        } else {
            "default".to_owned()
        };
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.relay"))
            .replace("{{value}}", &self.preferences.relay);
        cx.notify();
    }

    /// Advances through the bundled locale list and persists the selection for the next launch.
    fn cycle_locale(&mut self, cx: &mut Context<Self>) {
        let locales = Locale::all();
        let index = locales
            .iter()
            .position(|locale| *locale == self.locale)
            .unwrap_or(0);
        self.locale = locales[(index + 1) % locales.len()];
        persist_locale(self.locale);
        self.status = self.text("ready");
        cx.notify();
    }

    /// Toggles the receiver instructions without leaving the active transfer surface.
    fn toggle_receiver_help(&mut self, cx: &mut Context<Self>) {
        self.receiver_help_open = !self.receiver_help_open;
        cx.notify();
    }

    fn cycle_retry_limit(&mut self, cx: &mut Context<Self>) {
        self.preferences.retry_limit = match self.preferences.retry_limit {
            1 => 3,
            3 => 5,
            _ => 1,
        };
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.retry"))
            .replace("{{value}}", &self.preferences.retry_limit.to_string());
        cx.notify();
    }

    fn cycle_download_chunk(&mut self, cx: &mut Context<Self>) {
        self.preferences.download_limit_mb = match self.preferences.download_limit_mb {
            8 => 32,
            32 => 64,
            _ => 8,
        };
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.chunk"))
            .replace(
                "{{value}}",
                &format!("{} MB", self.preferences.download_limit_mb),
            );
        cx.notify();
    }

    /// Records one finished or failed transfer while avoiding ticket/file contents beyond the
    /// sender ticket that is needed for history replay.
    fn record_history(
        &mut self,
        role: &str,
        path: Option<&PathBuf>,
        outcome: &str,
        size_override: Option<u64>,
    ) {
        let duration = self
            .transfer_started_at
            .map(|started| started.elapsed())
            .unwrap_or_default();
        let size = size_override.unwrap_or_else(|| path.map(path_size).unwrap_or(0));
        let speed = if duration.is_zero() {
            0
        } else {
            (size as f64 / duration.as_secs_f64()) as u64
        };
        self.history.push(HistoryEntry {
            role: role.to_owned(),
            path: path
                .map(|value| value.display().to_string())
                .unwrap_or_default(),
            size,
            duration_ms: duration.as_millis(),
            speed,
            date: unix_timestamp().to_string(),
            outcome: outcome.to_owned(),
            ticket: if role == "sender" {
                self.ticket.clone()
            } else {
                None
            },
        });
        persist_history(&self.history);
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
        let relay_mode = if self.preferences.relay == "disabled" {
            sendmer::RelayModeOption::Disabled
        } else {
            sendmer::RelayModeOption::Default
        };
        let task = tokio::spawn(transfer::start_send(path, events, generation, relay_mode));
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
                self.send_phase = TransferPhase::Failed;
                self.error = Some(error);
                self.status = self.status_text("transfer_failed", "Sharing failed");
            }
        }
        cx.notify();
    }

    fn stop_sharing(&mut self, cx: &mut Context<Self>) {
        // Snapshot transfer metadata before shutdown clears the live send result and ticket.
        let was_transporting = self.send_phase == TransferPhase::Transporting;
        let stopped_size = self.send_progress.processed;
        let stopped_name = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string());
        let stopped_path = self.selected_path.clone();
        let stopped_duration = self
            .transfer_started_at
            .map(|started| started.elapsed())
            .unwrap_or_default();
        self.generation += 1;
        let generation = self.generation;
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        let Some(result) = self.send_result.take() else {
            if was_transporting {
                self.send_phase = TransferPhase::Completed;
                self.completed_stopped = true;
                self.completed_name = stopped_name;
                self.completed_path = stopped_path.clone();
                self.completed_size = stopped_size;
                self.completed_duration = stopped_duration;
                self.record_history(
                    "sender",
                    stopped_path.as_ref(),
                    "stopped",
                    Some(stopped_size),
                );
                self.ticket = None;
                self.transfer_started_at = None;
                self.status = self.status_text("stopped", "Transmission stopped");
                cx.notify();
                return;
            }
            self.send_phase = TransferPhase::Idle;
            self.transfer_started_at = None;
            self.status = self.status_text("ready", "Ready");
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
                        if was_transporting {
                            app.send_phase = TransferPhase::Completed;
                            app.completed_stopped = true;
                            app.completed_name = stopped_name;
                            app.completed_path = stopped_path.clone();
                            app.completed_size = stopped_size;
                            app.completed_duration = stopped_duration;
                            app.record_history(
                                "sender",
                                stopped_path.as_ref(),
                                "stopped",
                                Some(stopped_size),
                            );
                            app.ticket = None;
                            app.transfer_started_at = None;
                            app.status = app.status_text("stopped", "Transmission stopped");
                        } else {
                            app.send_phase = TransferPhase::Idle;
                            app.ticket = None;
                            app.transfer_started_at = None;
                            app.status = app.status_text("ready", "Ready");
                        }
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

    fn retry_send(&mut self, cx: &mut Context<Self>) {
        self.send_phase = TransferPhase::Idle;
        self.completed_stopped = false;
        self.error = None;
        self.send_progress = Progress::default();
        self.start_sharing(cx);
    }

    fn retry_receive(&mut self, cx: &mut Context<Self>) {
        self.receive_phase = TransferPhase::Idle;
        self.completed_stopped = false;
        self.error = None;
        self.receive_progress = Progress::default();
        self.start_receiving(cx);
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
            self.error = Some(self.text("receiver.ticketPlaceholder"));
            cx.notify();
            return;
        }
        if !ticket_is_valid(&ticket) {
            self.error = Some(self.text("receiver.invalidTicket"));
            self.receive_phase = TransferPhase::Failed;
            cx.notify();
            return;
        }
        if self.receive_phase != TransferPhase::Idle || self.receive_done.is_some() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.receive_phase = TransferPhase::Connecting;
        self.transfer_started_at = Some(Instant::now());
        self.status = self.status_text("connecting", "Connecting to sender...");
        self.error = None;
        let (cancel_sender, cancel_receiver) = tokio::sync::watch::channel(false);
        let (done_sender, done_receiver) = tokio::sync::oneshot::channel();
        let output_dir = self.save_path.clone();
        let events = self.event_sender.clone();
        let relay_mode = if self.preferences.relay == "disabled" {
            sendmer::RelayModeOption::Disabled
        } else {
            sendmer::RelayModeOption::Default
        };
        let retry_policy = sendmer::core::options::ReceiveRetryPolicy {
            download_retry_limit: self.preferences.retry_limit,
            size_fetch_retry_limit: self.preferences.retry_limit,
            size_fetch_chunk_size: self.preferences.download_limit_mb as u64 * 1024 * 1024,
            ..Default::default()
        };
        let task = tokio::spawn(async move {
            let result = transfer::start_receive(
                ticket,
                output_dir,
                events,
                generation,
                relay_mode,
                retry_policy,
                cancel_receiver,
            )
            .await;
            let _ = done_sender.send(());
            result
        });
        self.receive_cancel = Some(cancel_sender);
        self.receive_done = Some(done_receiver);
        self.receive_task_generation = Some(generation);
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
                    let owns_task = app.receive_task_generation == Some(generation);
                    let current_generation = app.generation == generation;
                    if owns_task {
                        app.receive_cancel = None;
                        app.receive_done = None;
                        app.receive_task_generation = None;
                    }
                    if current_generation {
                        app.apply_receive_finished(result, cx);
                    } else if owns_task && app.receive_phase == TransferPhase::Stopping {
                        app.receive_phase = TransferPhase::Idle;
                        app.status = app.status_text("ready", "Ready");
                        cx.notify();
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
                self.completed_stopped = false;
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
                let path = Some(result.file_path.clone());
                self.record_history(
                    "receiver",
                    path.as_ref(),
                    "completed",
                    Some(self.receive_progress.total.max(self.completed_size)),
                );
                self.transfer_started_at = None;
                self.status = self.status_text("download_completed", "Download completed");
            }
            Err(error) => {
                self.receive_phase = TransferPhase::Failed;
                let path = self.completed_path.clone();
                self.record_history(
                    "receiver",
                    path.as_ref(),
                    "failed",
                    Some(self.receive_progress.processed),
                );
                self.transfer_started_at = None;
                self.error = Some(error);
                self.status = self.status_text("receive_failed", "Receive failed");
            }
        }
        cx.notify();
    }

    fn stop_receiving(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if let Some(cancel) = self.receive_cancel.take() {
            let _ = cancel.send(true);
        }
        self.receive_phase = TransferPhase::Stopping;
        self.ticket_input.clear();
        self.ticket_selection = 0..0;
        self.receive_progress = Progress::default();
        self.receive_files.clear();
        self.transfer_started_at = None;
        self.status = self.status_text("ready", "Ready");
        cx.notify();
    }

    /// Takes ownership of active transfer resources for the app-quit hook.
    ///
    /// The receive task receives a graceful cancellation request, while both transfer cleanup
    /// handles are returned so the caller can await their ordered resource release on exit.
    pub(crate) fn take_shutdown_resources(
        &mut self,
    ) -> (
        Option<SendResult>,
        Option<tokio::sync::oneshot::Receiver<()>>,
    ) {
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        if let Some(cancel) = self.receive_cancel.take() {
            let _ = cancel.send(true);
        }
        (self.send_result.take(), self.receive_done.take())
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
            TransferPhase::Connecting | TransferPhase::Transporting | TransferPhase::Stopping
        );
        if active {
            self.error = Some(self.text("transfer.wasStopped"));
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
        self.send_progress = Progress::default();
        self.receive_progress = Progress::default();
        self.receive_files.clear();
        self.selected_path = None;
        self.ticket = None;
        self.ticket_input.clear();
        self.receive_files.clear();
        self.completed_name = None;
        self.completed_path = None;
        self.completed_size = 0;
        self.completed_duration = Duration::ZERO;
        self.completed_stopped = false;
        self.receiver_help_open = false;
        self.transfer_started_at = None;
        self.diagnostics = None;
        self.error = None;
        self.status = self.status_text("ready", "Ready");
        cx.notify();
    }

    fn done_transfer(&mut self, cx: &mut Context<Self>) {
        if self.send_phase == TransferPhase::Completed
            || self.receive_phase == TransferPhase::Completed
        {
            self.new_transfer(cx);
        }
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
                    current_name: self.selected_path.as_ref().and_then(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                    }),
                    completed_files: if processed >= total && total > 0 {
                        1
                    } else {
                        0
                    },
                    total_files: 1,
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
                    current_name: self.receive_files.first().cloned(),
                    completed_files: if processed >= total && total > 0 {
                        self.receive_files.len().max(1) as u32
                    } else {
                        0
                    },
                    total_files: self.receive_files.len().max(1) as u32,
                };
                self.status = self.status_text("downloading", "Downloading in progress");
            }
            TransferEvent::FileNames {
                role: Role::Receiver,
                file_names,
            } => {
                self.receive_progress.total_files = file_names.len().max(1) as u32;
                self.receive_files = file_names;
                self.receive_progress.current_name = self.receive_files.first().cloned();
            }
            TransferEvent::FileNames {
                role: Role::Sender, ..
            } => {}
            TransferEvent::Completed { role: Role::Sender } => {
                self.completed_stopped = false;
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
                let path = self.selected_path.clone();
                self.record_history(
                    "sender",
                    path.as_ref(),
                    "completed",
                    Some(self.send_progress.total),
                );
            }
            TransferEvent::Completed {
                role: Role::Receiver,
            } => {
                self.completed_stopped = false;
                self.receive_phase = TransferPhase::Completed;
                self.status = self.status_text("finalizing", "Finalizing download");
            }
            TransferEvent::Failed { role, message } => {
                self.error = Some(message);
                match role {
                    Role::Sender => {
                        self.send_phase = TransferPhase::Failed;
                        let path = self.selected_path.clone();
                        self.record_history(
                            "sender",
                            path.as_ref(),
                            "failed",
                            Some(self.send_progress.processed),
                        );
                    }
                    Role::Receiver => {
                        self.receive_phase = TransferPhase::Failed;
                        let path = self.completed_path.clone();
                        self.record_history(
                            "receiver",
                            path.as_ref(),
                            "failed",
                            Some(self.receive_progress.processed),
                        );
                    }
                }
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
            .focusable()
            .tab_stop(true)
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
                if !tabs_locked(app.send_phase, app.receive_phase) {
                    app.tab = tab;
                    cx.notify();
                }
            }))
            .child(label)
    }

    fn button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<String>,
        enabled: bool,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let id = id.into();
        let label = label.into();
        let destructive = id.to_string().contains("stop");
        div()
            .id(id)
            .focusable()
            .tab_stop(true)
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
                if destructive {
                    colors.danger
                } else {
                    colors.accent
                }
            } else {
                colors.border
            })
            .bg(if enabled {
                if destructive {
                    colors.danger
                } else {
                    colors.accent
                }
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
            .unwrap_or_default();
        let ready = self.send_phase == TransferPhase::Idle;
        let completed = self.send_phase == TransferPhase::Completed;
        let failed = self.send_phase == TransferPhase::Failed;
        let preparing = matches!(
            self.send_phase,
            TransferPhase::Preparing | TransferPhase::Stopping
        );
        let sharing = matches!(
            self.send_phase,
            TransferPhase::Sharing | TransferPhase::Transporting
        );
        let show_drop_zone = self.send_phase == TransferPhase::Idle;
        let progress = self.send_progress.clone();
        let selected_label = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| selected.clone());
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
                div().text_xs().text_color(colors.muted).child(
                    self.text("state.label")
                        .replace("{{value}}", &state_label(self.send_phase, self.locale)),
                ),
            )
            .when(self.selected_path.is_some(), |view| {
                view.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().flex_1().text_sm().truncate().child(format!(
                            "{} {}",
                            self.text("sender.fileLabel"),
                            selected_label
                        )))
                        .child(div().text_xs().text_color(colors.muted).child(
                            if self.selected_is_dir {
                                self.text("transfer.folder")
                            } else {
                                self.text("transfer.file")
                            },
                        )),
                )
            })
            .when(show_drop_zone, |view| {
                view.child(
                    div()
                        .id("send-drop-zone")
                        .focusable()
                        .tab_stop(true)
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
            })
            .when(!sharing && !completed && !failed && !preparing, |view| {
                view.child(self.button(
                    "send-start",
                    self.copy("start"),
                    ready && self.selected_path.is_some(),
                    colors,
                    cx,
                    |app, cx| app.start_sharing(cx),
                ))
            })
            .when(preparing, |view| {
                view.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.status.clone()),
                )
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
                        .child(progress_bar(&progress, colors))
                        .child(
                            div().text_xs().text_color(colors.muted).child(
                                progress
                                    .current_name
                                    .clone()
                                    .unwrap_or_else(|| self.text("transfer.file")),
                            ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(progress.files_label(&self.text("transfer.files"))),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(self.button(
                                    "send-stop",
                                    self.copy("stop"),
                                    true,
                                    colors,
                                    cx,
                                    |app, cx| app.stop_sharing(cx),
                                ))
                                .child(self.button(
                                    "send-save-ticket",
                                    self.text("sender.saveTicket"),
                                    self.ticket.is_some(),
                                    colors,
                                    cx,
                                    |app, cx| app.save_ticket(cx),
                                ))
                                .child(self.button(
                                    "send-open-folder",
                                    self.text("sender.openSharedFolder"),
                                    self.selected_path.is_some(),
                                    colors,
                                    cx,
                                    |app, cx| app.open_shared_folder(cx),
                                )),
                        ),
                )
            })
            .when(failed, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(colors.danger)
                        .child(self.text("transfer.failed")),
                )
                .child(self.button(
                    "send-retry",
                    self.text("transfer.tryAgain"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.retry_send(cx),
                ))
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
        let receiving = self.receive_phase.is_active();
        let completed = self.receive_phase == TransferPhase::Completed;
        let failed = self.receive_phase == TransferPhase::Failed;
        let save = self.save_path.display().to_string();
        let progress = self.receive_progress.clone();
        let receive_name = self
            .receive_files
            .first()
            .cloned()
            .unwrap_or_else(|| self.text("transfer.file"));
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
            .child(
                div().text_xs().text_color(colors.muted).child(
                    self.text("state.label")
                        .replace("{{value}}", &state_label(self.receive_phase, self.locale)),
                ),
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
                            self.receive_phase == TransferPhase::Idle,
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
                .child(self.button(
                    "receiver-help",
                    self.text("receiver.howToReceive"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.toggle_receiver_help(cx),
                ))
                .when(self.receiver_help_open, |view| {
                    view.child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(colors.panel_alt)
                            .text_xs()
                            .text_color(colors.muted)
                            .child(format!(
                                "1. {}\n2. {}\n3. {}\n4. {}\n5. {}",
                                self.text("receiver.instruction1"),
                                self.text("receiver.instruction2"),
                                self.text("receiver.instruction3"),
                                self.text("receiver.instruction4"),
                                self.text("receiver.instruction5")
                            )),
                    )
                })
            })
            .when(receiving && !completed, |view| {
                view.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.status.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .truncate()
                        .child(receive_name.clone()),
                )
                .child(progress_bar(&progress, colors))
                .child(
                    div().text_xs().text_color(colors.muted).child(
                        progress
                            .current_name
                            .clone()
                            .unwrap_or_else(|| receive_name.clone()),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(progress.files_label(&self.text("transfer.files"))),
                )
                .child(self.button(
                    "receive-stop",
                    self.text("receiver.stopReceiving"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.stop_receiving(cx),
                ))
            })
            .when(failed, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(colors.danger)
                        .child(self.text("transfer.failed")),
                )
                .child(self.button(
                    "receive-retry",
                    self.text("transfer.tryAgain"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.retry_receive(cx),
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
            .id("alter-sendme-root")
            .role(A11yRole::Application)
            .aria_label(self.text("appTitle"))
            .track_focus(&self.root_focus)
            .on_action(cx.listener(|_, _: &crate::Tab, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &crate::TabPrev, window, cx| window.focus_prev(cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "v" && event.keystroke.modifiers.platform {
                    app.paste_ticket(cx);
                } else if event.keystroke.key == "enter"
                    && app.tab == Tab::Receive
                    && !event.keystroke.modifiers.shift
                {
                    app.start_receiving(cx);
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
                                            .child(self.text("appTitle")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.muted)
                                            .child(self.text("appSubtitle")),
                                    ),
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
                                    .aria_label(self.copy("theme"))
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
                                        persist_theme(app.theme);
                                        cx.notify();
                                    }))
                                    .child(format!(
                                        "{}: {}",
                                        self.copy("theme"),
                                        self.theme_label()
                                    )),
                            )
                            .child(
                                div()
                                    .id("locale-toggle")
                                    .role(A11yRole::Button)
                                    .aria_label(self.copy("language"))
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(colors.panel_alt)
                                    .text_sm()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|app, _, _, cx| app.cycle_locale(cx)))
                                    .child(format!(
                                        "{}: {}",
                                        self.copy("language"),
                                        self.locale.label()
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex_grow(1.0)
                    .flex_shrink(1.0)
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .flex()
                    .w_full()
                    .id("content-scroll")
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .max_w(px(720.0))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(self.button(
                                        "diagnostics",
                                        self.text("diagnostics.action"),
                                        true,
                                        colors,
                                        cx,
                                        |app, cx| app.run_diagnostics(cx),
                                    ))
                                    .child(self.button(
                                        "history",
                                        self.text("history.title"),
                                        true,
                                        colors,
                                        cx,
                                        |app, cx| app.show_history(cx),
                                    ))
                                    .child(self.button(
                                        "relay-mode",
                                        self.text("preferences.relay"),
                                        true,
                                        colors,
                                        cx,
                                        |app, cx| app.cycle_relay(cx),
                                    ))
                                    .child(self.button(
                                        "retry-limit",
                                        self.text("preferences.retry"),
                                        true,
                                        colors,
                                        cx,
                                        |app, cx| app.cycle_retry_limit(cx),
                                    ))
                                    .child(self.button(
                                        "download-chunk",
                                        self.text("preferences.chunk"),
                                        true,
                                        colors,
                                        cx,
                                        |app, cx| app.cycle_download_chunk(cx),
                                    )),
                            )
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
                                    .min_w(px(0.0))
                                    .min_h(px(0.0))
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
                                                .child(self.error.clone().unwrap_or_default()),
                                        )
                                    })
                                    .when(self.history_open, |view| {
                                        view.child(history_panel(self, colors, cx))
                                    })
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.muted)
                                            .child(self.status.clone()),
                                    )
                                    .when(self.diagnostics.is_some(), |view| {
                                        view.child(
                                            div().text_xs().text_color(colors.muted).child(
                                                self.diagnostics.clone().unwrap_or_default(),
                                            ),
                                        )
                                    }),
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
                                    .on_click(cx.listener(|app, _, _, cx| app.new_transfer(cx)))
                                    .child(self.copy("new")),
                            )
                            .child(
                                div()
                                    .id("footer-sponsor")
                                    .role(A11yRole::Button)
                                    .aria_label(self.text("donate"))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.open_sponsor_page(cx)),
                                    )
                                    .child(self.text("donate")),
                            )
                            .child(
                                div()
                                    .id("footer-update")
                                    .role(A11yRole::Button)
                                    .aria_label(self.text("update.checkNow"))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.open_update_or_check(cx)),
                                    )
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

fn progress_bar(progress: &Progress, colors: Palette) -> gpui::Div {
    // Keep the fill proportional to its parent so the bar remains usable at the
    // minimum window size instead of overflowing a fixed pixel width.
    let fraction = progress_fraction(progress);
    div()
        .h(px(8.0))
        .w_full()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(
            div()
                .h(px(8.0))
                .w(relative(fraction))
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
        "-windows-setup.exe"
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
    use super::{current_asset_suffix, current_platform_key, is_newer_version};

    #[test]
    fn compares_semver_like_versions() {
        assert!(is_newer_version("0.2.0", "0.2.1"));
        assert!(is_newer_version("0.2.0", "1.0.0"));
        assert!(!is_newer_version("0.2.0", "0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.1.99"));
        assert!(is_newer_version("v0.2.0", "v0.3.0"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn selects_the_windows_installer_published_by_release_workflow() {
        assert_eq!(current_asset_suffix(), "-windows-setup.exe");
        assert_eq!(current_platform_key(), "windows-x86_64-nsis");
    }
}

/// Returns whether an active transfer must keep the user on the current tab.
/// Completed and failed states remain navigable so a user can switch workflows
/// without first creating a new transfer.
fn tabs_locked(send_phase: TransferPhase, receive_phase: TransferPhase) -> bool {
    send_phase.is_active() || receive_phase.is_active()
}

/// Converts a transfer percentage into a stable relative fill fraction for layout.
fn progress_fraction(progress: &Progress) -> f32 {
    (progress.percentage() / 100.0).clamp(0.0, 1.0)
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
                .child(if app.is_send_stopped() {
                    app.text("transfer.stopped")
                } else {
                    app.text("transfer.complete")
                }),
        )
        .child(
            div().text_sm().child(
                app.completed_name
                    .clone()
                    .unwrap_or_else(|| app.text("transfer.file")),
            ),
        )
        .child(
            div()
                .text_sm()
                .text_color(colors.muted)
                .child(if app.is_send_stopped() {
                    app.text("transfer.wasStopped")
                } else {
                    format!(
                        "{} bytes - {:.1}s",
                        app.completed_size,
                        app.completed_duration.as_secs_f32()
                    )
                }),
        )
        .child(app.button(
            "receive-reveal",
            app.text("open_folder"),
            app.completed_path.is_some(),
            colors,
            cx,
            |app, cx| app.reveal_completed_path(cx),
        ))
        .child(app.button(
            "receive-done",
            if app.receive_phase == TransferPhase::Completed {
                app.copy("new").to_owned()
            } else {
                app.text("transfer.done")
            },
            true,
            colors,
            cx,
            |app, cx| app.done_transfer(cx),
        ))
}

fn history_panel(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui::Div {
    let rows = app.history.iter().enumerate().fold(
        div().flex().flex_col().gap_2(),
        |panel, (_index, entry)| {
            let ticket = entry.ticket.clone();
            let path = entry.path.clone();
            let summary = format!(
                "{} | {} | {} bytes | {} ms | {}",
                entry.role, entry.outcome, entry.size, entry.duration_ms, entry.date
            );
            panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(colors.muted)
                            .truncate()
                            .child(format!("{path} - {summary}")),
                    )
                    .when_some(ticket, |view, ticket| {
                        view.child(app.button(
                            ("history-copy", entry.size as usize),
                            app.copy("copy"),
                            true,
                            colors,
                            cx,
                            move |app, cx| app.copy_history_ticket(ticket.clone(), cx),
                        ))
                    }),
            )
        },
    );
    rows.p_3()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(app.button(
            "history-clear",
            app.text("history.clear"),
            !app.history.is_empty(),
            colors,
            cx,
            |app, cx| app.clear_history(cx),
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
        .focusable()
        .tab_stop(true)
        .role(A11yRole::TextInput)
        .aria_label(app.text("receiver.pasteTicket"))
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
                    app.text("receiver.ticketPlaceholder")
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

fn history_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.data_dir().join("history.json"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-history.json"))
}

fn theme_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("theme"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-theme"))
}

fn locale_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("locale"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-locale"))
}

/// Loads a previously selected locale while falling back to English for invalid or missing data.
fn load_locale() -> Locale {
    match fs::read_to_string(locale_path()).ok().as_deref() {
        Some("ar") => Locale::Arabic,
        Some("cs") => Locale::Czech,
        Some("de") => Locale::German,
        Some("es") => Locale::Spanish,
        Some("fa") => Locale::Persian,
        Some("fr") => Locale::French,
        Some("hi") => Locale::Hindi,
        Some("it") => Locale::Italian,
        Some("ja") => Locale::Japanese,
        Some("ko") => Locale::Korean,
        Some("no") => Locale::Norwegian,
        Some("pl") => Locale::Polish,
        Some("pt-BR") => Locale::BrazilianPortuguese,
        Some("ru") => Locale::Russian,
        Some("sr") => Locale::Serbian,
        Some("th") => Locale::Thai,
        Some("tr") => Locale::Turkish,
        Some("uk") => Locale::Ukrainian,
        Some("zh-CN") => Locale::SimplifiedChinese,
        Some("zh-TW") => Locale::TraditionalChinese,
        _ => Locale::English,
    }
}

/// Stores the locale code in the platform configuration directory for future launches.
fn persist_locale(locale: Locale) {
    let path = locale_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, locale.code());
}

fn preferences_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("preferences.json"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-preferences.json"))
}

fn default_preferences() -> Preferences {
    Preferences {
        relay: "default".to_owned(),
        retry_limit: 3,
        download_limit_mb: 32,
    }
}

fn load_preferences() -> Preferences {
    fs::read_to_string(preferences_path())
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_else(default_preferences)
}

fn persist_preferences(preferences: &Preferences) {
    let path = preferences_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(preferences) {
        let _ = fs::write(path, contents);
    }
}

fn load_theme() -> Theme {
    match fs::read_to_string(theme_path()).ok().as_deref() {
        Some("dark") => Theme::Dark,
        Some("light") => Theme::Light,
        _ => Theme::System,
    }
}

fn persist_theme(theme: Theme) {
    let path = theme_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let value = match theme {
        Theme::System => "system",
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let _ = fs::write(path, value);
}

fn load_history() -> Vec<HistoryEntry> {
    fs::read_to_string(history_path())
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn persist_history(entries: &[HistoryEntry]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(entries) {
        let _ = fs::write(path, contents);
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn ticket_is_valid(ticket: &str) -> bool {
    ticket.parse::<iroh_blobs::ticket::BlobTicket>().is_ok()
}

#[cfg(test)]
mod history_tests {
    use super::{HistoryEntry, Progress, TransferPhase, ticket_is_valid};

    #[test]
    fn phase_model_exposes_idle_active_completion_and_failure() {
        assert!(!TransferPhase::Idle.is_active());
        assert!(TransferPhase::Preparing.is_active());
        assert!(TransferPhase::Transporting.is_active());
        assert!(!TransferPhase::Completed.is_active());
        assert!(!TransferPhase::Failed.is_active());
    }

    #[test]
    fn progress_reports_folder_counts_without_overflow() {
        let progress = Progress {
            processed: 10,
            total: 10,
            speed: 100.0,
            current_name: Some("a.txt".into()),
            completed_files: u32::MAX,
            total_files: u32::MAX,
        };
        assert_eq!(progress.files_label("files"), "4294967295/4294967295 files");
        assert_eq!(progress.percentage(), 100.0);
    }

    #[test]
    fn history_entries_preserve_sender_ticket_and_outcome() {
        let entry = HistoryEntry {
            role: "sender".into(),
            path: "C:/share/a.txt".into(),
            size: 5,
            duration_ms: 10,
            speed: 500,
            date: "1".into(),
            outcome: "completed".into(),
            ticket: Some("ticket".into()),
        };
        let encoded = serde_json::to_string(&entry).expect("history serializes");
        let decoded: HistoryEntry = serde_json::from_str(&encoded).expect("history parses");
        assert_eq!(decoded, entry);
    }

    #[test]
    fn receiver_rejects_empty_or_malformed_ticket() {
        assert!(!ticket_is_valid(""));
        assert!(!ticket_is_valid("sendme receive ticket"));
    }
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
                speed: 0.0,
                ..Default::default()
            }
            .percentage(),
            100.0
        );
    }

    #[test]
    fn progress_fraction_stays_within_relative_layout_bounds() {
        assert_eq!(progress_fraction(&Progress::default()), 0.0);
        assert_eq!(
            progress_fraction(&Progress {
                processed: 5,
                total: 10,
                ..Default::default()
            }),
            0.5
        );
        assert_eq!(
            progress_fraction(&Progress {
                processed: 20,
                total: 10,
                ..Default::default()
            }),
            1.0
        );
    }

    #[test]
    fn utf16_ranges_handle_non_bmp_ticket_text() {
        let text = "a😀b";
        assert_eq!(utf16_to_byte_range(text, 1..3), 1..5);
        assert_eq!(byte_to_utf16_range(text, 1..5), 1..3);
    }

    #[test]
    fn tabs_lock_only_while_a_transfer_is_active() {
        assert!(tabs_locked(TransferPhase::Preparing, TransferPhase::Idle));
        assert!(tabs_locked(
            TransferPhase::Idle,
            TransferPhase::Transporting
        ));
        assert!(!tabs_locked(TransferPhase::Completed, TransferPhase::Idle));
        assert!(!tabs_locked(
            TransferPhase::Failed,
            TransferPhase::Completed
        ));
    }
}
