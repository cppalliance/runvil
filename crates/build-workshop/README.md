# build-workshop

This crate is `cargo workshop`, the one build path for Runvil's desktop products. CI's `installers.yml` runs the same commands described here, so a local build makes what a release ships.

## Modes

- `cargo workshop [--release] [--target <triple>] [--gateway-binary <path>]` builds Workshop with Cargo. For a Windows target the gateway is Workshop's sidecar; elsewhere the build copies it beside Workshop in the target directory, where a development Workshop finds it. Every build also copies the std Plugin's library into `plugins` beside Workshop, where a development Workshop loads it.
- `cargo workshop installer [--target <triple>] [--sign] [--gateway-binary <path>]` builds one platform's installer in release.
- `cargo workshop manifest --assets <dir> --base-url <url> [--public-key <file>]` checks that a directory holds exactly one release's files for every platform, verifies each updater signature, and writes `latest.json` and `SHA256SUMS` beside them.

`cargo workshop --help` prints the full usage.

## The gateway

The gateway is its own product, in [cppalliance/runvil-gateway](https://github.com/cppalliance/runvil-gateway), so nothing here builds it. Every build ships the release `gateway-pin/pin.json` names. Stage it once per target:

```text
node tools/fetch-gateway-sidecar.mjs --target <triple>
```

The script downloads the release asset, checks its SHA-256 against the pin, and stages the executable in `crates/workshop/desktop/binaries/`, where every later build reuses it. `--gateway-binary <path>`, or the `RUNVIL_GATEWAY_SIDECAR` environment variable, names another gateway instead; for a Windows target, remove the staged one first, because a named gateway never replaces it. The installer build runs the gateway's `--version` and refuses one that is not the pinned version.

## The std Plugin

The file tools live in the std Plugin, a dynamic library whose code is in [cppalliance/runvil-plugin-std](https://github.com/cppalliance/runvil-plugin-std), pinned by commit in the root `Cargo.toml`. Nothing here builds it by name: the Workshop server depends on `workshop-plugin-std`, so Cargo writes `plugin_std.dll`, `libplugin_std.so`, or `libplugin_std.dylib` to `deps` in the profile's directory whenever it builds Workshop. Each mode takes the library from there and puts it in the Plugin directory, `plugins` beside Workshop. A Workshop run with plain `cargo run -p workshop` has no Plugin directory, so its agents have no file tools; run `cargo workshop` once to place the library.

## Building an installer

Every platform needs Node 22, the staged gateway above, and the UI install, which brings the pinned Tauri CLI:

```text
npm ci --prefix crates/workshop
```

The output lands in `target/installer/<triple>/`:

- `publish/`: the installer, and under `--sign` the updater files and their `.sig` signatures. Windows gets `Runvil_<version>_x64-setup.exe`, macOS `Runvil_<version>_<aarch64|x86_64>-setup.dmg`, and Linux `Runvil_<version>_<amd64|aarch64>-setup.run`.
- `payload/` (macOS and Linux): the installed items, `Runvil.app` and `Runvil Gateway.app`, or `Runvil.AppImage` and `runvil-gateway`, and `plugins/` with the std Plugin's library.
- `ifw/` (macOS and Linux): the Qt IFW definition filled in from `ifw/` in this crate, with the payload copied into its packages. The Plugin directory goes in Workshop's package.

Windows needs nothing more: Tauri builds the NSIS setup from `crates/workshop/desktop/installer.nsi`, with the gateway inside. That template takes the std Plugin's library from `deps` beside the built executable and installs it as `plugins\plugin_std.dll`.

Linux also needs Tauri's build packages and the libraries the Qt IFW tools load even without a display, as `installers.yml` installs them on Ubuntu:

```text
gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-pulseaudio
gstreamer1.0-alsa patchelf
libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev xdg-utils p7zip-full
libxkbcommon-x11-0 libxcb-icccm4 libxcb-image0 libxcb-keysyms1 libxcb-randr0
libxcb-render-util0 libxcb-shape0 libxcb-shm0 libxcb-sync1 libxcb-xfixes0
libxcb-xkb1 libxcb-util1 libfontconfig1 libfreetype6 libdbus-1-3
```

Building the Intel macOS installer on Apple silicon needs Rosetta, since the build runs the x86_64 gateway to check its version.

Where FUSE is unavailable, such as in a container, set `APPIMAGE_EXTRACT_AND_RUN=1` so Tauri's AppImage tools run without it.

### Qt Installer Framework

macOS and Linux installers need Qt IFW 4.11.0 exactly; the build checks `installerbase --version` and refuses any other version. Download the archive for your machine from `https://download.qt.io/online/qtsdkrepository/`:

- macOS (one universal build for both architectures): `mac_x64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603311245ifw-mac-universal.7z`
- Linux x64: `linux_x64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603231357ifw-linux-x64.7z`
- Linux ARM64: `linux_arm64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603231357ifw-linux-arm64.7z`, which runs only on glibc 2.38 or newer

Check its SHA-256 against the `ifw-sha256` pinned beside the same path in `.github/workflows/installers.yml`. Extract it into a directory you own, with `bsdtar -xf` on macOS or `7z x` on Linux, and point `QTIFWDIR` at the extracted directory that holds `bin/`. Without `QTIFWDIR`, the build uses the first `binarycreator` on `PATH` and the `installerbase` beside it.

### Signing

Without `--sign`, the build applies `crates/workshop/desktop/tauri.unsigned.conf.json` and makes no updater files. With it, `tauri build` signs Workshop's updater files with the minisign key in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. For a local try, make a throwaway key:

```text
node crates/workshop/node_modules/@tauri-apps/cli/tauri.js signer generate --ci -w <dir>/run.key
```

Installed copies trust only the release key, so files signed with any other key never install as an update.
