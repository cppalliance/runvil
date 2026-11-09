# Workshop desktop app

This crate is the Workshop desktop app, `runvil-workshop`: a Tauri window around Workshop's server and page. It attaches to a gateway already running for the user, launches the installed one when none is, or uses the remote gateway that `~/.runvil/workshop.toml` names.

## How it ships

The Runvil installer offers Workshop and the gateway as separate components, and on Windows also Speech to Text. The gateway is the release `gateway-pin/pin.json` names, from [cppalliance/runvil-gateway](https://github.com/cppalliance/runvil-gateway).

- Windows: one NSIS setup, built from `installer.nsi`, installs into `%LOCALAPPDATA%\Runvil`. The gateway, `runvil-gateway.exe`, travels inside Workshop's bundle and sits beside `runvil-workshop.exe`.
- macOS: a Qt IFW installer in a DMG places `Runvil.app` and `Runvil Gateway.app` side by side, in `/Applications/Runvil` by default.
- Linux: a Qt IFW `.run` installer places `Runvil.AppImage` and `runvil-gateway` side by side, in `~/Runvil` by default.

On every platform Workshop finds the gateway beside its own install through `workshop-gateway-discovery`'s installed-peer lookup.

## Updates

Workshop checks for an update a few seconds after it starts and from Help, About, through the Tauri updater and the `latest.json` on the `workshop-latest` release, whose files are signed. On macOS and Linux it replaces its own `.app` or AppImage; on Windows it runs the new setup, which updates every installed component, the gateway included.

## Building

`cargo workshop` builds this app for development around the pinned gateway, and `cargo workshop installer` builds a platform's installer; `crates/build-workshop/README.md` covers both.
