//! 传输 manifest 摘要和恢复状态记录。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const RECOVERY_SCHEMA_VERSION: u16 = 2;
pub(crate) const RECOVERY_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
const RECOVERY_PROGRESS_PERSIST_BYTES: u64 = RECOVERY_CHUNK_SIZE;

/// 记录 sendmer manifest 的桌面可见摘要，实际目录和分块校验仍由核心传输层负责。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ManifestSummary {
    pub(crate) schema_version: u16,
    pub(crate) manifest_mode: bool,
    pub(crate) file_count: u32,
    pub(crate) total_bytes: u64,
    pub(crate) chunk_size: u64,
    pub(crate) chunk_count: u64,
    pub(crate) digest: String,
}

impl ManifestSummary {
    /// 从发送路径生成稳定摘要，核心传输仍负责实际 manifest 和分块校验。
    pub(crate) fn from_path(path: &Path, manifest_mode: bool) -> Self {
        let mut entries = Vec::new();
        if path.is_file() {
            if let Ok(metadata) = std::fs::metadata(path) {
                entries.push((
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string()),
                    metadata.len(),
                ));
            }
        } else if path.is_dir() {
            for entry in walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(Result::ok)
            {
                if entry.file_type().is_file() {
                    let relative = entry
                        .path()
                        .strip_prefix(path)
                        .unwrap_or(entry.path())
                        .to_string_lossy()
                        .to_string();
                    let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                    entries.push((Some(relative), size));
                }
            }
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let total_bytes = entries.iter().map(|(_, size)| *size).sum();
        let chunk_count = entries
            .iter()
            .map(|(_, size)| size.saturating_add(RECOVERY_CHUNK_SIZE - 1) / RECOVERY_CHUNK_SIZE)
            .sum();
        let digest = manifest_digest(&entries, manifest_mode);
        Self {
            schema_version: RECOVERY_SCHEMA_VERSION,
            manifest_mode,
            file_count: entries.len().try_into().unwrap_or(u32::MAX),
            total_bytes,
            chunk_size: RECOVERY_CHUNK_SIZE,
            chunk_count,
            digest,
        }
    }

    /// 为接收端建立不包含本地路径的文件数量摘要，大小和分块摘要由核心事件补充。
    pub(crate) fn from_received_file_names(file_count: usize) -> Self {
        Self {
            schema_version: RECOVERY_SCHEMA_VERSION,
            file_count: file_count.try_into().unwrap_or(u32::MAX),
            chunk_size: RECOVERY_CHUNK_SIZE,
            ..Self::default()
        }
    }
}

fn manifest_digest(entries: &[(Option<String>, u64)], manifest_mode: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update([RECOVERY_SCHEMA_VERSION as u8, manifest_mode as u8]);
    for (name, size) in entries {
        hasher.update(name.as_deref().unwrap_or_default().as_bytes());
        hasher.update(size.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// 持久化一次传输的恢复字段。`verified_bytes` 只在核心终态事件确认后填写。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RecoverySnapshot {
    pub(crate) schema_version: u16,
    pub(crate) role: String,
    pub(crate) session_id: Option<String>,
    pub(crate) manifest: Option<ManifestSummary>,
    #[serde(rename = "processed_bytes", alias = "processed")]
    pub(crate) processed_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verified_bytes: Option<u64>,
    #[serde(rename = "total_bytes", alias = "total")]
    pub(crate) total_bytes: u64,
    pub(crate) speed_bytes_per_sec: f64,
    pub(crate) attempts: u32,
    #[serde(rename = "manual_retries", alias = "reconnects")]
    pub(crate) manual_retries: u32,
    /// 核心事件未公开内部自动重连次数，未知时保持 None，不冒充人工重试。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) protocol_reconnects: Option<u32>,
    /// 持久化格式使用 state，旧版本的 status 字段仍可读取。
    #[serde(rename = "state", alias = "status")]
    pub(crate) status: String,
    pub(crate) started_at: Option<u64>,
    pub(crate) updated_at: Option<u64>,
    pub(crate) error: Option<String>,
    /// 该字段只在进程内使用，防止新会话误继承旧的重试计数。
    #[serde(skip)]
    pending_retry: bool,
    /// 该字段只在进程内使用，用于按字节阈值降低恢复文件写入频率。
    #[serde(skip)]
    last_persisted_processed: u64,
}

impl RecoverySnapshot {
    /// 判断事件是否属于当前恢复会话，避免旧会话事件覆盖新快照。
    pub(crate) fn matches_session(&self, role: &str, session_id: &str) -> bool {
        self.role == role && self.session_id.as_deref() == Some(session_id)
    }

    /// 开始一个新会话。只有显式重试才沿用当前会话的人工重试计数。
    pub(crate) fn begin(
        &mut self,
        role: &str,
        session_id: String,
        manifest: Option<ManifestSummary>,
    ) {
        let continuing = self.pending_retry && self.role == role && self.session_id.is_some();
        let attempts = 1;
        let manual_retries = if continuing { self.manual_retries } else { 0 };
        self.schema_version = RECOVERY_SCHEMA_VERSION;
        self.role = role.to_owned();
        self.session_id = Some(session_id);
        self.manifest = manifest;
        self.processed_bytes = 0;
        self.verified_bytes = None;
        self.total_bytes = 0;
        self.speed_bytes_per_sec = 0.0;
        self.attempts = attempts;
        self.manual_retries = manual_retries;
        self.protocol_reconnects = None;
        self.status = if continuing {
            "recovering"
        } else {
            "connecting"
        }
        .to_owned();
        self.started_at = Some(unix_timestamp());
        self.updated_at = self.started_at;
        self.error = None;
        self.pending_retry = false;
        self.last_persisted_processed = 0;
    }

    /// 记录已传输字节。该事件只表示进度，不把已处理字节误标为已校验字节。
    pub(crate) fn update_progress(&mut self, processed: u64, total: u64, speed: f64) {
        self.processed_bytes = processed;
        self.total_bytes = total;
        self.speed_bytes_per_sec = speed;
        self.status = "transferring".to_owned();
        self.updated_at = Some(unix_timestamp());
    }

    /// 标记一次人工重试，下一次 Started 事件会按同一恢复链保留该计数。
    pub(crate) fn mark_retry(&mut self) {
        self.manual_retries = self.manual_retries.saturating_add(1);
        self.pending_retry = true;
        self.status = "recovering".to_owned();
        self.updated_at = Some(unix_timestamp());
    }

    pub(crate) fn complete(&mut self) {
        self.status = "completed".to_owned();
        self.verified_bytes = Some(self.total_bytes.max(self.processed_bytes));
        self.processed_bytes = self
            .processed_bytes
            .max(self.verified_bytes.unwrap_or_default());
        self.updated_at = Some(unix_timestamp());
    }

    pub(crate) fn fail(&mut self, error: String) {
        self.status = "failed".to_owned();
        self.error = Some(error);
        self.updated_at = Some(unix_timestamp());
    }

    pub(crate) fn cancel(&mut self) {
        self.status = "cancelled".to_owned();
        self.updated_at = Some(unix_timestamp());
    }

    /// 传输中的快照按分块阈值写入，终态和连接状态始终立即写入。
    pub(crate) fn should_persist(&self) -> bool {
        self.status != "transferring"
            || self
                .processed_bytes
                .saturating_sub(self.last_persisted_processed)
                >= RECOVERY_PROGRESS_PERSIST_BYTES
    }

    pub(crate) fn mark_persisted(&mut self) {
        self.last_persisted_processed = self.processed_bytes;
    }

    /// 校验启动时读取的快照，并迁移 v1 中错误的进度校验语义。
    pub(crate) fn normalize_loaded(&mut self) -> bool {
        if self.schema_version == 0 || self.schema_version > RECOVERY_SCHEMA_VERSION {
            return false;
        }
        if !matches!(self.role.as_str(), "sender" | "receiver")
            || self.session_id.as_deref().is_none_or(str::is_empty)
            || !matches!(
                self.status.as_str(),
                "connecting" | "recovering" | "transferring" | "completed" | "failed" | "cancelled"
            )
            || !self.speed_bytes_per_sec.is_finite()
            || self.speed_bytes_per_sec < 0.0
            || self
                .verified_bytes
                .is_some_and(|value| value > self.total_bytes)
        {
            return false;
        }
        if self.schema_version < RECOVERY_SCHEMA_VERSION {
            if self.status != "completed" {
                self.verified_bytes = None;
            }
            self.schema_version = RECOVERY_SCHEMA_VERSION;
        }
        self.pending_retry = false;
        self.last_persisted_processed = self.processed_bytes;
        true
    }

    pub(crate) fn label(&self) -> String {
        let verification = self
            .verified_bytes
            .map(|bytes| format!(" · {bytes}/{} verified", self.total_bytes))
            .unwrap_or_default();
        format!(
            "{} · {}/{} processed · {} manual retries{}",
            self.status, self.processed_bytes, self.total_bytes, self.manual_retries, verification,
        )
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{ManifestSummary, RECOVERY_CHUNK_SIZE, RecoverySnapshot};
    use std::fs;

    #[test]
    fn manifest_summary_counts_files_and_chunks() {
        let root = tempfile::tempdir().expect("create manifest fixture");
        fs::write(root.path().join("small.txt"), b"small").expect("write fixture");
        let summary = ManifestSummary::from_path(root.path(), true);
        assert_eq!(summary.file_count, 1);
        assert_eq!(summary.total_bytes, 5);
        assert_eq!(summary.chunk_size, RECOVERY_CHUNK_SIZE);
        assert_eq!(summary.chunk_count, 1);
        assert!(!summary.digest.is_empty());
    }

    #[test]
    fn receiver_manifest_summary_keeps_only_file_count() {
        let summary = ManifestSummary::from_received_file_names(2);
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.total_bytes, 0);
        assert!(summary.digest.is_empty());
    }

    #[test]
    fn recovery_snapshot_distinguishes_processed_and_verified_bytes() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("receiver", "session-1".to_owned(), None);
        snapshot.update_progress(4, 10, 2.0);
        assert_eq!(snapshot.processed_bytes, 4);
        assert_eq!(snapshot.verified_bytes, None);
        snapshot.complete();
        assert_eq!(snapshot.verified_bytes, Some(10));
        assert!(snapshot.label().contains("processed"));
    }

    #[test]
    fn new_session_resets_attempts_and_manual_retries() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("receiver", "session-1".to_owned(), None);
        snapshot.begin("receiver", "session-2".to_owned(), None);
        assert_eq!(snapshot.attempts, 1);
        assert_eq!(snapshot.manual_retries, 0);
        assert_eq!(snapshot.status, "connecting");
    }

    #[test]
    fn explicit_retry_carries_only_the_recovery_chain() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("receiver", "session-1".to_owned(), None);
        snapshot.mark_retry();
        snapshot.begin("receiver", "session-2".to_owned(), None);
        assert_eq!(snapshot.attempts, 1);
        assert_eq!(snapshot.manual_retries, 1);
        assert_eq!(snapshot.status, "recovering");
        snapshot.mark_retry();
        snapshot.begin("receiver", "session-3".to_owned(), None);
        assert_eq!(snapshot.attempts, 1);
        assert_eq!(snapshot.manual_retries, 2);
    }

    #[test]
    fn old_snapshot_status_and_field_names_are_migrated() {
        let mut snapshot: RecoverySnapshot = serde_json::from_str(
            r#"{"schema_version":1,"role":"receiver","session_id":"old","processed":4,"verified_bytes":4,"total":10,"speed_bytes_per_sec":1.0,"attempts":2,"reconnects":1,"status":"transferring"}"#,
        )
        .expect("parse old snapshot");
        assert!(snapshot.normalize_loaded());
        assert_eq!(snapshot.processed_bytes, 4);
        assert_eq!(snapshot.total_bytes, 10);
        assert_eq!(snapshot.manual_retries, 1);
        assert_eq!(snapshot.verified_bytes, None);
        assert_eq!(snapshot.schema_version, 2);
    }

    #[test]
    fn v2_snapshot_uses_state_and_byte_field_names() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("sender", "session-2".to_owned(), None);
        snapshot.update_progress(4, 10, 1.0);
        let value = serde_json::to_value(snapshot).expect("serialize snapshot");
        let object = value.as_object().expect("snapshot object");
        assert!(object.contains_key("state"));
        assert!(object.contains_key("processed_bytes"));
        assert!(object.contains_key("total_bytes"));
        assert!(object.contains_key("manual_retries"));
        assert!(!object.contains_key("status"));
        assert!(!object.contains_key("processed"));
        assert!(!object.contains_key("reconnects"));
        assert!(!object.contains_key("verified_bytes"));
    }

    #[test]
    fn progress_persistence_is_thresholded_until_terminal_state() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("sender", "session-3".to_owned(), None);
        snapshot.mark_persisted();
        snapshot.update_progress(1, 10, 1.0);
        assert!(!snapshot.should_persist());
        snapshot.update_progress(RECOVERY_CHUNK_SIZE, 10, 1.0);
        assert!(snapshot.should_persist());
        snapshot.mark_persisted();
        snapshot.complete();
        assert!(snapshot.should_persist());
    }

    #[test]
    fn session_binding_rejects_stale_events() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("receiver", "session-4".to_owned(), None);
        assert!(snapshot.matches_session("receiver", "session-4"));
        assert!(!snapshot.matches_session("receiver", "session-old"));
        assert!(!snapshot.matches_session("sender", "session-4"));
    }
}
