# AlterSendme GPUI

Native Rust/GPUI rewrite of AlterSendme. The desktop UI, transfer state machine, file dialogs,
drag-and-drop, clipboard actions, and sendmer event bridge live in one Rust process. The transfer
protocol is provided by the `sendmer` 0.6.0 core project.

## Development

```powershell
cargo check
cargo test
cargo run
```

The window is native GPUI only; there is no Tauri or webview runtime. The sender and receiver use
the public `sendmer` 0.6.0 iroh stack directly. Start sharing returns a ticket, while receiving
validates the ticket before opening the network task. Stop waits for sender shutdown, and failed
transfers stay in a visible Failed state until `Try again` or `New transfer` is selected.

The application pins the public `sendmer` repository to the `v0.6.0` tag so clean checkouts and
release builders do not depend on a sibling directory. During local core development, Cargo's
normal git checkout cache keeps builds reproducible while `F:\project\sendmer` remains the source
of truth for protocol changes.

Locale JSON files are bundled under `locales/` and converted into a static lookup table at build
time, so installed builds do not need the source project or a runtime locale directory.

## Scope

The native client covers the product mainline: Send and Receive tabs, file/folder selection,
drag-and-drop, ticket copy/paste/save, real sendmer transfers, segmented progress with folder file
counters, cancellation, completion summaries, failure retry, history persistence, theme/language,
relay/retry preferences, diagnostics, and update/donation entry points.

History is stored as JSON below the platform application-data directory. It contains transfer
metadata only, never file contents. Windows portable and Inno Setup packages are built with the
scripts under `scripts/`; CI and release workflows run the locked Rust gates before packaging.

Detailed architecture, milestone gates, packaging, and release checks are in
[`docs/DESIGN.md`](docs/DESIGN.md) and [`docs/DEVELOPMENT_PLAN.md`](docs/DEVELOPMENT_PLAN.md).
