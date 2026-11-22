//! GPUI workbench and its state machine.

use crate::locale::Locale;
use crate::transfer;
use crate::updater::{self, AvailableUpdate};
use async_channel::{Receiver, Sender};
use gpui_kit::prelude::*;
use gpui_kit::{
    AsyncApp, Context, ElementInputHandler, ExternalPaths, FocusHandle, FontWeight, KeyDownEvent,
    PathPromptOptions, Render, Rgba, Role as A11yRole, Window, WindowAppearance, div, px, relative,
    rgb, svg,
};
use sendmer::{
    Role, SendHandle, SenderTransferStatus, TransferError, TransferEvent, TransferEventData,
    TransferPhase as CoreTransferPhase,
};
use std::{
    fs,
    ops::Range,
    path::PathBuf,
    time::{Duration, Instant},
};

#[path = "app_model.rs"]
mod app_model;
use app_model::*;
#[path = "app_actions.rs"]
mod app_actions;
#[path = "app_diagnostics.rs"]
mod app_diagnostics;
#[path = "app_events.rs"]
mod app_events;
#[path = "app_receive_view.rs"]
mod app_receive_view;
#[path = "app_send_view.rs"]
mod app_send_view;
#[path = "app_transfer.rs"]
mod app_transfer;
use app_receive_view::pump_events;
#[path = "app_window_view.rs"]
mod app_window_view;

#[path = "app_persistence.rs"]
mod app_persistence;
use app_persistence::*;
#[path = "app_input.rs"]
mod app_input;
#[path = "app_render.rs"]
mod app_render;
use app_render::*;
#[path = "app_cards.rs"]
mod app_cards;
use app_cards::*;
#[cfg(test)]
#[path = "app_tests.rs"]
mod app_tests;

/// Owns all user-facing state and serializes async transfer completions by generation.
pub struct AlterSendmeApp {
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
    send_result: Option<SendHandle>,
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
    upload_rate_focus: FocusHandle,
    upload_rate_input: String,
    upload_rate_selection: Range<usize>,
    generation: u64,
    update_checking: bool,
    update_installing: bool,
    update_info: Option<AvailableUpdate>,
    update_status: Option<String>,
    history: Vec<HistoryEntry>,
    history_open: bool,
    preferences_open: bool,
    locale_menu_open: bool,
    preferences: Preferences,
    cache_maintenance_running: bool,
    diagnostics: Option<String>,
    receiver_help_open: bool,
    completed_stopped: bool,
    sender_event_state: EventSessionState,
    receiver_event_state: EventSessionState,
    sender_status: SenderTransferStatus,
}

impl AlterSendmeApp {
    /// Creates a ready-to-use workspace and starts a lightweight event pump.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (event_sender, event_receiver) = async_channel::unbounded();
        let save_path = directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let root_focus = cx.focus_handle();
        let ticket_focus = cx.focus_handle();
        let upload_rate_focus = cx.focus_handle();
        let preferences = load_preferences();
        let upload_rate_input = preferences
            .max_upload_rate_mib_per_sec
            .map(|rate| rate.to_string())
            .unwrap_or_default();
        window.focus(&root_focus, cx);
        let app = Self {
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
            ticket_focus,
            upload_rate_focus,
            upload_rate_selection: upload_rate_input.len()..upload_rate_input.len(),
            upload_rate_input,
            generation: 0,
            update_checking: false,
            update_installing: false,
            update_info: None,
            update_status: None,
            history: load_history(),
            history_open: false,
            preferences_open: false,
            locale_menu_open: false,
            preferences,
            cache_maintenance_running: false,
            diagnostics: None,
            receiver_help_open: false,
            completed_stopped: false,
            sender_event_state: EventSessionState::default(),
            receiver_event_state: EventSessionState::default(),
            sender_status: SenderTransferStatus::Idle,
        };
        let entity = cx.entity().downgrade();
        cx.spawn(
            move |_this: gpui_kit::WeakEntity<Self>, cx: &mut AsyncApp| {
                let mut cx = cx.clone();
                let receiver = event_receiver;
                async move { pump_events(entity, receiver, &mut cx).await }
            },
        )
        .detach();
        // Match the legacy client by checking quietly on startup. Only an available update is
        // surfaced; offline and up-to-date startup checks stay out of the transfer workflow.
        cx.defer_in(window, |app, _, cx| app.check_for_updates(false, cx));
        app
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
