# Contributing to Relay

Relay targets x64 Windows. Changes should preserve the existing desktop client, avoid modifying its settings, and keep memory measurements attributable to a specific channel, build, and profile. Please describe memory changes as measurements, not guarantees.

## Source map

| Path | Responsibility |
| --- | --- |
| `src/main.rs`, `src/ui.rs` | Native Windows interface and event loop. The launcher exits after handing off to Discord. |
| `src/discord.rs` | Client discovery, launch arguments, process ownership checks, and on-demand WMI counters. |
| `src/readings.rs` | Profile classification data, seven-sample medians, and compatible local reading files. |
| `build.rs`, `assets/relay.ico`, `src/relay.manifest` | Embed the icon and manifest as x64 Windows resources. |
| `tests/native_flow.rs`, `tests/fixture.rs` | Interactive integration test and disposable stand-in for Discord. |

## Check a change

From a PowerShell terminal in the repository root:

```powershell
cargo fmt --all -- --check
rustfmt --edition 2021 --check tests/fixture.rs
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

For changes to the launcher, meter, or UI, also run the opt-in integration test on an interactive Windows desktop:

```powershell
cargo test --release --locked --test native_flow -- --ignored
```

It opens the actual launcher against a temporary fixture, exercises all three profiles, and records a 30-second reading. It does not stop or launch your installed Discord. The Windows CI workflow runs formatting, lint, unit tests, and the release build. The interactive test is separate.

## Reporting a problem

Include your Windows version, Discord channel and build, launch profile, whether Vencord is present, and what you expected Relay to do. For memory issues, say how long Discord was running and whether you were idle, in a call, or streaming. Do **not** attach Discord profile folders, account tokens, raw command lines, or screenshots containing conversations.

## Preparing a release

- Review `git status --short --untracked-files=all`. Keep `Cargo.lock`, the icon, and manifest; do not add local toolchains, executables, archives, or `verification/` data to the source tree.
- Update the version in `Cargo.toml` and regenerate `Cargo.lock` when appropriate. Run the checks above and test the release executable.
- If distributing compiled binaries, provide the corresponding source and license notices, including applicable third-party notices. Keep release downloads separate from the Git source tree.

Project license: [GPL-3.0-only](LICENSE).
