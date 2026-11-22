//! Async sendmer integration kept separate from the GPUI view state.

use async_channel::Sender;
use sendmer::{
    AddrInfoOptions, AppHandle, EventEmitter, ReceiveCacheOptions, ReceiveOptions, RelayModeOption,
    SendOptions, TransferEvent, send_handle,
};
use std::{num::NonZeroU64, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::watch;

const MEBIBYTE: u64 = 1024 * 1024;

/// Non-blocking adapter from sendmer events to the GPUI event loop.
#[derive(Clone)]
pub struct ChannelEventEmitter {
    pub sender: Sender<(u64, TransferEvent)>,
    pub generation: u64,
}

/// Groups receive-side transport policy so UI callers cannot misorder related arguments.
pub(crate) struct ReceiveTransferOptions {
    pub relay_mode: RelayModeOption,
    pub retry_policy: sendmer::core::options::ReceiveRetryPolicy,
    pub receive_cache: Option<ReceiveCacheOptions>,
}

/// Groups sender-side policy so UI callers pass one coherent public API mapping.
pub(crate) struct SendTransferOptions {
    pub relay_mode: RelayModeOption,
    pub max_upload_rate_mib_per_sec: Option<u64>,
    pub max_receivers: Option<u64>,
    pub session_lifetime_seconds: Option<u64>,
    pub manifest_mode: bool,
}

impl EventEmitter for ChannelEventEmitter {
    fn emit(&self, event: &TransferEvent) {
        let _ = self.sender.try_send((self.generation, event.clone()));
    }
}

pub fn emitter(sender: Sender<(u64, TransferEvent)>, generation: u64) -> AppHandle {
    Some(Arc::new(ChannelEventEmitter { sender, generation }))
}

/// Converts the desktop client's MiB/s preference into sendmer's exact bytes/s API.
///
/// `None` keeps the sender unrestricted. Zero and overflowing values are rejected before
/// sendmer creates an endpoint or temporary store.
pub(crate) fn max_upload_rate_from_mib(
    rate_mib_per_sec: Option<u64>,
) -> anyhow::Result<Option<NonZeroU64>> {
    let Some(rate_mib_per_sec) = rate_mib_per_sec else {
        return Ok(None);
    };
    let bytes_per_sec = rate_mib_per_sec
        .checked_mul(MEBIBYTE)
        .ok_or_else(|| anyhow::anyhow!("upload rate is too large"))?;
    NonZeroU64::new(bytes_per_sec)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("upload rate must be greater than zero"))
}

/// Converts the optional receiver-count preference into sendmer's non-zero API value.
pub(crate) fn max_receivers_from_preference(
    max_receivers: Option<u64>,
) -> anyhow::Result<Option<NonZeroU64>> {
    max_receivers
        .map(|value| {
            NonZeroU64::new(value)
                .ok_or_else(|| anyhow::anyhow!("receiver limit must be greater than zero"))
        })
        .transpose()
}

/// Converts the desktop lifetime preset into sendmer's fixed post-ready session duration.
pub(crate) fn session_lifetime_from_seconds(
    seconds: Option<u64>,
) -> anyhow::Result<Option<Duration>> {
    seconds
        .map(|value| {
            anyhow::ensure!(value > 0, "session lifetime must be greater than zero");
            Ok(Duration::from_secs(value))
        })
        .transpose()
}

/// Maps the desktop cache preference to sendmer's persistent receive-cache contract.
///
/// Disabled caching returns `None`; enabled caching records the selected retention period
/// in each new cache entry and validates the root before network setup begins.
pub(crate) fn receive_cache_options(
    enabled: bool,
    root_dir: PathBuf,
    ttl_days: u64,
) -> anyhow::Result<Option<ReceiveCacheOptions>> {
    if !enabled {
        return Ok(None);
    }
    let ttl_seconds = ttl_days
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| anyhow::anyhow!("receive cache retention is too large"))?;
    let options = ReceiveCacheOptions::new(root_dir).with_ttl(Duration::from_secs(ttl_seconds));
    options.validate()?;
    Ok(Some(options))
}

pub async fn start_send(
    path: PathBuf,
    sender: Sender<(u64, TransferEvent)>,
    generation: u64,
    transfer_options: SendTransferOptions,
) -> anyhow::Result<sendmer::SendHandle> {
    let options = SendOptions {
        relay_mode: transfer_options.relay_mode,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        manifest_mode: transfer_options.manifest_mode,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
        max_upload_rate_bytes_per_sec: max_upload_rate_from_mib(
            transfer_options.max_upload_rate_mib_per_sec,
        )?,
        max_receivers: max_receivers_from_preference(transfer_options.max_receivers)?,
        max_files: None,
        max_total_size_bytes: None,
        max_import_memory_bytes: None,
        session_lifetime: session_lifetime_from_seconds(transfer_options.session_lifetime_seconds)?,
    };
    send_handle(path, options, emitter(sender, generation)).await
}

pub async fn start_receive(
    ticket: String,
    output_dir: PathBuf,
    sender: Sender<(u64, TransferEvent)>,
    generation: u64,
    transfer_options: ReceiveTransferOptions,
    cancellation: watch::Receiver<bool>,
) -> anyhow::Result<sendmer::ReceiveResult> {
    let options = ReceiveOptions {
        output_dir: Some(output_dir),
        relay_mode: transfer_options.relay_mode,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
        retry_policy: transfer_options.retry_policy,
        receive_cache: transfer_options.receive_cache,
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
#[path = "transfer_tests.rs"]
mod tests;
