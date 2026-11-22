//! 传输 manifest 摘要和恢复状态记录。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const RECOVERY_SCHEMA_VERSION: u16 = 1;
pub(crate) const RECOVERY_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

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

/// 持久化一次传输的安全恢复字段，供界面、诊断和弱网验收读取。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct RecoverySnapshot {
    pub(crate) schema_version: u16,
    pub(crate) role: String,
    pub(crate) session_id: Option<String>,
    pub(crate) manifest: Option<ManifestSummary>,
    pub(crate) processed: u64,
    pub(crate) verified_bytes: u64,
    pub(crate) total: u64,
    pub(crate) speed_bytes_per_sec: f64,
    pub(crate) attempts: u32,
    pub(crate) reconnects: u32,
    pub(crate) status: String,
    pub(crate) started_at: Option<u64>,
    pub(crate) updated_at: Option<u64>,
    pub(crate) error: Option<String>,
}

impl RecoverySnapshot {
    pub(crate) fn begin(
        &mut self,
        role: &str,
        session_id: String,
        manifest: Option<ManifestSummary>,
    ) {
        let reconnects = self.reconnects;
        self.schema_version = RECOVERY_SCHEMA_VERSION;
        self.role = role.to_owned();
        self.session_id = Some(session_id);
        self.manifest = manifest;
        self.processed = 0;
        self.verified_bytes = 0;
        self.total = 0;
        self.speed_bytes_per_sec = 0.0;
        self.attempts = self.attempts.saturating_add(1);
        self.reconnects = reconnects;
        self.status = if reconnects > 0 {
            "recovering"
        } else {
            "connecting"
        }
        .to_owned();
        self.started_at = Some(unix_timestamp());
        self.updated_at = self.started_at;
        self.error = None;
    }

    pub(crate) fn update_progress(&mut self, processed: u64, total: u64, speed: f64) {
        self.processed = processed;
        self.verified_bytes = processed;
        self.total = total;
        self.speed_bytes_per_sec = speed;
        self.status = "transferring".to_owned();
        self.updated_at = Some(unix_timestamp());
    }

    pub(crate) fn mark_retry(&mut self) {
        self.reconnects = self.reconnects.saturating_add(1);
        self.status = "recovering".to_owned();
        self.updated_at = Some(unix_timestamp());
    }

    pub(crate) fn complete(&mut self) {
        self.status = "completed".to_owned();
        self.verified_bytes = self.total.max(self.verified_bytes);
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

    pub(crate) fn label(&self) -> String {
        format!(
            "{} · {}/{} verified · {} reconnects",
            self.status, self.verified_bytes, self.total, self.reconnects
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
    fn recovery_snapshot_tracks_progress_and_completion() {
        let mut snapshot = RecoverySnapshot::default();
        snapshot.begin("receiver", "session-1".to_owned(), None);
        snapshot.update_progress(4, 10, 2.0);
        snapshot.mark_retry();
        snapshot.complete();
        assert_eq!(snapshot.status, "completed");
        assert_eq!(snapshot.verified_bytes, 10);
        assert_eq!(snapshot.reconnects, 1);
        assert!(snapshot.label().contains("reconnects"));
    }
}
