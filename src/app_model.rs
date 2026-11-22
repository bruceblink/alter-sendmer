//! AlterSendmer 桌面状态模型和传输诊断支持函数。

use crate::locale::Locale;
use sendmer::{
    Role, SenderTransferStatus, TransferError, TransferErrorCode, TransferEvent,
    TransferEventStreamValidator, TransferPhase as CoreTransferPhase,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) const MAX_TICKET_LEN: usize = 16_384;
pub(crate) const DEFAULT_UPLOAD_RATE_MIB_PER_SEC: u64 = 10;
pub(crate) const MAX_UPLOAD_RATE_MIB_PER_SEC: u64 = 10_240;
pub(crate) const DEFAULT_RECEIVE_CACHE_TTL_DAYS: u64 = 7;
pub(crate) const RECEIVE_CACHE_TTL_PRESETS: [u64; 3] = [1, 7, 30];
pub(crate) const MAX_RECEIVER_PRESETS: [Option<u64>; 5] =
    [None, Some(1), Some(2), Some(4), Some(8)];
pub(crate) const SESSION_LIFETIME_PRESETS: [Option<u64>; 4] =
    [None, Some(15 * 60), Some(60 * 60), Some(8 * 60 * 60)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Send,
    Receive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferPhase {
    Idle,
    Preparing,
    Sharing,
    Connecting,
    Metadata,
    Transporting,
    Exporting,
    Finalizing,
    Stopping,
    Completed,
    Failed,
}

// Keep the diagnostic contract explicit because Rust does not expose dependency versions at runtime.
pub(crate) const SENDMER_VERSION: &str = "0.11.0";

impl TransferPhase {
    pub(crate) fn is_active(self) -> bool {
        matches!(
            self,
            Self::Preparing
                | Self::Sharing
                | Self::Connecting
                | Self::Metadata
                | Self::Transporting
                | Self::Exporting
                | Self::Finalizing
                | Self::Stopping
        )
    }
}

pub(crate) fn state_label(phase: TransferPhase, locale: Locale) -> String {
    let key = match phase {
        TransferPhase::Idle => "state.idle",
        TransferPhase::Preparing => "state.preparing",
        TransferPhase::Sharing => "state.listening",
        TransferPhase::Connecting => "state.preparing",
        TransferPhase::Metadata => "state.metadata",
        TransferPhase::Transporting => "state.active",
        TransferPhase::Exporting => "state.exporting",
        TransferPhase::Finalizing => "state.finalizing",
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

pub(crate) fn transfer_error_code_name(code: TransferErrorCode) -> &'static str {
    match code {
        TransferErrorCode::InvalidInput => "invalid_input",
        TransferErrorCode::ConnectionFailed => "connection_failed",
        TransferErrorCode::Timeout => "timeout",
        TransferErrorCode::RemoteRejected => "remote_rejected",
        TransferErrorCode::TransferInterrupted => "transfer_interrupted",
        TransferErrorCode::TargetConflict => "target_conflict",
        TransferErrorCode::Filesystem => "filesystem",
        TransferErrorCode::Internal => "internal",
    }
}

pub(crate) fn transfer_error_locale_key(code: TransferErrorCode) -> &'static str {
    match code {
        TransferErrorCode::InvalidInput => "errors.invalidInput",
        TransferErrorCode::ConnectionFailed => "errors.connectionFailed",
        TransferErrorCode::Timeout => "errors.timeout",
        TransferErrorCode::RemoteRejected => "errors.remoteRejected",
        TransferErrorCode::TransferInterrupted => "errors.transferInterrupted",
        TransferErrorCode::TargetConflict => "errors.targetConflict",
        TransferErrorCode::Filesystem => "errors.filesystem",
        TransferErrorCode::Internal => "errors.internal",
    }
}

pub(crate) fn transfer_phase_name(phase: CoreTransferPhase) -> &'static str {
    match phase {
        CoreTransferPhase::Preparing => "preparing",
        CoreTransferPhase::Connecting => "connecting",
        CoreTransferPhase::Metadata => "metadata",
        CoreTransferPhase::Transferring => "transferring",
        CoreTransferPhase::Exporting => "exporting",
        CoreTransferPhase::Finalizing => "finalizing",
    }
}

/// Maps the public sender terminal error to the lifecycle status exposed by sendmer.
pub(crate) fn sender_status_for_failure(error: &TransferError) -> SenderTransferStatus {
    if error.code == TransferErrorCode::Timeout
        && error.phase == CoreTransferPhase::Finalizing
        && !error.retryable
    {
        SenderTransferStatus::Expired
    } else {
        SenderTransferStatus::Aborted
    }
}

/// Holds only the fields that are safe to display or export for support.
pub(crate) struct DiagnosticSummary<'a> {
    pub(crate) client_version: &'a str,
    pub(crate) relay: &'a str,
    pub(crate) history_count: usize,
    pub(crate) sendmer_version: &'a str,
    pub(crate) receive_cache: &'a str,
    pub(crate) sender: DiagnosticTransferState<'a>,
    pub(crate) receiver: DiagnosticTransferState<'a>,
    pub(crate) last_error: &'a str,
}

/// Holds stable, support-safe fields for one sender or receiver event stream.
pub(crate) struct DiagnosticTransferState<'a> {
    pub(crate) role: &'a str,
    pub(crate) phase: &'a str,
    pub(crate) sequence: u64,
    pub(crate) session_id: &'a str,
    pub(crate) error_code: &'a str,
    pub(crate) error_phase: &'a str,
    pub(crate) duration_ms: u128,
}

/// Format the support-safe diagnostic fields that can be displayed or exported.
pub(crate) fn format_diagnostics(summary: DiagnosticSummary<'_>) -> String {
    format!(
        "client={} | relay={} | history_entries={} | sendmer={} | receive_cache={} | sender_role={} | sender_phase={} | sender_sequence={} | sender_session={} | sender_error_code={} | sender_error_phase={} | sender_duration_ms={} | receiver_role={} | receiver_phase={} | receiver_sequence={} | receiver_session={} | receiver_error_code={} | receiver_error_phase={} | receiver_duration_ms={} | last_error={}",
        summary.client_version,
        summary.relay,
        summary.history_count,
        summary.sendmer_version,
        summary.receive_cache,
        summary.sender.role,
        summary.sender.phase,
        summary.sender.sequence,
        summary.sender.session_id,
        summary.sender.error_code,
        summary.sender.error_phase,
        summary.sender.duration_ms,
        summary.receiver.role,
        summary.receiver.phase,
        summary.receiver.sequence,
        summary.receiver.session_id,
        summary.receiver.error_code,
        summary.receiver.error_phase,
        summary.receiver.duration_ms,
        summary.last_error,
    )
}

/// Maps the desktop lifecycle phase to a stable, locale-independent diagnostic value.
pub(crate) fn diagnostic_phase_name(phase: TransferPhase) -> &'static str {
    match phase {
        TransferPhase::Idle => "idle",
        TransferPhase::Preparing => "preparing",
        TransferPhase::Sharing => "sharing",
        TransferPhase::Connecting => "connecting",
        TransferPhase::Metadata => "metadata",
        TransferPhase::Transporting => "transporting",
        TransferPhase::Exporting => "exporting",
        TransferPhase::Finalizing => "finalizing",
        TransferPhase::Stopping => "stopping",
        TransferPhase::Completed => "completed",
        TransferPhase::Failed => "failed",
    }
}

pub(crate) fn diagnostic_report_file_name(timestamp: u64) -> String {
    format!("alter-sendmer-diagnostics-{timestamp}.txt")
}

/// Write one diagnostic report without replacing an existing report with the same timestamp.
pub(crate) fn write_diagnostic_report(
    directory: &Path,
    timestamp: u64,
    report: &str,
) -> std::io::Result<PathBuf> {
    let destination = directory.join(diagnostic_report_file_name(timestamp));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)?;
    file.write_all(report.as_bytes())?;
    Ok(destination)
}

pub(crate) fn ui_phase_for_core(role: Role, phase: CoreTransferPhase) -> TransferPhase {
    match role {
        Role::Sender => match phase {
            CoreTransferPhase::Preparing => TransferPhase::Preparing,
            CoreTransferPhase::Connecting => TransferPhase::Sharing,
            CoreTransferPhase::Metadata => TransferPhase::Metadata,
            CoreTransferPhase::Transferring => TransferPhase::Transporting,
            CoreTransferPhase::Exporting => TransferPhase::Exporting,
            CoreTransferPhase::Finalizing => TransferPhase::Finalizing,
        },
        Role::Receiver => match phase {
            CoreTransferPhase::Preparing => TransferPhase::Preparing,
            CoreTransferPhase::Connecting => TransferPhase::Connecting,
            CoreTransferPhase::Metadata => TransferPhase::Metadata,
            CoreTransferPhase::Transferring => TransferPhase::Transporting,
            CoreTransferPhase::Exporting => TransferPhase::Exporting,
            CoreTransferPhase::Finalizing => TransferPhase::Finalizing,
        },
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Progress {
    pub(crate) processed: u64,
    pub(crate) total: u64,
    pub(crate) speed: f64,
    pub(crate) current_name: Option<String>,
    pub(crate) completed_files: u32,
    pub(crate) total_files: u32,
}

impl Progress {
    pub(crate) fn percentage(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            ((self.processed as f32 / self.total as f32) * 100.0).clamp(0.0, 100.0)
        }
    }

    pub(crate) fn files_label(&self, unit: &str) -> String {
        if self.total_files == 0 {
            return String::new();
        }
        format!("{}/{} {unit}", self.completed_files, self.total_files)
    }

    /// Formats the byte counters and current throughput for the active transfer view.
    pub(crate) fn transfer_stats_label(&self) -> String {
        format!(
            "{} / {} · {}",
            format_bytes(self.processed),
            format_bytes(self.total),
            format_bytes_per_second(self.speed),
        )
    }
}

/// Uses decimal units so the displayed value follows the speed unit used by sendmer events.
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1_000.0 && unit_index < UNITS.len() - 1 {
        value /= 1_000.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit_index])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit_index])
    } else {
        format!("{value:.2} {}", UNITS[unit_index])
    }
}

/// Converts sendmer's floating point throughput into a stable user-facing rate string.
pub(crate) fn format_bytes_per_second(speed: f64) -> String {
    if !speed.is_finite() || speed <= 0.0 {
        return "0 B/s".to_owned();
    }
    format!("{}/s", format_bytes(speed.round() as u64))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HistoryEntry {
    pub(crate) role: String,
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) duration_ms: u128,
    pub(crate) speed: u64,
    pub(crate) date: String,
    pub(crate) outcome: String,
    pub(crate) ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error_phase: Option<String>,
}

/// Tracks the public event contract for one role without retaining tickets or local paths.
#[derive(Clone, Debug, Default)]
pub(crate) struct EventSessionState {
    pub(crate) validator: TransferEventStreamValidator,
    pub(crate) session_id: Option<String>,
    pub(crate) sequence: u64,
    pub(crate) error_code: Option<String>,
    pub(crate) error_phase: Option<String>,
    pub(crate) retryable: Option<bool>,
    pub(crate) duration_ms: u128,
}

impl EventSessionState {
    /// Accepts the schema's started-at-one stream and rejects gaps, duplicates, and late events.
    pub(crate) fn accept(&mut self, event: &TransferEvent) -> bool {
        if self.validator.accept(event).is_err() {
            return false;
        }
        self.session_id = Some(event.session_id.as_str().to_owned());
        self.sequence = event.sequence;
        true
    }
}

pub(crate) const fn default_receive_cache_enabled() -> bool {
    true
}

pub(crate) const fn default_receive_cache_ttl_days() -> u64 {
    DEFAULT_RECEIVE_CACHE_TTL_DAYS
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Preferences {
    pub(crate) relay: String,
    pub(crate) retry_limit: u32,
    pub(crate) download_limit_mb: u32,
    #[serde(default)]
    pub(crate) max_upload_rate_mib_per_sec: Option<u64>,
    #[serde(default)]
    pub(crate) max_receivers: Option<u64>,
    #[serde(default)]
    pub(crate) session_lifetime_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) manifest_mode: bool,
    #[serde(default = "default_receive_cache_enabled")]
    pub(crate) receive_cache_enabled: bool,
    #[serde(default = "default_receive_cache_ttl_days")]
    pub(crate) receive_cache_ttl_days: u64,
}
