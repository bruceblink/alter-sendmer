//! sendmer 适配器的跨进程和配置回归测试。

use super::{
    MEBIBYTE, ReceiveTransferOptions, SendTransferOptions, max_receivers_from_preference,
    max_upload_rate_from_mib, receive_cache_options, session_lifetime_from_seconds, start_receive,
    start_send,
};
use async_channel::unbounded;
use iroh::EndpointAddr;
use iroh_blobs::{BlobFormat, Hash, ticket::BlobTicket};
use sendmer::{
    RelayModeOption, Role, SenderTransferStatus, TRANSFER_EVENT_SCHEMA_VERSION, TransferEvent,
    TransferEventData,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use tokio::sync::watch;

const WORKER_MODE: &str = "ALTER_SENDME_TRANSFER_WORKER";
const SOURCE_PATH: &str = "ALTER_SENDME_TRANSFER_SOURCE";
const TICKET_PATH: &str = "ALTER_SENDME_TRANSFER_TICKET";
const OUTPUT_PATH: &str = "ALTER_SENDME_TRANSFER_OUTPUT";
const MANIFEST_MODE: &str = "ALTER_SENDME_TRANSFER_MANIFEST";

/// Verifies the stable event envelope before the GPUI state machine consumes it.
fn assert_event_contract(events: &[(u64, TransferEvent)], role: Role) {
    assert!(!events.is_empty(), "adapter should forward events");
    assert!(events.iter().all(|(generation, _)| *generation == 41));
    let session_id = events[0].1.session_id.clone();
    let mut validator = sendmer::TransferEventStreamValidator::new();
    for (index, (_, event)) in events.iter().enumerate() {
        assert_eq!(event.schema_version, TRANSFER_EVENT_SCHEMA_VERSION);
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.sequence, index as u64 + 1);
        assert_eq!(event.role, role);
        validator
            .accept(event)
            .expect("valid upstream event contract");
    }
    assert!(matches!(events[0].1.event, TransferEventData::Started));
}

/// Runs the sender half in a separate OS process so iroh does not reject a self-connection.
#[test]
fn worker_sender() {
    if std::env::var(WORKER_MODE).ok().as_deref() != Some("sender") {
        return;
    }
    let source = std::env::var(SOURCE_PATH).expect("sender source path");
    let ticket_path = std::env::var(TICKET_PATH).expect("sender ticket path");
    let manifest_mode = std::env::var(MANIFEST_MODE).ok().as_deref() == Some("1");
    let runtime = tokio::runtime::Runtime::new().expect("create sender runtime");
    let (event_sender, event_receiver) = unbounded();
    let result = runtime
        .block_on(start_send(
            source.into(),
            event_sender,
            41,
            SendTransferOptions {
                relay_mode: RelayModeOption::Disabled,
                max_upload_rate_mib_per_sec: None,
                max_receivers: None,
                session_lifetime_seconds: None,
                manifest_mode,
            },
        ))
        .expect("start adapter sender");
    fs::write(&ticket_path, result.ticket().to_string()).expect("publish sender ticket");

    // 文件用例等待首次发送状态完成，再用原票据启动第二个接收进程。
    let completion = (!manifest_mode).then(|| {
        let mut status = result.subscribe_transfer_status();
        let marker = PathBuf::from(&ticket_path).with_extension("completed");
        runtime.spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(20),
                status.wait_for(|status| *status == SenderTransferStatus::Completed),
            )
            .await
            .expect("first transfer completion deadline")
            .expect("sender state must remain open");
            fs::write(marker, b"completed").expect("signal first transfer completion");
        })
    });

    // Keep the SendHandle alive until the receiver has finished and the parent closes stdin.
    let mut signal = String::new();
    std::io::stdin()
        .read_to_string(&mut signal)
        .expect("wait for sender shutdown signal");
    if let Some(completion) = completion {
        runtime.block_on(completion).expect("watch transfer status");
    }
    let mut events = Vec::new();
    while let Ok(event) = event_receiver.try_recv() {
        events.push(event);
    }
    assert!(
        events.iter().all(|(_, event)| !event.event.is_terminal()),
        "individual downloads must leave the sender event stream open"
    );
    runtime
        .block_on(result.close())
        .expect("shut down adapter sender");

    while let Ok(event) = event_receiver.try_recv() {
        events.push(event);
    }
    assert_event_contract(&events, Role::Sender);
    assert!(matches!(
        events.last().map(|(_, event)| &event.event),
        Some(TransferEventData::Completed)
    ));
}

/// Runs the receiver half in a separate OS process and verifies the adapter event stream.
#[test]
fn worker_receiver() {
    if std::env::var(WORKER_MODE).ok().as_deref() != Some("receiver") {
        return;
    }
    let ticket = fs::read_to_string(std::env::var(TICKET_PATH).expect("receiver ticket path"))
        .expect("read sender ticket");
    let output = PathBuf::from(std::env::var(OUTPUT_PATH).expect("receiver output path"));
    let receive_cache = receive_cache_options(true, output.join(".receive-cache"), 7)
        .expect("configure receive cache");
    let runtime = tokio::runtime::Runtime::new().expect("create receiver runtime");
    let (event_sender, event_receiver) = unbounded();
    let result = runtime
        .block_on(start_receive(
            ticket,
            output,
            event_sender,
            41,
            ReceiveTransferOptions {
                relay_mode: RelayModeOption::Disabled,
                retry_policy: Default::default(),
                receive_cache,
            },
            watch::channel(false).1,
        ))
        .expect("receive through adapter");
    assert!(
        result.file_path.exists(),
        "adapter receive output should exist"
    );

    let mut events = Vec::new();
    while let Ok(event) = event_receiver.try_recv() {
        events.push(event);
    }
    assert_event_contract(&events, Role::Receiver);
    assert!(matches!(
        events.last().map(|(_, event)| &event.event),
        Some(TransferEventData::Completed)
    ));
}

/// 核对实际桌面传输适配器。首次发送完成后，同一票据可服务第二个进程。
#[test]
fn adapter_transfers_a_file_and_forwards_transfer_events() {
    let source_dir = tempdir().expect("create source directory");
    let output_dir = tempdir().expect("create output directory");
    let source_file = source_dir.path().join("adapter-e2e.txt");
    let contents = b"GPUI transfer adapter end-to-end verification";
    let expected_hash = sha256_bytes(contents);
    fs::write(&source_file, contents).expect("write source fixture");

    let ticket_path = output_dir.path().join("ticket.txt");
    let test_binary = std::env::current_exe().expect("locate test binary");
    let mut sender = Command::new(&test_binary)
        .args(["--exact", "transfer::tests::worker_sender", "--nocapture"])
        .env(WORKER_MODE, "sender")
        .env(SOURCE_PATH, &source_file)
        .env(TICKET_PATH, &ticket_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn adapter sender worker");

    let deadline = Instant::now() + Duration::from_secs(60);
    while !ticket_path.exists() {
        assert!(Instant::now() < deadline, "sender did not publish a ticket");
        if let Some(status) = sender.try_wait().expect("poll sender worker") {
            panic!("sender worker exited early: {status}");
        }
        thread::sleep(Duration::from_millis(100));
    }

    for name in ["first", "second"] {
        let target = output_dir.path().join(name);
        fs::create_dir(&target).expect("create separate receive directory");
        let receiver_status = Command::new(&test_binary)
            .args(["--exact", "transfer::tests::worker_receiver", "--nocapture"])
            .env(WORKER_MODE, "receiver")
            .env(TICKET_PATH, &ticket_path)
            .env(OUTPUT_PATH, &target)
            .status()
            .expect("run adapter receiver worker");
        assert!(
            receiver_status.success(),
            "receiver worker failed: {receiver_status}"
        );
        let received = target.join("adapter-e2e.txt");
        assert_eq!(fs::read(received).expect("read received file"), contents);
        assert_eq!(
            sha256_file(&target.join("adapter-e2e.txt")),
            expected_hash,
            "receive output hash must match the source hash"
        );

        if name == "first" {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !ticket_path.with_extension("completed").exists() {
                assert!(
                    Instant::now() < deadline,
                    "first transfer status not reported"
                );
                if let Some(status) = sender.try_wait().expect("poll sender worker") {
                    panic!("sender worker exited after first download: {status}");
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    sender
        .stdin
        .take()
        .expect("sender worker stdin")
        .write_all(b"receiver finished\n")
        .expect("signal sender worker");
    let sender_status = sender.wait().expect("wait for sender worker");
    assert!(
        sender_status.success(),
        "sender worker failed: {sender_status}"
    );
}

/// Exercises the public TM1 switch across processes and verifies that an empty directory
/// and a supported file timestamp survive without the desktop reading manifest data.
#[test]
fn adapter_transfers_manifest_directory_with_empty_child() {
    let source_dir = tempdir().expect("create source directory");
    let output_dir = tempdir().expect("create output directory");
    let source_root = source_dir.path().join("manifest-share");
    let empty_child = source_root.join("empty-child");
    let source_file = source_root.join("readme.txt");
    fs::create_dir_all(&empty_child).expect("create empty manifest child");
    fs::write(&source_file, b"manifest adapter fixture").expect("write manifest fixture");
    let source_time = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&source_file)
        .expect("open manifest fixture")
        .set_modified(source_time)
        .expect("set manifest fixture timestamp");

    let ticket_path = output_dir.path().join("manifest-ticket.txt");
    let test_binary = std::env::current_exe().expect("locate test binary");
    let mut sender = Command::new(&test_binary)
        .args(["--exact", "transfer::tests::worker_sender", "--nocapture"])
        .env(WORKER_MODE, "sender")
        .env(SOURCE_PATH, &source_root)
        .env(TICKET_PATH, &ticket_path)
        .env(MANIFEST_MODE, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn manifest sender worker");

    let deadline = Instant::now() + Duration::from_secs(60);
    while !ticket_path.exists() {
        assert!(
            Instant::now() < deadline,
            "manifest sender did not publish a ticket"
        );
        if let Some(status) = sender.try_wait().expect("poll manifest sender worker") {
            panic!("manifest sender worker exited early: {status}");
        }
        thread::sleep(Duration::from_millis(100));
    }

    let receiver_status = Command::new(&test_binary)
        .args(["--exact", "transfer::tests::worker_receiver", "--nocapture"])
        .env(WORKER_MODE, "receiver")
        .env(TICKET_PATH, &ticket_path)
        .env(OUTPUT_PATH, output_dir.path())
        .status()
        .expect("run manifest receiver worker");
    assert!(
        receiver_status.success(),
        "manifest receiver worker failed: {receiver_status}"
    );

    sender
        .stdin
        .take()
        .expect("manifest sender worker stdin")
        .write_all(b"manifest receiver finished\n")
        .expect("signal manifest sender worker");
    let sender_status = sender.wait().expect("wait for manifest sender worker");
    assert!(
        sender_status.success(),
        "manifest sender worker failed: {sender_status}"
    );

    let received_root = output_dir.path().join("manifest-share");
    assert_eq!(
        fs::read(received_root.join("readme.txt")).expect("read received manifest file"),
        b"manifest adapter fixture"
    );
    assert!(
        received_root.join("empty-child").is_dir(),
        "TM1 should materialize the empty child directory"
    );
    let received_time = fs::metadata(received_root.join("readme.txt"))
        .expect("stat received manifest file")
        .modified()
        .expect("read received manifest timestamp");
    assert_eq!(
        received_time
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("received timestamp should be after epoch")
            .as_secs(),
        1_700_000_000
    );
}

/// Verifies that a caller cancellation reaches sendmer before a receive can connect.
#[test]
fn adapter_forwards_pre_cancelled_receive_and_cleans_staging() {
    let output_dir = tempdir().expect("create output directory");
    let ticket = BlobTicket::new(
        EndpointAddr::new(iroh::SecretKey::generate().public()),
        Hash::new(b"gpui-cancellation-test"),
        BlobFormat::HashSeq,
    );
    let staging_prefix = format!(".sendmer-recv-{}-", ticket.hash().to_hex());
    let before = temp_staging_dirs(&staging_prefix);
    let (event_sender, event_receiver) = unbounded();
    let (_, cancellation) = watch::channel(true);
    let runtime = tokio::runtime::Runtime::new().expect("create cancellation runtime");

    let error = runtime
        .block_on(start_receive(
            ticket.to_string(),
            output_dir.path().to_path_buf(),
            event_sender,
            7,
            ReceiveTransferOptions {
                relay_mode: RelayModeOption::Disabled,
                retry_policy: Default::default(),
                receive_cache: None,
            },
            cancellation,
        ))
        .expect_err("pre-cancelled receive should stop before connecting");

    assert_eq!(error.to_string(), "Operation cancelled");
    assert_eq!(temp_staging_dirs(&staging_prefix), before);
    let mut events = Vec::new();
    while let Ok(event) = event_receiver.try_recv() {
        events.push(event);
    }
    assert!(!events.is_empty());
    let session_id = events[0].1.session_id.clone();
    let mut validator = sendmer::TransferEventStreamValidator::new();
    for (_, event) in &events {
        validator
            .accept(event)
            .expect("valid cancellation event contract");
    }
    assert!(
        events
            .iter()
            .all(|(_, event)| { event.role == Role::Receiver && event.session_id == session_id })
    );
    assert!(matches!(
        events.last().map(|(_, event)| &event.event),
        Some(TransferEventData::Cancelled)
    ));
}

#[test]
fn receive_cache_preference_maps_to_sendmer_options() {
    let root = tempdir().expect("create cache root");
    let disabled = receive_cache_options(false, root.path().join("disabled"), u64::MAX)
        .expect("disabled cache should not validate unused settings");
    assert!(disabled.is_none());

    let cache_root = root.path().join("enabled");
    let enabled = receive_cache_options(true, cache_root.clone(), 30)
        .expect("enabled cache should map to sendmer options")
        .expect("enabled cache options");
    assert_eq!(enabled.root_dir, cache_root);
    assert_eq!(enabled.ttl, Duration::from_secs(30 * 24 * 60 * 60));
    assert!(receive_cache_options(true, root.path().join("invalid"), 0).is_err());
}

#[test]
fn upload_rate_preference_maps_to_sendmer_bytes_per_second() {
    assert_eq!(
        max_upload_rate_from_mib(None).expect("unlimited rate"),
        None
    );
    assert_eq!(
        max_upload_rate_from_mib(Some(25))
            .expect("valid rate")
            .expect("configured rate")
            .get(),
        25 * MEBIBYTE
    );
    assert!(max_upload_rate_from_mib(Some(0)).is_err());
    assert!(max_upload_rate_from_mib(Some(u64::MAX)).is_err());
}

#[test]
fn sender_preferences_map_to_v010_options() {
    assert_eq!(
        max_receivers_from_preference(None).expect("unlimited receivers"),
        None
    );
    assert_eq!(
        max_receivers_from_preference(Some(4))
            .expect("receiver limit")
            .expect("configured receiver limit")
            .get(),
        4
    );
    assert!(max_receivers_from_preference(Some(0)).is_err());

    assert_eq!(
        session_lifetime_from_seconds(Some(900))
            .expect("session lifetime")
            .expect("configured session lifetime"),
        Duration::from_secs(900)
    );
    assert!(session_lifetime_from_seconds(Some(0)).is_err());
    assert_eq!(
        session_lifetime_from_seconds(None).expect("unlimited lifetime"),
        None
    );
}

/// Lists sendmer receive staging directories so cancellation cleanup can be compared exactly.
fn temp_staging_dirs(prefix: &str) -> Vec<std::path::PathBuf> {
    let mut paths = std::fs::read_dir(std::env::temp_dir())
        .expect("read system temporary directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &std::path::Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).expect("read file for hash"));
    format!("{:x}", hasher.finalize())
}
