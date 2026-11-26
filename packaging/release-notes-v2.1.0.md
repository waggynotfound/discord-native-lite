## Install on Windows x64

Download **Relay-2.1.0-windows-x64-setup.exe** below. It installs Relay for the current user without administrator rights, adds a Start menu shortcut, and provides an uninstaller under **Settings → Apps**. Or download the portable ZIP, extract it, and run `Relay.exe`.

- Start the official Discord desktop app with Stock, Balanced, or Strong saver settings. Vencord is optional.
- Compare seven-sample private RAM readings for the same Discord build and profile.
- The README includes [two Task Manager captures](https://github.com/waggynotfound/discord-native-lite/blob/v2.1.0/README.md#example-two-task-manager-snapshots): **1,190.4 MB** for ordinary Discord and **612.4 MB** for Discord launched with Relay. The captures show six versus three processes and different activity. They illustrate one observation, not a guaranteed reduction.

**Verification:** The Windows packages are unsigned. Compare `Get-FileHash -Algorithm SHA256 <downloaded-file>` with `SHA256SUMS.txt`. The matching source is available from this tag; license notices are included in both packages. This release does not contain Discord or Vencord.
