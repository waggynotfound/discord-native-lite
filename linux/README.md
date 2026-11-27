# Relay for Linux

Relay launches your installed **official desktop Discord** with Stock, Balanced or Strong saver settings. It does not contain Discord, install Vencord, change account settings, or install a service. Existing desktop Vencord installations stay in Discord's launch path. Memory savings vary by workload; Strong saver can cost CPU.

## Run the AppImage

On an Intel/AMD **x86-64 Linux** desktop, download `Relay-2.1.0-linux-x86_64.AppImage` and its optional `.sha256` from the public [v2.1.0 release](https://github.com/waggynotfound/discord-native-lite/releases/tag/v2.1.0). From the download directory:

```sh
sha256sum -c Relay-2.1.0-linux-x86_64.AppImage.sha256  # optional
chmod +x Relay-2.1.0-linux-x86_64.AppImage
./Relay-2.1.0-linux-x86_64.AppImage
```

No root access, Rust installation, bundled Electron, GTK or Qt runtime is required. The graphical interface uses X11, including XWayland on Wayland desktops, and a 24/32-bit TrueColor visual. For systems without usable FUSE, use `./Relay-2.1.0-linux-x86_64.AppImage --appimage-extract-and-run`. This package is **not** for ARM/aarch64. You can share the GitHub download link directly; no sign-in is required.

## Select the client

Use the channel button to cycle Stable, Canary, and PTB. Standard native installations under `/usr/share`, `/opt`, `/usr/lib`, `/usr/lib64`, `/usr/local/share`, and common per-user install directories are checked. Executable symlinks on `PATH` are also supported. For a custom tarball location pass `--discord /absolute/path/to/Discord`. Flatpak, Snap, Vesktop, and shell wrappers are not supported by this port. Do not run Relay or Discord as root.

Finish your call and fully quit Discord before switching profiles. Stock passes no flags. Balanced passes `--optimize-for-size`; Strong saver also passes `--memory-saver-mode --heap-growing-percent=15`, together in one Electron `--js-flags` argument. Relay exits after handoff and never kills a running client. A small Vencord loader marker is informational, not a plugin health check.

**Compare RAM** measures the selected native executable's same-user processes via `/proc`. It sums private clean, dirty, and private huge pages, not total RSS. Linux `smaps_rollup` availability and process permissions vary: if counters or renderer arguments cannot be verified, recording is unavailable. Relay never requests elevated privileges or asks you to weaken `/proc` restrictions. The median of seven samples approximately five seconds apart is saved for the same executable fingerprint, channel, and actual profile in `${XDG_STATE_HOME:-$HOME/.local/state}/relay/readings`. The launcher and non-matching helper executables are excluded. Compare equivalent workloads after equal warm-up periods. Closing the window cancels an unfinished recording.

**Terminal mode:** `--help`, `--version`, `--status`, `--launch stock|balanced|strong`, `--record`, `--channel stable|canary|ptb`. Keyboard controls in the graphical interface: Tab/Shift‑Tab or arrows to move, Enter/Space to select, Escape to close.

## Build and test

This crate is independent of the Windows crate. On x86-64 Linux with a current Rust toolchain, `musl-tools`, curl, Python 3 and standard coreutils installed, run from the repository root:

```sh
rustup target add x86_64-unknown-linux-musl
cargo test --manifest-path linux/Cargo.toml --locked
bash linux/build-appimage.sh
# Test requires Xvfb, xauth, xdotool and ImageMagick:
xvfb-run -a -s '-screen 0 1024x768x24' bash linux/tests/smoke.sh
```

Outputs are in `linux/out/`. The packager pins the upstream AppImage runtime and tool by asset ID and SHA-256. Set `APPIMAGETOOL` and `RELAY_RUNTIME` to matching local files for offline packaging with a cached Rust toolchain and crates. The test uses an isolated native fixture, not a real Discord login. The AppImage includes the GPL-3.0-only license for Relay, font and dependency notices, and the AppImage runtime notice under `usr/share/doc/relay/`.
