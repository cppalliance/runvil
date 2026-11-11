# Workshop desktop app

This crate is the Workshop desktop app, `runvil-workshop`: a Tauri window around Workshop's server and page. It attaches to a gateway already running for the user, launches the installed one when none is, or uses the remote gateway that `~/.runvil/workshop.toml` names.

## How it ships

The Runvil installer offers Workshop and the gateway as separate components, and on Windows also Speech to Text. The gateway is the release `gateway-pin/pin.json` names, from [cppalliance/runvil-gateway](https://github.com/cppalliance/runvil-gateway).

- Windows: one NSIS setup, built from `installer.nsi`, installs into `%LOCALAPPDATA%\Runvil`. The gateway, `runvil-gateway.exe`, travels inside Workshop's bundle and sits beside `runvil-workshop.exe`.
- macOS: a Qt IFW installer in a DMG places `Runvil.app` and `Runvil Gateway.app` side by side, in `/Applications/Runvil` by default.
- Linux: a Qt IFW `.run` installer places `Runvil.AppImage` and `runvil-gateway` side by side, in `~/Runvil` by default.

On every platform Workshop finds the gateway beside its own install through `workshop-gateway-discovery`'s installed-peer lookup.

## Plugins

Workshop loads every library Plugin in its Plugin directory when it starts. The directory is named `plugins` and ships with Workshop itself, so whatever replaces Workshop replaces its Plugins too:

- Windows: beside `runvil-workshop.exe`.
- macOS: inside the bundle, at `Runvil.app/Contents/Resources/plugins`.
- Linux: inside the AppImage, at `usr/lib/runvil/plugins`.

`[agents] plugins` in `workshop.toml` names another directory instead.

Each installer puts one library there, the std Plugin, which holds the file tools the built-in chat agent declares: `plugin_std.dll`, `libplugin_std.dylib`, or `libplugin_std.so`. Its code lives in [cppalliance/runvil-plugin-std](https://github.com/cppalliance/runvil-plugin-std), pinned by commit in the root `Cargo.toml`. A Workshop with no Plugin directory, or one whose std library does not load, refuses an agent that requires `std` and says that `runvil/std` is not installed.

`runvil-workshop --plugins` prints each Plugin the installation loads, as its name and its version, and exits. A library that does not load goes to stderr with the reason. Workshop keeps no log, so this is the way to see why an agent has no file tools.

## Updates

Workshop checks for an update a few seconds after it starts and from Help, About, through the Tauri updater and the `latest.json` on the `workshop-latest` release, whose files are signed. On macOS and Linux it replaces its own `.app` or AppImage; on Windows it runs the new setup, which updates every installed component, the gateway included.

An update replaces the Plugin directory with Workshop on every platform, because the directory sits inside what the updater replaces: the `.app`, the AppImage, or the files the Windows setup installs. A Plugin loads only in a Workshop that speaks its exact contract version, so the two must change together.

## Building

`cargo workshop` builds this app for development around the pinned gateway, and `cargo workshop installer` builds a platform's installer; `crates/build-workshop/README.md` covers both.
