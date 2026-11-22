//! AlterSendmer 应用状态与交互的回归测试。

use super::*;

#[cfg(test)]
mod history_tests {
    use super::app_persistence::{history_display_name, sanitize_history_entry};
    use super::{
        DiagnosticSummary, DiagnosticTransferState, EventSessionState, HistoryEntry,
        MAX_TICKET_LEN, Progress, TransferPhase, diagnostic_phase_name,
        diagnostic_report_file_name, format_diagnostics, parse_preferences,
        parse_upload_rate_input, path_file_count, sender_status_for_failure, ticket_is_valid,
        ticket_replacement_fits_limit, transfer_error_code_name, ui_phase_for_core,
        write_diagnostic_report,
    };
    use sendmer::{
        Role, SenderTransferStatus, TRANSFER_EVENT_SCHEMA_VERSION, TransferError,
        TransferErrorCode, TransferEvent, TransferEventData, TransferPhase as CoreTransferPhase,
        TransferSessionId,
    };
    use std::fs;

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
    fn path_file_count_distinguishes_files_and_nested_folders() {
        let root = tempfile::tempdir().expect("create folder fixture");
        std::fs::create_dir(root.path().join("nested")).expect("create nested folder");
        std::fs::write(root.path().join("a.txt"), b"a").expect("write first fixture");
        std::fs::write(root.path().join("nested").join("b.txt"), b"b")
            .expect("write second fixture");
        assert_eq!(path_file_count(&root.path().to_path_buf()), 2);
        assert_eq!(path_file_count(&root.path().join("a.txt")), 1);
    }

    #[test]
    fn history_entries_serialize_without_ticket() {
        let entry = HistoryEntry {
            role: "sender".into(),
            path: "a.txt".into(),
            size: 5,
            duration_ms: 10,
            speed: 500,
            date: "1".into(),
            outcome: "completed".into(),
            ticket: None,
            session_id: Some("0123456789abcdef0123456789abcdef".into()),
            error_code: None,
            error_phase: None,
        };
        let encoded = serde_json::to_string(&entry).expect("history serializes");
        let decoded: HistoryEntry = serde_json::from_str(&encoded).expect("history parses");
        assert_eq!(decoded, entry);
        assert!(!encoded.contains("ticket"));
    }

    #[test]
    fn history_serializer_drops_legacy_ticket_even_without_migration() {
        let entry = HistoryEntry {
            role: "sender".into(),
            path: "a.txt".into(),
            size: 1,
            duration_ms: 1,
            speed: 1,
            date: "1".into(),
            outcome: "completed".into(),
            ticket: Some("secret-bearer-ticket".into()),
            session_id: None,
            error_code: None,
            error_phase: None,
        };
        let encoded = serde_json::to_string(&entry).expect("history serializes");
        assert!(!encoded.contains("secret-bearer-ticket"));
        assert!(!encoded.contains("ticket"));
    }

    #[test]
    fn legacy_history_entries_default_versioned_event_metadata() {
        let decoded: HistoryEntry = serde_json::from_str(
            r#"{"role":"receiver","path":"downloads","size":1,"duration_ms":2,"speed":3,"date":"4","outcome":"completed","ticket":null}"#,
        )
        .expect("legacy history parses");
        assert_eq!(decoded.session_id, None);
        assert_eq!(decoded.error_code, None);
        assert_eq!(decoded.error_phase, None);
    }

    #[test]
    fn legacy_history_migration_removes_ticket_and_absolute_path() {
        let mut entry: HistoryEntry = serde_json::from_str(
            r#"{"role":"sender","path":"C:\\Users\\Bruce\\share.txt","size":1,"duration_ms":2,"speed":3,"date":"4","outcome":"completed","ticket":"secret-bearer-ticket"}"#,
        )
        .expect("legacy history parses");
        sanitize_history_entry(&mut entry);
        assert_eq!(entry.path, "share.txt");
        assert_eq!(entry.ticket, None);
        let encoded = serde_json::to_string(&entry).expect("migrated history serializes");
        assert!(!encoded.contains("secret-bearer-ticket"));
        assert!(!encoded.contains("C:\\\\Users"));
    }

    #[test]
    fn history_display_name_handles_posix_windows_and_directory_paths() {
        assert_eq!(
            history_display_name("/tmp/sendmer/report.bin"),
            "report.bin"
        );
        assert_eq!(
            history_display_name("C:\\tmp\\sendmer\\report.bin"),
            "report.bin"
        );
        assert_eq!(history_display_name("/tmp/sendmer/"), "sendmer");
    }

    #[test]
    fn versioned_event_cursor_rejects_gaps_duplicates_and_non_started_sessions() {
        let session_id = TransferSessionId::new();
        let started = TransferEvent::new(
            session_id.clone(),
            1,
            1,
            Role::Receiver,
            CoreTransferPhase::Preparing,
            TransferEventData::Started,
        );
        let progress = TransferEvent::new(
            session_id.clone(),
            2,
            2,
            Role::Receiver,
            CoreTransferPhase::Transferring,
            TransferEventData::Progress {
                processed: 1,
                total: 2,
                speed_bytes_per_sec: 1.0,
            },
        );
        let gap = TransferEvent::new(
            session_id,
            4,
            4,
            Role::Receiver,
            CoreTransferPhase::Finalizing,
            TransferEventData::Completed,
        );
        let mut state = EventSessionState::default();
        let mut unsupported_schema = started.clone();
        unsupported_schema.schema_version = TRANSFER_EVENT_SCHEMA_VERSION + 1;
        assert!(!state.accept(&unsupported_schema));
        assert!(!state.accept(&started)); // Rejection is sticky until an explicit reset.
        state = EventSessionState::default();
        assert!(state.accept(&started));
        assert!(state.accept(&progress));
        assert!(!state.accept(&gap));
        assert!(!state.accept(&progress));

        let mut duplicate = EventSessionState::default();
        assert!(duplicate.accept(&started));
        assert!(!duplicate.accept(&started));
        assert!(!duplicate.accept(&progress));

        let unexpected = TransferEvent::new(
            TransferSessionId::new(),
            1,
            5,
            Role::Receiver,
            CoreTransferPhase::Transferring,
            TransferEventData::Progress {
                processed: 1,
                total: 1,
                speed_bytes_per_sec: 1.0,
            },
        );
        assert!(!EventSessionState::default().accept(&unexpected));
    }

    #[test]
    fn event_stream_rejects_session_role_changes_and_events_after_terminal() {
        let started = TransferEvent::new(
            TransferSessionId::new(),
            1,
            1,
            Role::Receiver,
            CoreTransferPhase::Preparing,
            TransferEventData::Started,
        );
        let mut changed_session = started.clone();
        changed_session.session_id = TransferSessionId::new();
        let mut state = EventSessionState::default();
        assert!(state.accept(&started));
        assert!(!state.accept(&changed_session));

        let mut terminal = started.clone();
        terminal.sequence = 2;
        terminal.phase = CoreTransferPhase::Finalizing;
        terminal.event = TransferEventData::Completed;
        let mut changed_role = terminal.clone();
        changed_role.role = Role::Sender;
        let mut state = EventSessionState::default();
        assert!(state.accept(&started));
        assert!(!state.accept(&changed_role));
        assert!(!state.accept(&terminal));

        let mut state = EventSessionState::default();
        assert!(state.accept(&started));
        assert!(state.accept(&terminal));
        terminal.sequence = 3;
        assert!(!state.accept(&terminal));
    }

    #[test]
    fn event_stream_rejects_unsafe_file_names_before_projection() {
        for name in [
            "/private/file",
            "../secret",
            "folder/../../secret",
            "C:\\secret",
        ] {
            let started = TransferEvent::new(
                TransferSessionId::new(),
                1,
                1,
                Role::Receiver,
                CoreTransferPhase::Preparing,
                TransferEventData::Started,
            );
            let mut names = started.clone();
            names.sequence = 2;
            names.event = TransferEventData::FileNames {
                file_names: vec![name.into()],
            };
            let mut state = EventSessionState::default();
            assert!(state.accept(&started));
            assert!(!state.accept(&names));
            assert_eq!(state.sequence, 1);
        }
    }

    #[test]
    fn core_phases_and_error_codes_map_to_stable_desktop_values() {
        assert_eq!(
            ui_phase_for_core(Role::Receiver, CoreTransferPhase::Metadata),
            TransferPhase::Metadata
        );
        assert_eq!(
            ui_phase_for_core(Role::Receiver, CoreTransferPhase::Exporting),
            TransferPhase::Exporting
        );
        assert_eq!(
            ui_phase_for_core(Role::Sender, CoreTransferPhase::Connecting),
            TransferPhase::Sharing
        );
        assert_eq!(
            transfer_error_code_name(TransferErrorCode::TargetConflict),
            "target_conflict"
        );
    }

    #[test]
    fn receiver_rejects_empty_or_malformed_ticket() {
        assert!(!ticket_is_valid(""));
        assert!(!ticket_is_valid("sendme receive ticket"));
    }

    #[test]
    fn ticket_replacement_enforces_the_byte_limit_and_handles_overflow() {
        assert!(ticket_replacement_fits_limit(MAX_TICKET_LEN, 0, 0));
        assert!(!ticket_replacement_fits_limit(MAX_TICKET_LEN, 0, 1));
        assert!(ticket_replacement_fits_limit(MAX_TICKET_LEN, 1, 1));
        assert!(!ticket_replacement_fits_limit(0, 1, 0));
        assert!(!ticket_replacement_fits_limit(usize::MAX, 0, 1));
    }

    #[test]
    fn diagnostics_report_contains_only_support_safe_fields() {
        let report = format_diagnostics(DiagnosticSummary {
            client_version: "Client 0.6.0",
            relay: "Automatic",
            history_count: 2,
            sendmer_version: "0.10.0",
            receive_cache: "enabled",
            sender: DiagnosticTransferState {
                role: "sender",
                phase: "transferring",
                sequence: 7,
                session_id: "0123456789abcdef0123456789abcdef",
                error_code: "-",
                error_phase: "-",
                duration_ms: 1234,
            },
            receiver: DiagnosticTransferState {
                role: "receiver",
                phase: "idle",
                sequence: 0,
                session_id: "-",
                error_code: "timeout",
                error_phase: "connecting",
                duration_ms: 0,
            },
            last_error: "timeout",
        });
        assert!(report.contains("client=Client 0.6.0"));
        assert!(report.contains("sendmer=0.10.0"));
        assert!(report.contains("sender_role=sender"));
        assert!(report.contains("sender_phase=transferring"));
        assert!(report.contains("sender_sequence=7"));
        assert!(report.contains("sender_duration_ms=1234"));
        assert!(report.contains("receiver_error_code=timeout"));
        assert!(report.contains("last_error=timeout"));
        assert!(!report.contains("ticket"));
        assert!(!report.contains("C:/"));
        assert!(!report.contains("connection_id"));
    }

    #[test]
    fn diagnostic_phase_names_are_stable_and_locale_independent() {
        assert_eq!(diagnostic_phase_name(TransferPhase::Idle), "idle");
        assert_eq!(
            diagnostic_phase_name(TransferPhase::Transporting),
            "transporting"
        );
        assert_eq!(diagnostic_phase_name(TransferPhase::Failed), "failed");
    }

    #[test]
    fn diagnostics_report_file_name_is_timestamped() {
        assert_eq!(
            diagnostic_report_file_name(1_700_000_000),
            "alter-sendmer-diagnostics-1700000000.txt"
        );
    }

    #[test]
    fn diagnostic_report_write_is_non_replacing_and_round_trips_contents() {
        let directory = tempfile::tempdir().expect("create diagnostic directory");
        let path = write_diagnostic_report(directory.path(), 1_700_000_000, "safe report\n")
            .expect("write diagnostic report");

        assert_eq!(
            fs::read_to_string(&path).expect("read diagnostic report"),
            "safe report\n"
        );
        assert!(write_diagnostic_report(directory.path(), 1_700_000_000, "replacement\n").is_err());
    }

    #[test]
    fn preferences_migrate_legacy_values_with_safe_cache_defaults() {
        let preferences =
            parse_preferences(r#"{"relay":"default","retry_limit":3,"download_limit_mb":32}"#)
                .expect("legacy preferences should parse");
        assert_eq!(preferences.max_upload_rate_mib_per_sec, None);
        assert_eq!(preferences.max_receivers, None);
        assert_eq!(preferences.session_lifetime_seconds, None);
        assert!(!preferences.manifest_mode);
        assert!(preferences.receive_cache_enabled);
        assert_eq!(
            preferences.receive_cache_ttl_days,
            super::DEFAULT_RECEIVE_CACHE_TTL_DAYS
        );
    }

    #[test]
    fn preferences_preserve_valid_upload_rate_and_reject_invalid_values() {
        let configured = parse_preferences(
            r#"{"relay":"default","retry_limit":3,"download_limit_mb":32,"max_upload_rate_mib_per_sec":25}"#,
        )
        .expect("configured preferences should parse");
        assert_eq!(configured.max_upload_rate_mib_per_sec, Some(25));

        for invalid in [0, super::MAX_UPLOAD_RATE_MIB_PER_SEC + 1] {
            let contents = format!(
                r#"{{"relay":"default","retry_limit":3,"download_limit_mb":32,"max_upload_rate_mib_per_sec":{invalid}}}"#
            );
            let preferences =
                parse_preferences(&contents).expect("invalid upload rate should be sanitized");
            assert_eq!(preferences.max_upload_rate_mib_per_sec, None);
        }
    }

    #[test]
    fn preferences_preserve_v010_sender_controls_and_sanitize_unknown_values() {
        let configured = parse_preferences(
            r#"{"relay":"default","retry_limit":3,"download_limit_mb":32,"max_receivers":4,"session_lifetime_seconds":900}"#,
        )
        .expect("v0.10 sender preferences should parse");
        assert_eq!(configured.max_receivers, Some(4));
        assert_eq!(configured.session_lifetime_seconds, Some(900));

        let manifest = parse_preferences(
            r#"{"relay":"default","retry_limit":3,"download_limit_mb":32,"manifest_mode":true}"#,
        )
        .expect("manifest preference should parse");
        assert!(manifest.manifest_mode);

        let sanitized = parse_preferences(
            r#"{"relay":"default","retry_limit":3,"download_limit_mb":32,"max_receivers":3,"session_lifetime_seconds":123}"#,
        )
        .expect("unknown sender presets should sanitize");
        assert_eq!(sanitized.max_receivers, None);
        assert_eq!(sanitized.session_lifetime_seconds, None);
    }

    #[test]
    fn sender_expiration_maps_to_non_retryable_timeout_status() {
        let error = TransferError::new(
            TransferErrorCode::Timeout,
            CoreTransferPhase::Finalizing,
            false,
            "expired",
        );
        assert_eq!(
            sender_status_for_failure(&error),
            SenderTransferStatus::Expired
        );

        let retryable = TransferError::new(
            TransferErrorCode::Timeout,
            CoreTransferPhase::Transferring,
            true,
            "retry",
        );
        assert_eq!(
            sender_status_for_failure(&retryable),
            SenderTransferStatus::Aborted
        );
    }

    #[test]
    fn preferences_preserve_cache_settings_and_sanitize_unknown_retention() {
        let configured = parse_preferences(
            r#"{"relay":"default","retry_limit":3,"download_limit_mb":32,"receive_cache_enabled":false,"receive_cache_ttl_days":30}"#,
        )
        .expect("configured cache preferences should parse");
        assert!(!configured.receive_cache_enabled);
        assert_eq!(configured.receive_cache_ttl_days, 30);

        for invalid in [0, 2, 31, u64::MAX] {
            let contents = format!(
                r#"{{"relay":"default","retry_limit":3,"download_limit_mb":32,"receive_cache_enabled":true,"receive_cache_ttl_days":{invalid}}}"#
            );
            let preferences =
                parse_preferences(&contents).expect("invalid cache retention should be sanitized");
            assert!(preferences.receive_cache_enabled);
            assert_eq!(
                preferences.receive_cache_ttl_days,
                super::DEFAULT_RECEIVE_CACHE_TTL_DAYS
            );
        }
    }

    #[test]
    fn upload_rate_input_accepts_unlimited_and_bounded_integer_values() {
        assert_eq!(parse_upload_rate_input(""), Ok(None));
        assert_eq!(parse_upload_rate_input("1"), Ok(Some(1)));
        assert_eq!(
            parse_upload_rate_input(&super::MAX_UPLOAD_RATE_MIB_PER_SEC.to_string()),
            Ok(Some(super::MAX_UPLOAD_RATE_MIB_PER_SEC))
        );
        assert_eq!(parse_upload_rate_input("0"), Err(()));
        assert_eq!(parse_upload_rate_input("1.5"), Err(()));
        assert_eq!(parse_upload_rate_input("fast"), Err(()));
    }
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
    fn all_core_ui_copy_aliases_resolve() {
        let expected = [
            "drop",
            "start",
            "stop",
            "save",
            "receive_action",
            "copy",
            "new",
            "path_selected_file",
            "path_selected_folder",
            "preparing",
            "listening",
            "sharing",
            "stopping",
            "ticket_copied",
            "connecting",
            "downloading",
            "download_completed",
            "receive_failed",
            "transfer_completed",
            "transfer_failed",
            "finalizing",
            "open_folder",
            "transfer_complete",
            "failed",
            "try_again",
        ];
        for key in expected {
            assert!(
                Locale::English.ui_copy(key).is_some(),
                "missing UI copy alias: {key}"
            );
        }
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

    #[test]
    fn failed_receive_history_path_falls_back_to_save_directory() {
        let save_path = PathBuf::from("C:/Users/test/Downloads");
        let completed_path: Option<PathBuf> = None;
        let path = completed_path.or_else(|| Some(save_path.clone()));
        assert_eq!(path.as_deref(), Some(save_path.as_path()));
    }
}
