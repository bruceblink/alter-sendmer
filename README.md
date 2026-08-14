<div align="center">

<img src="assets/alter-sendme-mark.svg" width="96" alt="AlterSendme logo">

# AlterSendme

Native peer-to-peer file transfer built with Rust, GPUI, and sendmer.

[![CI](https://github.com/bruceblink/alter-sendmer/actions/workflows/ci.yml/badge.svg)](https://github.com/bruceblink/alter-sendmer/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/bruceblink/alter-sendmer)](https://github.com/bruceblink/alter-sendmer/releases/latest)
[![License](https://img.shields.io/github/license/bruceblink/alter-sendmer)](LICENSE)

</div>

AlterSendme transfers files and folders directly between peers without uploading them to third-party
cloud storage. The GPUI application replaces the retired Tauri/WebView client previously maintained
at [`bruceblink/alter-sendme`](https://github.com/bruceblink/alter-sendme).

![AlterSendme GPUI workspace](artifacts/acceptance-send-1024x720-current.png)

## Features

- Direct encrypted transfer over the sendmer iroh/QUIC stack, with NAT traversal and relay fallback.
- File and folder sending through the native picker or drag-and-drop.
- Ticket copy, paste, and save actions with explicit sender and receiver cancellation.
- Live progress, folder file counts, completion summaries, failure recovery, and transfer history.
- System, dark, and light themes plus a dropdown containing all 21 bundled languages.
- Relay, retry, and download-chunk preferences, diagnostics, update checks, and donation links.
- One native Rust process with no Node.js, Tauri, React, or WebView runtime.

## Install

Download the current Windows installer or portable ZIP from
[GitHub Releases](https://github.com/bruceblink/alter-sendmer/releases/latest). Each package has a
matching SHA-256 file, and `latest.json` supplies the same verified release location to the in-app
update check.

The GPUI release line is Windows-first. Archived macOS and Linux builds of the former Tauri client
remain available from the [legacy releases](https://github.com/bruceblink/alter-sendme/releases), but
they are no longer actively maintained.

## Development

Requirements:

- Rust `1.95` or newer
- Native GPUI build prerequisites for the target platform
- Inno Setup 6 only when producing the Windows installer

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run
```

The application pins GPUI and `gpui_platform` to the same reviewed Zed revision. It also pins the
public [`sendmer`](https://github.com/bruceblink/sendmer) repository to the reviewed
`2d4d6bf5ea79fb184ed70812db84fe4c265f485c` revision, so a clean checkout does not depend on a sibling
directory.

## Packaging

```powershell
cargo build --locked --release
.\scripts\package-portable.ps1 -Version 0.2.0 -SkipBuild
.\scripts\package-installer.ps1 -Version 0.2.0 -SkipBuild
.\scripts\write-release-manifest.ps1 -Version 0.2.0
```

Release tags run the same locked Rust gates before publishing a draft containing the installer,
portable ZIP, SHA-256 files, and update manifest.

## Documentation

- [Design](docs/DESIGN.md)
- [Development plan](docs/DEVELOPMENT_PLAN.md)
- [Migration from the Tauri client](docs/MIGRATION.md)

The repeatable Windows visual check is `scripts/capture-ui-acceptance.ps1`. It captures the default
sender, receiver, transfer settings, minimum window, and complete language dropdown states.

## Privacy and License

Transfer history contains metadata only and never file contents. AlterSendme is licensed under
[AGPL-3.0](LICENSE). See the [privacy policy](PRIVACY.md) for the local data and network services used
by the native application.

Support development through [GitHub Sponsors](https://github.com/sponsors/bruceblink) or
[Buy Me a Coffee](https://buymeacoffee.com/bruceblink).
