# Stable dependency refresh — 2026-11-17

## Scope

This branch upgrades the application to published Sendmer 0.11.0 and the paired GPUI /
gpui_platform crates from the latest non-prerelease Zed tag v1.28.1 (e8bc16a).
It refreshes all directly declared crates and the Cargo lockfile, including compatible
transitive updates, and updates release/CI actions to their current stable releases.
Rust is pinned to 1.100.0 for reproducible local and CI checks. Explicit X11 and Wayland
platform features preserve Linux backends after the GPUI dependency change.

- Sendmer 0.11.0 (library-only integration; disable its default clipboard feature).
- iroh-blobs 0.105.0 directly; iroh 1.4.1 for test fixtures.
- Tokio 1.54.1, async-channel 2.5.0, anyhow 1.0.104, serde 1.0.229,
  serde_json 1.0.154, directories 6.0.0, walkdir 2.5.0, rustls 0.23.46,
  tracing 0.1.44, tracing-subscriber 0.3.23, tempfile 3.27.0, embed-resource 3.0.12.
- cargo-packager-updater 0.2.3 and cargo-packager 0.11.8 are already current stable.
- Windows-only embed-resource is now scoped to Windows build dependencies.
- GitHub Actions: checkout 7.0.1, upload-artifact 7.0.2, download-artifact 8.0.2,
  rust-cache 2.9.3, sbom-action 0.24.3, cosign-installer 4.1.3, attest 4.2.2,
  setup-python 7.1.0, action-gh-release 3.0.4. rust-toolchain retains its documented
  stable channel action with the explicit Rust version input.

## Upstream constraints (not silently overridden)

The newest stable top-level crates do not imply that every transitive crate can use its
newest major or patch release. `cargo update --dry-run --verbose` reports zero additional
resolvable updates. In particular:

- Sendmer 0.11.0 still requires iroh-blobs 0.103.x, resolved to 0.103.1. It coexists with
  the application's 0.105.0. No private Sendmer types are exchanged across this boundary;
  ticket compatibility is exercised by adapter end-to-end tests. Removing the older
  transport version requires a new compatible Sendmer release, not a hidden local patch.
- Zed v1.28.1 pins taffy exactly to 0.15.0; 0.15.1 is unavailable under that constraint.
- GPUI's web dependency pins unicode-properties exactly to 0.1.3; a dry-run attempt at
  0.1.5 confirms the resolver conflict. It remains in the all-platform lockfile.
- crypto-common 0.1.7 pins generic-array exactly to 0.14.7 (rather than 0.14.9).
- GPUI's own reviewed Git revisions and older-major dependencies are retained as required
  by the stable Zed release. No upstream source is patched merely to change version numbers.

## Regression fixes

The old desktop event cursor could reset itself when a different session started, resume
accepting after an invalid event, and accept events after a terminal state. The desktop now
uses Sendmer's TransferEventStreamValidator before projecting event data. The validator
also validates role consistency and rejects unsafe file names. An explicit new-transfer
reset is required after rejection. Old-generation filtering remains before validation.

New tests cover sticky rejection, session/role changes, duplicate and gapped sequences,
post-terminal events, and unsafe filenames. Adapter E2E and cancellation event streams
are also validated using the upstream validator. The diagnostic Sendmer version is updated.

## Verification

Local macOS / Apple Silicon, Rust 1.100.0:

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets --locked --features gpui_platform/runtime_shaders`: passed.
- `cargo clippy --workspace --all-targets --locked --features gpui_platform/runtime_shaders -- -D warnings`: passed.
- `cargo test --workspace --locked --features gpui_platform/runtime_shaders`: 42 passed.
- `cargo build --locked --features gpui_platform/runtime_shaders`: passed.
- `python3 .github/scripts/test_generate_release_notes.py`: passed (1 test).
- `git diff --check`: passed.

The tests include real separate-process file transfer, TM1 directory transfer with an empty
child and preserved modification time, pre-cancelled receive cleanup, cache/limit option
mapping, diagnostics/history compatibility, and localization/update configuration checks.
They are not a kernel packet-loss/latency test, a two-machine Internet test, or signed-update
installation validation.

A temporary macOS app bundle using the runtime-shader build was launched and inspected:
sender/receiver navigation, invalid ticket error, repeated retry, new-transfer reset,
settings open/close, all 21 language choices, Escape dismissal, and normal quit passed.
No real user files were transmitted. This is a smoke test, not Windows/Linux visual acceptance.

The Mac only has Command Line Tools, not Xcode's Metal compiler. Consequently the default
shader-compilation check fails at `xcrun: unable to find utility metal` before application
compilation. The supported runtime_shaders feature was used for local checks; the default
release behavior is unchanged. Native default builds remain covered by the PR's three-OS CI.
Release packaging/signing/upload workflows were not run, since this task does not authorize
publishing a new release. See the PR checks for exact remote status and commit.
