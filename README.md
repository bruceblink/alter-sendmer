# AlterSendme GPUI

Native Rust/GPUI rewrite of AlterSendme. The desktop UI, transfer state machine, file dialogs,
drag-and-drop, clipboard actions, and sendmer event bridge live in one Rust process. The transfer
protocol remains owned by the sibling `F:\project\sendmer` project.

## Development

```powershell
cargo check
cargo test
cargo run
```

The local path dependency intentionally points at `F:\project\sendmer` while the two projects are
being developed together. A release branch can switch that dependency to `sendmer = "0.5.1"` after
the shared core API is frozen.

## Scope

The first native slice covers the existing product mainline: Send and Receive tabs, file/folder
selection, drag-and-drop, ticket copy/paste, real sendmer transfers, progress, cancellation,
completion summaries, theme switching, language selection, and update/donation entry points.

Detailed architecture and milestone gates are in [`docs/DESIGN.md`](docs/DESIGN.md) and
[`docs/DEVELOPMENT_PLAN.md`](docs/DEVELOPMENT_PLAN.md).
