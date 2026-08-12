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

The application pins the public `sendmer` repository to the `v0.6.0` tag so clean checkouts and
release builders do not depend on a sibling directory. During local core development, Cargo's
normal git checkout cache keeps builds reproducible while `F:\project\sendmer` remains the source
of truth for protocol changes.

Locale JSON files are bundled under `locales/` and converted into a static lookup table at build
time, so installed builds do not need the source project or a runtime locale directory.

## Scope

The first native slice covers the existing product mainline: Send and Receive tabs, file/folder
selection, drag-and-drop, ticket copy/paste, real sendmer transfers, progress, cancellation,
completion summaries, theme switching, language selection, and update/donation entry points.

Detailed architecture, milestone gates, packaging, and release checks are in
[`docs/DESIGN.md`](docs/DESIGN.md) and [`docs/DEVELOPMENT_PLAN.md`](docs/DEVELOPMENT_PLAN.md).
