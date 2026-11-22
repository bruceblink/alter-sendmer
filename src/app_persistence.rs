//! AlterSendmer 的本地路径、偏好、历史和缓存持久化。

use super::app_model::*;
use super::recovery::RecoverySnapshot;
use crate::locale::Locale;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) fn path_size(path: &PathBuf) -> u64 {
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

/// Counts regular files represented by a send path for the folder progress label.
pub(crate) fn path_file_count(path: &PathBuf) -> u32 {
    if path.is_file() {
        return 1;
    }
    if path.is_dir() {
        return walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
    }
    0
}

pub(crate) fn history_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.data_dir().join("history.json"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-history.json"))
}

pub(crate) fn theme_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("theme"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-theme"))
}

pub(crate) fn locale_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("locale"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-locale"))
}

/// Loads a previously selected locale while falling back to English for invalid or missing data.
pub(crate) fn load_locale() -> Locale {
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
pub(crate) fn persist_locale(locale: Locale) {
    let path = locale_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, locale.code());
}

pub(crate) fn preferences_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.config_dir().join("preferences.json"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-preferences.json"))
}

pub(crate) fn receive_cache_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.cache_dir().join("receive-cache"))
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join("AlterSendme")
                .join("receive-cache")
        })
}

pub(crate) fn recovery_path() -> PathBuf {
    directories::ProjectDirs::from("com", "AlterSendme", "AlterSendme")
        .map(|dirs| dirs.data_dir().join("recovery.json"))
        .unwrap_or_else(|| PathBuf::from("alter-sendme-recovery.json"))
}

/// 以同目录临时文件加替换写入恢复状态，避免进程中断留下半份 JSON。
pub(crate) fn persist_recovery(snapshot: &RecoverySnapshot) -> io::Result<()> {
    let path = recovery_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temporary = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("recovery"),
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&contents)?;
    file.sync_all()?;
    replace_file(&temporary, &path)
}

/// 在启动时读取并校验恢复状态。损坏、过期或不属于当前格式的文件会被忽略。
pub(crate) fn load_recovery() -> Option<RecoverySnapshot> {
    let contents = fs::read_to_string(recovery_path()).ok()?;
    let mut snapshot = serde_json::from_str::<RecoverySnapshot>(&contents).ok()?;
    snapshot.normalize_loaded().then_some(snapshot)
}

/// 删除新会话不再需要的恢复快照，保留其它缓存和历史记录。
pub(crate) fn clear_recovery() -> io::Result<()> {
    match fs::remove_file(recovery_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // Windows 不允许 rename 覆盖现有文件，先移除旧文件再完成同目录替换。
            fs::remove_file(destination)?;
            fs::rename(temporary, destination)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn default_preferences() -> Preferences {
    Preferences {
        relay: "default".to_owned(),
        retry_limit: 3,
        download_limit_mb: 32,
        max_upload_rate_mib_per_sec: None,
        max_receivers: None,
        session_lifetime_seconds: None,
        manifest_mode: false,
        receive_cache_enabled: default_receive_cache_enabled(),
        receive_cache_ttl_days: default_receive_cache_ttl_days(),
    }
}

pub(crate) fn load_preferences() -> Preferences {
    fs::read_to_string(preferences_path())
        .ok()
        .and_then(|contents| parse_preferences(&contents))
        .unwrap_or_else(default_preferences)
}

/// Decodes persisted preferences while treating an invalid upload limit as unrestricted.
pub(crate) fn parse_preferences(contents: &str) -> Option<Preferences> {
    let mut preferences = serde_json::from_str::<Preferences>(contents).ok()?;
    if !matches!(
        preferences.max_upload_rate_mib_per_sec,
        None | Some(1..=MAX_UPLOAD_RATE_MIB_PER_SEC)
    ) {
        preferences.max_upload_rate_mib_per_sec = None;
    }
    if !MAX_RECEIVER_PRESETS.contains(&preferences.max_receivers) {
        preferences.max_receivers = None;
    }
    if !SESSION_LIFETIME_PRESETS.contains(&preferences.session_lifetime_seconds) {
        preferences.session_lifetime_seconds = None;
    }
    if !RECEIVE_CACHE_TTL_PRESETS.contains(&preferences.receive_cache_ttl_days) {
        preferences.receive_cache_ttl_days = DEFAULT_RECEIVE_CACHE_TTL_DAYS;
    }
    Some(preferences)
}

/// Parses the editable MiB/s field; an empty field is the explicit unrestricted mode.
pub(crate) fn parse_upload_rate_input(input: &str) -> Result<Option<u64>, ()> {
    if input.is_empty() {
        return Ok(None);
    }
    let rate = input.parse::<u64>().map_err(|_| ())?;
    (1..=MAX_UPLOAD_RATE_MIB_PER_SEC)
        .contains(&rate)
        .then_some(Some(rate))
        .ok_or(())
}

pub(crate) fn persist_preferences(preferences: &Preferences) {
    let path = preferences_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(preferences) {
        let _ = fs::write(path, contents);
    }
}

pub(crate) fn load_theme() -> Theme {
    match fs::read_to_string(theme_path()).ok().as_deref() {
        Some("dark") => Theme::Dark,
        Some("light") => Theme::Light,
        _ => Theme::System,
    }
}

pub(crate) fn persist_theme(theme: Theme) {
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

pub(crate) fn load_history() -> Vec<HistoryEntry> {
    let mut entries: Vec<HistoryEntry> = fs::read_to_string(history_path())
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default();
    let original = entries.clone();
    for entry in &mut entries {
        sanitize_history_entry(entry);
    }
    if entries != original {
        persist_history(&entries);
    }
    entries
}

pub(crate) fn persist_history(entries: &[HistoryEntry]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut safe_entries = entries.to_vec();
    for entry in &mut safe_entries {
        sanitize_history_entry(entry);
    }
    if let Ok(contents) = serde_json::to_string_pretty(&safe_entries) {
        let _ = fs::write(path, contents);
    }
}

/// 只保留历史记录需要的显示名称，并清除旧版本保存的 bearer ticket。
pub(crate) fn sanitize_history_entry(entry: &mut HistoryEntry) {
    entry.path = history_display_name(&entry.path);
    entry.ticket = None;
}

/// 从本地路径或旧版本路径字符串中提取不含目录的显示名称。
pub(crate) fn history_display_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(crate) fn ticket_is_valid(ticket: &str) -> bool {
    ticket.parse::<iroh_blobs::ticket::BlobTicket>().is_ok()
}

/// Checks the resulting UTF-8 byte length of a ticket replacement without allowing arithmetic
/// overflow; callers can reject an edit before mutating the visible input value.
pub(crate) fn ticket_replacement_fits_limit(
    current_len: usize,
    replaced_len: usize,
    inserted_len: usize,
) -> bool {
    let Some(remaining_len) = current_len.checked_sub(replaced_len) else {
        return false;
    };
    let Some(result_len) = remaining_len.checked_add(inserted_len) else {
        return false;
    };
    result_len <= MAX_TICKET_LEN
}

#[cfg(test)]
mod tests {
    use super::replace_file;
    use std::fs;

    #[test]
    fn recovery_replace_file_leaves_complete_json() {
        let directory = tempfile::tempdir().expect("create persistence fixture");
        let destination = directory.path().join("recovery.json");
        let temporary = directory.path().join(".recovery.json.tmp");
        fs::write(&destination, b"old").expect("write old snapshot");
        fs::write(&temporary, b"{\"state\":\"completed\"}").expect("write temporary snapshot");
        replace_file(&temporary, &destination).expect("replace snapshot");
        assert_eq!(
            fs::read_to_string(destination).expect("read snapshot"),
            "{\"state\":\"completed\"}"
        );
        assert!(!temporary.exists());
    }
}
