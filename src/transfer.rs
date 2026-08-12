//! Async sendmer integration kept separate from the GPUI view state.

use async_channel::Sender;
use sendmer::{
    AddrInfoOptions, AppHandle, EventEmitter, ReceiveOptions, RelayModeOption, SendOptions,
    TransferEvent, send,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::watch;

/// Non-blocking adapter from sendmer events to the GPUI event loop.
#[derive(Clone)]
pub struct ChannelEventEmitter {
    pub sender: Sender<(u64, TransferEvent)>,
    pub generation: u64,
}

impl EventEmitter for ChannelEventEmitter {
    fn emit(&self, event: &TransferEvent) {
        let _ = self.sender.try_send((self.generation, event.clone()));
    }
}

pub fn emitter(sender: Sender<(u64, TransferEvent)>, generation: u64) -> AppHandle {
    Some(Arc::new(ChannelEventEmitter { sender, generation }))
}

pub async fn start_send(
    path: PathBuf,
    sender: Sender<(u64, TransferEvent)>,
    generation: u64,
    relay_mode: RelayModeOption,
) -> anyhow::Result<sendmer::SendResult> {
    let options = SendOptions {
        relay_mode,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
    };
    send(path, options, emitter(sender, generation)).await
}

pub async fn start_receive(
    ticket: String,
    output_dir: PathBuf,
    sender: Sender<(u64, TransferEvent)>,
    generation: u64,
    relay_mode: RelayModeOption,
    retry_policy: sendmer::core::options::ReceiveRetryPolicy,
    cancellation: watch::Receiver<bool>,
) -> anyhow::Result<sendmer::ReceiveResult> {
    let options = ReceiveOptions {
        output_dir: Some(output_dir),
        relay_mode,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
        retry_policy,
    };
    sendmer::receive_with_cancellation(
        ticket,
        options,
        emitter(sender, generation),
        Some(cancellation),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{start_receive, start_send};
    use async_channel::unbounded;
    use iroh::EndpointAddr;
    use iroh_blobs::{BlobFormat, Hash, ticket::BlobTicket};
    use sendmer::{RelayModeOption, Role, TransferEvent};
    use std::fs;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;
    use tokio::sync::watch;

    const WORKER_MODE: &str = "ALTER_SENDME_TRANSFER_WORKER";
    const SOURCE_PATH: &str = "ALTER_SENDME_TRANSFER_SOURCE";
    const TICKET_PATH: &str = "ALTER_SENDME_TRANSFER_TICKET";
    const OUTPUT_PATH: &str = "ALTER_SENDME_TRANSFER_OUTPUT";

    /// Runs the sender half in a separate OS process so iroh does not reject a self-connection.
    #[test]
    fn worker_sender() {
        if std::env::var(WORKER_MODE).ok().as_deref() != Some("sender") {
            return;
        }
        let source = std::env::var(SOURCE_PATH).expect("sender source path");
        let ticket_path = std::env::var(TICKET_PATH).expect("sender ticket path");
        let runtime = tokio::runtime::Runtime::new().expect("create sender runtime");
        let (event_sender, event_receiver) = unbounded();
        let result = runtime
            .block_on(start_send(
                source.into(),
                event_sender,
                41,
                RelayModeOption::Disabled,
            ))
            .expect("start adapter sender");
        fs::write(ticket_path, result.ticket.to_string()).expect("publish sender ticket");

        // Keep the SendResult alive until the receiver has finished and the parent closes stdin.
        let mut signal = String::new();
        std::io::stdin()
            .read_to_string(&mut signal)
            .expect("wait for sender shutdown signal");
        runtime
            .block_on(result.shutdown())
            .expect("shut down adapter sender");

        let mut events = Vec::new();
        while let Ok(event) = event_receiver.try_recv() {
            events.push(event);
        }
        assert!(events.iter().all(|(generation, _)| *generation == 41));
        assert!(
            events
                .iter()
                .any(|(_, event)| matches!(event, TransferEvent::Started { role: Role::Sender }))
        );
    }

    /// Runs the receiver half in a separate OS process and verifies the adapter event stream.
    #[test]
    fn worker_receiver() {
        if std::env::var(WORKER_MODE).ok().as_deref() != Some("receiver") {
            return;
        }
        let ticket = fs::read_to_string(std::env::var(TICKET_PATH).expect("receiver ticket path"))
            .expect("read sender ticket");
        let output = std::env::var(OUTPUT_PATH).expect("receiver output path");
        let runtime = tokio::runtime::Runtime::new().expect("create receiver runtime");
        let (event_sender, event_receiver) = unbounded();
        let result = runtime
            .block_on(start_receive(
                ticket,
                output.into(),
                event_sender,
                41,
                RelayModeOption::Disabled,
                Default::default(),
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
        assert!(events.iter().all(|(generation, _)| *generation == 41));
        assert!(events.iter().any(|(_, event)| matches!(
            event,
            TransferEvent::Started {
                role: Role::Receiver
            }
        )));
        assert!(events.iter().any(|(_, event)| matches!(
            event,
            TransferEvent::Completed {
                role: Role::Receiver
            }
        )));
    }

    /// Uses the actual GPUI transfer adapter from two processes and compares the received bytes.
    #[test]
    fn adapter_transfers_a_file_and_forwards_transfer_events() {
        let source_dir = tempdir().expect("create source directory");
        let output_dir = tempdir().expect("create output directory");
        let source_file = source_dir.path().join("adapter-e2e.txt");
        let contents = b"GPUI transfer adapter end-to-end verification";
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

        let receiver_status = Command::new(&test_binary)
            .args(["--exact", "transfer::tests::worker_receiver", "--nocapture"])
            .env(WORKER_MODE, "receiver")
            .env(TICKET_PATH, &ticket_path)
            .env(OUTPUT_PATH, output_dir.path())
            .status()
            .expect("run adapter receiver worker");
        assert!(
            receiver_status.success(),
            "receiver worker failed: {receiver_status}"
        );

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

        let received = output_dir.path().join("adapter-e2e.txt");
        assert_eq!(fs::read(received).expect("read received file"), contents);
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
        let (event_sender, _event_receiver) = unbounded();
        let (_, cancellation) = watch::channel(true);
        let runtime = tokio::runtime::Runtime::new().expect("create cancellation runtime");

        let error = runtime
            .block_on(start_receive(
                ticket.to_string(),
                output_dir.path().to_path_buf(),
                event_sender,
                7,
                RelayModeOption::Disabled,
                Default::default(),
                cancellation,
            ))
            .expect_err("pre-cancelled receive should stop before connecting");

        assert_eq!(error.to_string(), "Operation cancelled");
        assert_eq!(temp_staging_dirs(&staging_prefix), before);
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
}
