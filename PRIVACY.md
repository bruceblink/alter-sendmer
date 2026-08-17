# Privacy Policy

Last updated: August 14, 2026

AlterSendmer is a peer-to-peer file-transfer application. It does not require an account and does
not operate an application server that stores transferred files.

## Data processed by the application

AlterSendmer processes the files or folders that you explicitly select, the destination directory
you choose, and the transfer ticket used to connect the sender and receiver. File contents are sent
through the sendmer/iroh transport and are not uploaded to an AlterSendmer-owned cloud service.

Transfers use authenticated QUIC connections with TLS encryption. When a direct connection cannot
be established, the configured relay service may carry encrypted traffic. Relay and peer operators
can observe connection metadata such as IP addresses, connection times, duration, and traffic
volume, but the encrypted transport does not expose file contents to the relay.

## Data stored locally

The native GPUI application may store the following information in the operating system's standard
application data and configuration directories:

- theme, language, relay, retry, and download-chunk preferences;
- transfer history containing role, local path, byte count, duration, average speed, result, and
  timestamp;
- sender tickets in transfer history so the user can copy a still-active ticket from the history
  panel; and
- temporary sendmer working data needed while a transfer is active.

Sender tickets can grant access to an active share. Treat them as private capability tokens and
clear transfer history when they should no longer remain on the device. AlterSendmer does not store
received file contents inside its history database. Received files remain in the destination chosen
by the user until the user deletes them.

Temporary transfer resources are cleaned up when a transfer ends or the application exits. A crash
or forced termination can leave temporary data behind until sendmer or the operating system removes
it.

## Network requests

The application can contact:

- peers and configured iroh relay or discovery services to establish a transfer;
- GitHub Releases for a user-initiated update check; and
- the sponsorship page only after the user selects the donation link.

AlterSendmer does not include advertising, analytics, telemetry, cookies, or device fingerprinting.
Normal network infrastructure and third-party services may retain their own access logs under their
respective policies.

## User control

You control which paths are shared, where received files are saved, whether relay fallback is
enabled, and when local transfer history is cleared. Uninstalling the application may not remove
configuration or history files automatically; those files can be removed from the platform-specific
application-data directory, which remains named `AlterSendme` for upgrade compatibility.

## Source and contact

The active source repository is
[`bruceblink/alter-sendmer`](https://github.com/bruceblink/alter-sendmer). Questions or privacy
issues should be reported through that repository's issue tracker.

This policy may be updated when application behavior or third-party dependencies change.
