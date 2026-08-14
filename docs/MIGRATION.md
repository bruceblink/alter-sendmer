# Migration from the Tauri client

The GPUI repository at [`bruceblink/alter-sendmer`](https://github.com/bruceblink/alter-sendmer) is
the active AlterSendme codebase. The former Tauri repository at `bruceblink/alter-sendme` is retained
as a read-only source and release archive.

## Replacement coverage

| Legacy capability | GPUI replacement |
| --- | --- |
| Send a file or directory | Native path picker and drag-and-drop |
| Generate and copy a ticket | Ticket card with copy and save actions |
| Receive into a selected folder | Native ticket input, paste action, and folder picker |
| Live progress and completion details | Byte progress, speed, file count, duration, and average speed |
| Stop send or receive | Sender shutdown and graceful receiver cancellation |
| Retry after failure | Explicit retry for both transfer roles |
| Reveal a completed download | Native platform file-manager reveal |
| System, dark, and light themes | Persistent GPUI theme selector |
| Language selection | Persistent dropdown with the same 21 locale catalogs |
| Update and donation actions | Signed in-app update installation and existing donation link |

The GPUI client additionally persists transfer metadata history and exposes relay mode, retry limit,
download chunk, and diagnostics controls. Transfer protocol compatibility remains owned by sendmer,
so a ticket does not encode whether its sender used the Tauri or GPUI interface.

## User migration

1. Stop any transfer that is still running in the Tauri application.
2. Install AlterSendme from the active repository's
   [latest release](https://github.com/bruceblink/alter-sendmer/releases/latest), or unpack the
   portable ZIP.
3. Select the preferred theme, language, relay mode, retry limit, and download directory once.
4. Start a new transfer. In-progress sessions and WebView local-storage preferences are intentionally
   not imported because they contain no durable user files.
5. Uninstall the Tauri client after the GPUI transfer has been verified on the machine.

Received files stay in the directory selected by the user and are not moved or deleted by migration.
The GPUI application stores its own settings and metadata history in the operating system's standard
application-data directories.

## Maintainer cutover checklist

- The active repository passes format, check, Clippy, and test gates on its pinned toolchain.
- A tagged release contains Windows, Linux, and macOS packages, `SHA256SUMS`, signatures, and `latest.json`.
- The release manifest and in-app update endpoints target `bruceblink/alter-sendmer`.
- Desktop screenshots cover the default window, minimum window, receive tab, and language dropdown.
- The legacy README redirects users before `bruceblink/alter-sendme` is archived.
