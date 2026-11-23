## Which file do I download?

| Platform | Installer |
|---|---|
| Windows x64 | `Runvil_{version}_x64-setup.exe` |
| macOS, Apple silicon | `Runvil_{version}_aarch64-setup.dmg` |
| macOS, Intel | `Runvil_{version}_x86_64-setup.dmg` |
| Linux x64 (Ubuntu 22.04, Debian 12, or later) | `Runvil_{version}_amd64-setup.run` |
| Linux ARM64 (Ubuntu 24.04, Debian 13, or later) | `Runvil_{version}_aarch64-setup.run` |

On Linux, make the `.run` file executable (`chmod +x`) and run it from your desktop session. Workshop installs as an AppImage: if it does not start and reports `libfuse.so.2`, install `libfuse2` (`libfuse2t64` on Ubuntu 24.04 and later).

The other files are for Workshop's updater (`latest.json`, the `.app.tar.gz` and `.AppImage` archives, and the `.sig` signature beside each updater file, the Windows setup included) and for checking downloads (`SHA256SUMS`). You don't need them to install.

## What gets installed

Workshop, the desktop app, and the Gateway, with its tray icon and Settings page. On macOS and Linux the Gateway always sets up speech; on Windows, Speech to Text is a choice in the setup. The first time the Gateway starts with speech configured, it downloads about 0.6 GB of speech files, so dictation then works offline. This release includes Gateway {gateway_version}.

## Unsigned installers

The installers are not code-signed yet.
- **Windows:** if SmartScreen stops the setup, choose More info, then Run anyway.
- **macOS:** the first open is refused. Open System Settings, then Privacy & Security, choose Open Anyway, and open the installer again. Until the apps are signed, the Gateway's Launch at Login does not work on macOS.
