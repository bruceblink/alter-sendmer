//! AlterSendmer 的事件状态和事件字段映射。

use super::*;

impl AlterSendmeApp {
    pub(crate) fn event_state(&self, role: Role) -> &EventSessionState {
        match role {
            Role::Sender => &self.sender_event_state,
            Role::Receiver => &self.receiver_event_state,
        }
    }

    pub(crate) fn event_state_mut(&mut self, role: Role) -> &mut EventSessionState {
        match role {
            Role::Sender => &mut self.sender_event_state,
            Role::Receiver => &mut self.receiver_event_state,
        }
    }

    /// Accepts only a complete, strictly ordered session stream from sendmer's event contract.
    pub(crate) fn accept_event(&mut self, event: &TransferEvent) -> bool {
        self.event_state_mut(event.role).accept(event)
    }

    pub(crate) fn reset_event_state(&mut self, role: Role) {
        *self.event_state_mut(role) = EventSessionState::default();
    }

    /// Captures elapsed time for one role while its transfer is active, or reuses its last value.
    pub(crate) fn capture_transfer_duration(&mut self, role: Role) -> Duration {
        if let Some(started) = self.transfer_started_at {
            let duration = started.elapsed();
            self.event_state_mut(role).duration_ms = duration.as_millis();
            duration
        } else {
            let duration_ms = self.event_state(role).duration_ms;
            Duration::from_millis(duration_ms.min(u64::MAX as u128) as u64)
        }
    }

    /// Selects current active time or the recorded terminal duration for diagnostics per role.
    pub(crate) fn diagnostic_duration_ms(&self, role: Role) -> u128 {
        let phase_is_active = match role {
            Role::Sender => self.send_phase.is_active(),
            Role::Receiver => self.receive_phase.is_active(),
        };
        if phase_is_active {
            self.transfer_started_at
                .map(|started| started.elapsed().as_millis())
                .unwrap_or_default()
        } else {
            self.event_state(role).duration_ms
        }
    }

    /// Projects the core phase onto the desktop state machine without reimplementing transport.
    pub(crate) fn apply_core_phase(&mut self, role: Role, phase: CoreTransferPhase) {
        let ui_phase = ui_phase_for_core(role, phase);
        match role {
            Role::Sender => self.send_phase = ui_phase,
            Role::Receiver => self.receive_phase = ui_phase,
        }
    }

    pub(crate) fn structured_error_message(&self, error: &TransferError) -> String {
        let summary = self.text(transfer_error_locale_key(error.code));
        let retry_hint = if error.retryable {
            format!(" ({})", self.text("errors.retryable"))
        } else {
            String::new()
        };
        format!("{summary}: {}{retry_hint}", error.message)
    }

    /// 记录一次传输结果，只保存文件名和诊断字段，不保存完整路径或 bearer ticket。
    pub(crate) fn record_history(
        &mut self,
        role: &str,
        path: Option<&PathBuf>,
        outcome: &str,
        size_override: Option<u64>,
    ) {
        let role_kind = if role == "sender" {
            Role::Sender
        } else {
            Role::Receiver
        };
        let duration = self.capture_transfer_duration(role_kind);
        let size = size_override.unwrap_or_else(|| path.map(path_size).unwrap_or(0));
        let speed = if duration.is_zero() {
            0
        } else {
            (size as f64 / duration.as_secs_f64()) as u64
        };
        let event_state = self.event_state(role_kind).clone();
        self.history.push(HistoryEntry {
            role: role.to_owned(),
            path: path
                .and_then(|value| value.file_name())
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default(),
            size,
            duration_ms: duration.as_millis(),
            speed,
            date: unix_timestamp().to_string(),
            outcome: outcome.to_owned(),
            ticket: None,
            session_id: event_state.session_id,
            error_code: event_state.error_code,
            error_phase: event_state.error_phase,
        });
        persist_history(&self.history);
    }

    pub(crate) fn apply_event(&mut self, event: TransferEvent, cx: &mut Context<Self>) {
        if !self.accept_event(&event) {
            return;
        }
        let role = event.role;
        let role_name = match role {
            Role::Sender => "sender",
            Role::Receiver => "receiver",
        };
        if !matches!(&event.event, TransferEventData::Started)
            && !self
                .recovery
                .matches_session(role_name, event.session_id.as_str())
        {
            tracing::debug!(role = role_name, "ignoring event outside recovery session");
            return;
        }
        self.apply_core_phase(role, event.phase);
        match event.event {
            TransferEventData::Started => self.apply_started(role, event.session_id.as_str()),
            TransferEventData::Progress {
                processed,
                total,
                speed_bytes_per_sec,
            } => self.apply_progress(role, processed, total, speed_bytes_per_sec),
            TransferEventData::FileNames { file_names } => self.apply_file_names(role, file_names),
            TransferEventData::Completed => self.apply_completed(role),
            TransferEventData::Failed { error } => self.apply_failed(role, error),
            TransferEventData::Cancelled => self.apply_cancelled(role),
        }
        if self.recovery.should_persist() {
            match persist_recovery(&self.recovery) {
                Ok(()) => self.recovery.mark_persisted(),
                Err(error) => tracing::warn!(error = %error, "failed to persist recovery snapshot"),
            }
        }
        cx.notify();
    }

    fn apply_started(&mut self, role: Role, session_id: &str) {
        let manifest = if role == Role::Sender {
            self.manifest_summary.clone()
        } else {
            None
        };
        self.recovery.begin(
            match role {
                Role::Sender => "sender",
                Role::Receiver => "receiver",
            },
            session_id.to_owned(),
            manifest,
        );
        self.transfer_started_at = Some(Instant::now());
        self.status = match role {
            Role::Sender => self.status_text("preparing", "Preparing for transport"),
            Role::Receiver => self.status_text("connecting", "Connecting to sender"),
        };
    }

    fn apply_progress(&mut self, role: Role, processed: u64, total: u64, speed: f64) {
        match role {
            Role::Sender => {
                let total_files = self.send_progress.total_files.max(
                    self.selected_path
                        .as_ref()
                        .map(path_file_count)
                        .unwrap_or(1),
                );
                self.send_progress = Progress {
                    processed,
                    total,
                    speed,
                    current_name: self.selected_path.as_ref().and_then(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                    }),
                    completed_files: if processed >= total && total > 0 {
                        total_files
                    } else {
                        0
                    },
                    total_files,
                };
                self.send_phase = TransferPhase::Transporting;
                self.status = self.status_text("sharing", "Sharing in progress");
            }
            Role::Receiver => {
                let total_files = self.receive_files.len().max(1) as u32;
                self.receive_progress = Progress {
                    processed,
                    total,
                    speed,
                    current_name: self.receive_files.first().cloned(),
                    completed_files: if processed >= total && total > 0 {
                        total_files
                    } else {
                        0
                    },
                    total_files,
                };
                self.receive_phase = TransferPhase::Transporting;
                self.status = self.status_text("downloading", "Downloading in progress");
            }
        }
        self.recovery.update_progress(processed, total, speed);
    }

    fn apply_file_names(&mut self, role: Role, file_names: Vec<String>) {
        if role == Role::Receiver {
            self.recovery.manifest =
                Some(ManifestSummary::from_received_file_names(file_names.len()));
            self.receive_progress.total_files = file_names.len().max(1) as u32;
            self.receive_files = file_names;
            self.receive_progress.current_name = self.receive_files.first().cloned();
            self.receive_phase = TransferPhase::Metadata;
        }
    }

    fn apply_completed(&mut self, role: Role) {
        self.recovery.complete();
        match role {
            Role::Sender => {
                let recorded = self.sender_status == SenderTransferStatus::Completed;
                self.sender_status = SenderTransferStatus::Completed;
                self.completed_stopped = false;
                self.send_phase = TransferPhase::Completed;
                self.send_progress.completed_files = self.send_progress.total_files;
                self.completed_name = self
                    .selected_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().to_string());
                self.completed_path = self.selected_path.clone();
                self.completed_size = self.send_progress.total;
                self.completed_duration = self.capture_transfer_duration(Role::Sender);
                self.transfer_started_at = None;
                self.status = self.status_text("transfer_completed", "Transfer completed");
                let path = self.selected_path.clone();
                if !recorded {
                    self.record_history(
                        "sender",
                        path.as_ref(),
                        "completed",
                        Some(self.send_progress.total),
                    );
                }
                self.ticket = None;
            }
            Role::Receiver => {
                self.completed_stopped = false;
                self.receive_phase = TransferPhase::Finalizing;
                self.status = self.status_text("finalizing", "Finalizing download");
            }
        }
    }

    fn apply_failed(&mut self, role: Role, error: TransferError) {
        let sender_expired = role == Role::Sender
            && sender_status_for_failure(&error) == SenderTransferStatus::Expired;
        let message = self.structured_error_message(&error);
        let error_code = transfer_error_code_name(error.code).to_owned();
        let error_phase = transfer_phase_name(error.phase).to_owned();
        let recovery_error = format!("{error_code}@{error_phase}");
        self.capture_transfer_duration(role);
        let state = self.event_state_mut(role);
        state.error_code = Some(error_code);
        state.error_phase = Some(error_phase);
        state.retryable = Some(error.retryable);
        self.error = Some(message);
        self.recovery.fail(recovery_error);

        match role {
            Role::Sender => self.apply_sender_failure(error, sender_expired),
            Role::Receiver => self.receive_phase = TransferPhase::Failed,
        }
        self.transfer_started_at = None;
        self.status = if sender_expired {
            self.status_text("sender.sessionExpired", "Sharing session expired")
        } else {
            self.status_text("transfer_failed", "Transfer failed")
        };
    }

    fn apply_sender_failure(&mut self, error: TransferError, sender_expired: bool) {
        self.sender_status = sender_status_for_failure(&error);
        self.send_phase = TransferPhase::Failed;
        let path = self.selected_path.clone();
        self.record_history(
            "sender",
            path.as_ref(),
            if sender_expired { "expired" } else { "failed" },
            Some(self.send_progress.processed),
        );
        self.ticket = None;
        if let Some(handle) = self.send_result.take() {
            tokio::spawn(async move {
                if handle.close().await.is_err() {
                    tracing::warn!("failed to clean up ended sender session");
                }
            });
        }
    }

    fn apply_cancelled(&mut self, role: Role) {
        self.recovery.cancel();
        self.completed_stopped = true;
        self.capture_transfer_duration(role);
        self.transfer_started_at = None;
        match role {
            Role::Sender => {
                self.sender_status = SenderTransferStatus::Aborted;
                self.send_phase = TransferPhase::Completed;
                let path = self.selected_path.clone();
                self.record_history(
                    "sender",
                    path.as_ref(),
                    "cancelled",
                    Some(self.send_progress.processed),
                );
                self.ticket = None;
            }
            Role::Receiver => {
                self.receive_phase = TransferPhase::Completed;
                let save_path = self.save_path.clone();
                self.record_history(
                    "receiver",
                    Some(&save_path),
                    "cancelled",
                    Some(self.receive_progress.processed),
                );
            }
        }
        self.status = self.status_text("stopped", "Transmission stopped");
    }
}
