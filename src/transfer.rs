//! Async sendmer integration kept separate from the GPUI view state.

use async_channel::Sender;
use sendmer::{
    AddrInfoOptions, AppHandle, EventEmitter, ReceiveOptions, RelayModeOption, SendOptions,
    TransferEvent, receive, send,
};
use std::{path::PathBuf, sync::Arc};

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
) -> anyhow::Result<sendmer::ReceiveResult> {
    let options = ReceiveOptions {
        output_dir: Some(output_dir),
        relay_mode,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
        retry_policy,
    };
    receive(ticket, options, emitter(sender, generation)).await
}
