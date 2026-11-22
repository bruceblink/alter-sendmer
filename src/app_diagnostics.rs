//! AlterSendmer 的更新、诊断、历史和偏好动作。

use super::*;

impl AlterSendmeApp {
    /// Shows a local diagnostic summary so support can identify the app, relay, and storage setup.
    pub(crate) fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        if self.diagnostics.is_some() {
            self.diagnostics = None;
        } else {
            self.history_open = false;
            self.preferences_open = false;
            let sender_session = self.sender_event_state.session_id.as_deref().unwrap_or("-");
            let receiver_session = self
                .receiver_event_state
                .session_id
                .as_deref()
                .unwrap_or("-");
            let last_error = self
                .receiver_event_state
                .error_code
                .as_deref()
                .or(self.sender_event_state.error_code.as_deref())
                .unwrap_or("-");
            let client_version = format!(
                "{} {}",
                self.text("diagnostics.client"),
                env!("CARGO_PKG_VERSION")
            );
            let relay = self.relay_label();
            let receive_cache = self.receive_cache_state_label();
            let sender_duration_ms = self.diagnostic_duration_ms(Role::Sender);
            let receiver_duration_ms = self.diagnostic_duration_ms(Role::Receiver);
            let sender_error_code = self.sender_event_state.error_code.as_deref().unwrap_or("-");
            let sender_error_phase = self
                .sender_event_state
                .error_phase
                .as_deref()
                .unwrap_or("-");
            let receiver_error_code = self
                .receiver_event_state
                .error_code
                .as_deref()
                .unwrap_or("-");
            let receiver_error_phase = self
                .receiver_event_state
                .error_phase
                .as_deref()
                .unwrap_or("-");
            self.diagnostics = Some(format_diagnostics(DiagnosticSummary {
                client_version: &client_version,
                relay: &relay,
                history_count: self.history.len(),
                sendmer_version: SENDMER_VERSION,
                receive_cache: &receive_cache,
                sender: DiagnosticTransferState {
                    role: "sender",
                    phase: diagnostic_phase_name(self.send_phase),
                    sequence: self.sender_event_state.sequence,
                    session_id: sender_session,
                    error_code: sender_error_code,
                    error_phase: sender_error_phase,
                    duration_ms: sender_duration_ms,
                },
                receiver: DiagnosticTransferState {
                    role: "receiver",
                    phase: diagnostic_phase_name(self.receive_phase),
                    sequence: self.receiver_event_state.sequence,
                    session_id: receiver_session,
                    error_code: receiver_error_code,
                    error_phase: receiver_error_phase,
                    duration_ms: receiver_duration_ms,
                },
                last_error,
            }));
            self.status = self.text("diagnostics.completed");
        }
        cx.notify();
    }

    /// Save the visible diagnostic summary without copying private transfer data to disk.
    pub(crate) fn save_diagnostics(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.diagnostics.clone() else {
            return;
        };
        let prompt = self.text("diagnostics.savePrompt");
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
                    && let Some(directory) = paths.into_iter().next()
                {
                    let result = write_diagnostic_report(&directory, unix_timestamp(), &report);
                    let _ = app.update(&mut cx, |app, cx| {
                        match result {
                            Ok(_) => app.status = app.text("diagnostics.saved"),
                            Err(error) => {
                                app.error = Some(format!(
                                    "{}: {error}",
                                    app.text("diagnostics.saveFailed")
                                ));
                            }
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn save_ticket(&mut self, cx: &mut Context<Self>) {
        let Some(ticket) = self.ticket.clone() else {
            return;
        };
        let path = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| format!("{}.sendme.ticket", name.to_string_lossy()))
            .unwrap_or_else(|| "alter-sendmer.ticket".to_owned());
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

    pub(crate) fn open_shared_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.selected_path.clone() {
            let target = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(&path).to_path_buf()
            };
            cx.open_with_system(&target);
            self.status = self.text("sender.folderOpened");
            cx.notify();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.history.clear();
        persist_history(&self.history);
        self.status = self.text("history.cleared");
        cx.notify();
    }

    pub(crate) fn show_history(&mut self, cx: &mut Context<Self>) {
        self.history_open = !self.history_open;
        if self.history_open {
            self.preferences_open = false;
            self.diagnostics = None;
            self.status = if self.history.is_empty() {
                self.text("history.empty")
            } else {
                format!("{}: {}", self.text("history.title"), self.history.len())
            };
        } else {
            self.status = self.text("ready");
        }
        cx.notify();
    }

    /// Opens the grouped transfer preferences without mixing settings into the primary task tabs.
    pub(crate) fn show_preferences(&mut self, cx: &mut Context<Self>) {
        self.preferences_open = !self.preferences_open;
        if self.preferences_open {
            self.history_open = false;
            self.diagnostics = None;
        }
        cx.notify();
    }

    pub(crate) fn copy_history_ticket(&mut self, ticket: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ticket.into());
        self.status = self.text("sender.ticketCopied");
        cx.notify();
    }

    pub(crate) fn cycle_relay(&mut self, cx: &mut Context<Self>) {
        self.preferences.relay = if self.preferences.relay == "default" {
            "disabled".to_owned()
        } else {
            "default".to_owned()
        };
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.relay"))
            .replace("{{value}}", &self.relay_label());
        cx.notify();
    }

    /// Cycles the sender's active receiver limit while keeping the unlimited default explicit.
    pub(crate) fn cycle_max_receivers(&mut self, cx: &mut Context<Self>) {
        let current = self.preferences.max_receivers;
        let next_index = MAX_RECEIVER_PRESETS
            .iter()
            .position(|value| *value == current)
            .map_or(0, |index| (index + 1) % MAX_RECEIVER_PRESETS.len());
        self.preferences.max_receivers = MAX_RECEIVER_PRESETS[next_index];
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.maxReceivers"))
            .replace("{{value}}", &self.max_receivers_label());
        cx.notify();
    }

    /// Cycles fixed sender lifetimes; sendmer owns the actual expiry timer after readiness.
    pub(crate) fn cycle_session_lifetime(&mut self, cx: &mut Context<Self>) {
        let current = self.preferences.session_lifetime_seconds;
        let next_index = SESSION_LIFETIME_PRESETS
            .iter()
            .position(|value| *value == current)
            .map_or(0, |index| (index + 1) % SESSION_LIFETIME_PRESETS.len());
        self.preferences.session_lifetime_seconds = SESSION_LIFETIME_PRESETS[next_index];
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.sessionLifetime"))
            .replace("{{value}}", &self.session_lifetime_label());
        cx.notify();
    }

    /// Toggles the explicit TM1 sender mode; legacy Collection V0 remains the default.
    pub(crate) fn toggle_manifest_mode(&mut self, cx: &mut Context<Self>) {
        self.preferences.manifest_mode = !self.preferences.manifest_mode;
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.manifest"))
            .replace("{{value}}", &self.manifest_mode_label());
        cx.notify();
    }

    /// Selects a locale from the dropdown and persists it for the next launch.
    pub(crate) fn select_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        self.locale_menu_open = false;
        persist_locale(self.locale);
        self.status = self.text("ready");
        cx.notify();
    }

    /// Toggles the receiver instructions without leaving the active transfer surface.
    pub(crate) fn toggle_receiver_help(&mut self, cx: &mut Context<Self>) {
        self.receiver_help_open = !self.receiver_help_open;
        cx.notify();
    }

    pub(crate) fn cycle_retry_limit(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn cycle_download_chunk(&mut self, cx: &mut Context<Self>) {
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

    /// Enables or disables cross-process receive recovery without changing the save directory.
    pub(crate) fn toggle_receive_cache(&mut self, cx: &mut Context<Self>) {
        self.preferences.receive_cache_enabled = !self.preferences.receive_cache_enabled;
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.receiveCache"))
            .replace("{{value}}", &self.receive_cache_state_label());
        cx.notify();
    }

    /// Cycles the retention written into newly created cache entries.
    pub(crate) fn cycle_receive_cache_ttl(&mut self, cx: &mut Context<Self>) {
        let current = self.preferences.receive_cache_ttl_days;
        let next_index = RECEIVE_CACHE_TTL_PRESETS
            .iter()
            .position(|days| *days == current)
            .map_or(0, |index| (index + 1) % RECEIVE_CACHE_TTL_PRESETS.len());
        self.preferences.receive_cache_ttl_days = RECEIVE_CACHE_TTL_PRESETS[next_index];
        persist_preferences(&self.preferences);
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.cacheRetention"))
            .replace("{{value}}", &self.receive_cache_ttl_label());
        cx.notify();
    }

    /// Removes only expired receive-cache entries while preserving active and unknown data.
    pub(crate) fn prune_receive_cache(&mut self, cx: &mut Context<Self>) {
        if self.cache_maintenance_running {
            return;
        }
        self.cache_maintenance_running = true;
        self.error = None;
        self.status = self.text("preferences.pruningCache");
        let root_dir = receive_cache_path();
        let task = tokio::spawn(sendmer::prune_receive_cache(root_dir));
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = task
                    .await
                    .map_err(|error| anyhow::anyhow!("cache maintenance task failed: {error}"))
                    .and_then(|result| result);
                let _ = app.update(&mut cx, |app, cx| {
                    app.cache_maintenance_running = false;
                    match result {
                        Ok(report) => {
                            app.status = app
                                .text("preferences.cachePruned")
                                .replace("{{removed}}", &report.removed_entries.to_string())
                                .replace("{{retained}}", &report.retained_entries.to_string())
                                .replace("{{active}}", &report.active_entries.to_string())
                                .replace("{{unknown}}", &report.unknown_entries.to_string());
                        }
                        Err(error) => {
                            app.error = Some(format!(
                                "{}: {error}",
                                app.text("preferences.cachePruneFailed")
                            ));
                            app.status = app.text("preferences.cachePruneFailed");
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Switches upload pacing off and persists the unrestricted sender mode.
    pub(crate) fn clear_upload_rate(&mut self, cx: &mut Context<Self>) {
        self.preferences.max_upload_rate_mib_per_sec = None;
        self.upload_rate_input.clear();
        self.upload_rate_selection = 0..0;
        persist_preferences(&self.preferences);
        self.error = None;
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.uploadRate"))
            .replace("{{value}}", &self.text("preferences.unlimited"));
        cx.notify();
    }

    /// Enables a practical default limit, selects it for replacement, and focuses the numeric field.
    pub(crate) fn focus_upload_rate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.max_upload_rate_mib_per_sec.is_none() {
            self.preferences.max_upload_rate_mib_per_sec = Some(DEFAULT_UPLOAD_RATE_MIB_PER_SEC);
            self.upload_rate_input = DEFAULT_UPLOAD_RATE_MIB_PER_SEC.to_string();
            persist_preferences(&self.preferences);
        }
        self.upload_rate_selection = 0..self.upload_rate_input.len();
        self.error = None;
        self.upload_rate_focus.focus(window, cx);
        cx.notify();
    }
}
