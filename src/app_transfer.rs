//! AlterSendmer 的发送、接收和传输生命周期。

use super::*;

impl AlterSendmeApp {
    pub(crate) fn start_sharing(&mut self, cx: &mut Context<Self>) {
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
        self.reset_event_state(Role::Sender);
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
        let task = tokio::spawn(transfer::start_send(
            path,
            events,
            generation,
            transfer::SendTransferOptions {
                relay_mode,
                max_upload_rate_mib_per_sec: self.preferences.max_upload_rate_mib_per_sec,
                max_receivers: self.preferences.max_receivers,
                session_lifetime_seconds: self.preferences.session_lifetime_seconds,
                manifest_mode: self.preferences.manifest_mode,
            },
        ));
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

    pub(crate) fn apply_send_ready(
        &mut self,
        result: Result<SendHandle, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(result) => {
                let mut status_rx = result.subscribe_transfer_status();
                self.send_abort = None;
                self.sender_status = SenderTransferStatus::Idle;
                self.ticket = Some(result.ticket().to_string());
                self.send_result = Some(result);
                self.send_phase = TransferPhase::Sharing;
                self.status = self.status_text("listening", "Listening for a receiver");

                // 单次传输完成由状态订阅报告。结构化终态事件表示整个分享会话结束。
                let generation = self.generation;
                let app = cx.entity().downgrade();
                cx.spawn(move |_, cx: &mut AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        loop {
                            let status = *status_rx.borrow_and_update();
                            let active = app
                                .update(&mut cx, |app, cx| {
                                    if app.generation != generation || app.send_result.is_none() {
                                        return false;
                                    }
                                    if status == SenderTransferStatus::Completed {
                                        app.mark_share_transfer_completed(cx);
                                    }
                                    true
                                })
                                .unwrap_or(false);
                            if !active
                                || status == SenderTransferStatus::Expired
                                || status_rx.changed().await.is_err()
                            {
                                break;
                            }
                        }
                    }
                })
                .detach();
            }
            Err(error) => {
                self.send_abort = None;
                self.sender_status = SenderTransferStatus::Aborted;
                self.send_phase = TransferPhase::Failed;
                if self.error.is_none() {
                    self.error = Some(error);
                }
                self.status = self.status_text("transfer_failed", "Sharing failed");
            }
        }
        cx.notify();
    }

    /// 记录首次传输完成，继续持有原票据和发送端。后续请求沿用同一分享会话。
    pub(crate) fn mark_share_transfer_completed(&mut self, cx: &mut Context<Self>) {
        if self.sender_status == SenderTransferStatus::Completed {
            return;
        }
        self.sender_status = SenderTransferStatus::Completed;
        self.send_phase = TransferPhase::Sharing;
        self.send_progress.completed_files = self.send_progress.total_files;
        let path = self.selected_path.clone();
        let size = self
            .send_result
            .as_ref()
            .map_or(self.send_progress.total, SendHandle::size);
        self.record_history("sender", path.as_ref(), "completed", Some(size));
        self.transfer_started_at = None;
        self.status = self.text("sender.shareStillActive");
        cx.notify();
    }

    pub(crate) fn stop_sharing(&mut self, cx: &mut Context<Self>) {
        // Snapshot transfer metadata before shutdown clears the live send result and ticket.
        let was_transporting = matches!(
            self.send_phase,
            TransferPhase::Metadata | TransferPhase::Transporting | TransferPhase::Exporting
        );
        let stopped_size = self.send_progress.processed;
        let stopped_name = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string());
        let stopped_path = self.selected_path.clone();
        let stopped_duration = self.capture_transfer_duration(Role::Sender);
        self.generation += 1;
        let generation = self.generation;
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        let Some(result) = self.send_result.take() else {
            if was_transporting {
                self.sender_status = SenderTransferStatus::Aborted;
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
            self.sender_status = SenderTransferStatus::Idle;
            self.send_phase = TransferPhase::Idle;
            self.transfer_started_at = None;
            self.status = self.status_text("ready", "Ready");
            cx.notify();
            return;
        };
        self.send_phase = TransferPhase::Stopping;
        self.status = self.status_text("stopping", "Stopping share...");
        let task =
            tokio::spawn(async move { result.close().await.map_err(|error| error.to_string()) });
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
                            app.sender_status = SenderTransferStatus::Aborted;
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
                            app.sender_status = SenderTransferStatus::Idle;
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

    pub(crate) fn copy_ticket(&mut self, cx: &mut Context<Self>) {
        if let Some(ticket) = self.ticket.clone() {
            cx.write_to_clipboard(ticket.into());
            self.status = self.status_text("ticket_copied", "Ticket copied to clipboard");
            cx.notify();
        }
    }

    pub(crate) fn retry_send(&mut self, cx: &mut Context<Self>) {
        // Failed sends retain their result until retry; release that router before starting a
        // fresh generation so a retry never leaves two share sessions alive at once.
        if let Some(abort) = self.send_abort.take() {
            abort.abort();
        }
        if let Some(result) = self.send_result.take() {
            tokio::spawn(async move {
                let _ = result.close().await;
            });
        }
        self.send_phase = TransferPhase::Idle;
        self.sender_status = SenderTransferStatus::Idle;
        self.completed_stopped = false;
        self.error = None;
        self.ticket = None;
        self.send_progress = Progress::default();
        self.start_sharing(cx);
    }

    pub(crate) fn retry_receive(&mut self, cx: &mut Context<Self>) {
        self.receive_phase = TransferPhase::Idle;
        self.completed_stopped = false;
        self.error = None;
        self.receive_progress = Progress::default();
        self.start_receiving(cx);
    }

    /// Replaces the receive ticket from the clipboard, keeping the current value when the
    /// trimmed clipboard content exceeds the byte limit so an invalid ticket is never staged.
    pub(crate) fn paste_ticket(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            let ticket = text.trim();
            if !ticket_replacement_fits_limit(0, 0, ticket.len()) {
                self.error = Some(self.ticket_length_error());
                cx.notify();
                return;
            }
            self.ticket_input = ticket.to_owned();
            self.ticket_selection = self.ticket_input.len()..self.ticket_input.len();
            self.error = None;
            cx.notify();
        }
    }

    /// Routes a keyboard paste to the focused text field instead of always replacing the ticket.
    pub(crate) fn paste_focused_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.upload_rate_focus.is_focused(window) {
            self.paste_ticket(cx);
            return;
        }
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.replace_upload_rate(None, text.trim());
            cx.notify();
        }
    }

    pub(crate) fn start_receiving(&mut self, cx: &mut Context<Self>) {
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
        let receive_cache = match transfer::receive_cache_options(
            self.preferences.receive_cache_enabled,
            receive_cache_path(),
            self.preferences.receive_cache_ttl_days,
        ) {
            Ok(options) => options,
            Err(error) => {
                self.error = Some(error.to_string());
                self.receive_phase = TransferPhase::Failed;
                cx.notify();
                return;
            }
        };
        self.generation += 1;
        let generation = self.generation;
        self.reset_event_state(Role::Receiver);
        self.receive_phase = TransferPhase::Connecting;
        self.receiver_help_open = false;
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
                transfer::ReceiveTransferOptions {
                    relay_mode,
                    retry_policy,
                    receive_cache,
                },
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

    /// Starts a new receive attempt, resetting a failed attempt while preserving edited inputs.
    pub(crate) fn begin_or_retry_receiving(&mut self, cx: &mut Context<Self>) {
        if self.receive_phase == TransferPhase::Failed {
            self.retry_receive(cx);
        } else {
            self.start_receiving(cx);
        }
    }

    pub(crate) fn apply_receive_finished(
        &mut self,
        result: Result<sendmer::ReceiveResult, String>,
        cx: &mut Context<Self>,
    ) {
        let completed_duration = self.capture_transfer_duration(Role::Receiver);
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
                self.completed_duration = completed_duration;
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
                let path = self
                    .completed_path
                    .clone()
                    .or_else(|| Some(self.save_path.clone()));
                self.record_history(
                    "receiver",
                    path.as_ref(),
                    "failed",
                    Some(self.receive_progress.processed),
                );
                self.transfer_started_at = None;
                if self.error.is_none() {
                    self.error = Some(error);
                }
                self.status = self.status_text("receive_failed", "Receive failed");
            }
        }
        cx.notify();
    }

    pub(crate) fn stop_receiving(&mut self, cx: &mut Context<Self>) {
        self.capture_transfer_duration(Role::Receiver);
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
        Option<SendHandle>,
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

    pub(crate) fn new_transfer(&mut self, cx: &mut Context<Self>) {
        let active = matches!(
            self.send_phase,
            TransferPhase::Preparing
                | TransferPhase::Sharing
                | TransferPhase::Metadata
                | TransferPhase::Transporting
                | TransferPhase::Exporting
                | TransferPhase::Finalizing
                | TransferPhase::Stopping
        ) || matches!(
            self.receive_phase,
            TransferPhase::Preparing
                | TransferPhase::Connecting
                | TransferPhase::Metadata
                | TransferPhase::Transporting
                | TransferPhase::Exporting
                | TransferPhase::Finalizing
                | TransferPhase::Stopping
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
                let _ = result.close().await;
            });
        }
        self.send_abort = None;
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
        self.history_open = false;
        self.preferences_open = false;
        self.locale_menu_open = false;
        self.diagnostics = None;
        self.error = None;
        self.sender_event_state = EventSessionState::default();
        self.receiver_event_state = EventSessionState::default();
        self.sender_status = SenderTransferStatus::Idle;
        self.status = self.status_text("ready", "Ready");
        cx.notify();
    }

    pub(crate) fn done_transfer(&mut self, cx: &mut Context<Self>) {
        if self.send_phase == TransferPhase::Completed
            || self.receive_phase == TransferPhase::Completed
        {
            self.new_transfer(cx);
        }
    }

    pub(crate) fn on_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        if let Some(path) = paths.paths().first() {
            self.select_path(path.clone(), cx);
        }
    }
}
