# build-installer

This crate is `cargo installer`, the one build path for Runvil's installers. Each installer installs Workshop, the desktop app, and the Gateway. CI's `installers.yml` runs the same commands described here, so a local build makes what a release ships.

Workshop is its own product, in [cppalliance/runvil-workshop](https://github.com/cppalliance/runvil-workshop), so nothing here builds it. `cargo installer` runs that repository's own tool, `cargo workshop bundle`, in a checkout of it, and builds the installer from the Tauri bundle the tool leaves. The Gateway is its own product too, in [cppalliance/runvil-gateway](https://github.com/cppalliance/runvil-gateway), and nothing here builds it either.

## Modes

- `cargo installer [--target <triple>] [--sign] [--gateway-binary <path>]` builds one platform's installer in release.
- `cargo installer manifest --assets <dir> --base-url <url> [--public-key <file>]` checks that a directory holds exactly one release's files for every platform, verifies each updater signature, and writes `latest.json` and `SHA256SUMS` beside them. The key it verifies against is the updater public key in the Workshop checkout's `crates/workshop/desktop/tauri.conf.json`, unless `--public-key` names another.

`cargo installer --help` prints the full usage.

## The Workshop checkout

The installer is built from a runvil-workshop checkout, and the `manifest` mode reads the updater public key from it. The `RUNVIL_WORKSHOP` environment variable names the checkout. When the variable is unset, `.cargo/config.toml` sets it to the checkout next to this repository, `../runvil-workshop`. `installers.yml` takes Workshop at the commit the root `Cargo.toml` pins in `[workspace.metadata.workshop]`, so a local build of a release starts from that commit.

Do two things in that checkout before the first build. Install the UI's packages, which bring the pinned Tauri CLI, and stage the Gateway release that the checkout's `gateway-pin/pin.json` names, once per target:

```text
npm ci --prefix crates/workshop
node tools/fetch-gateway-sidecar.mjs --target <triple>
```

That repository's `crates/build-workshop/README.md` lists what else its bundle build needs, such as Node 22 and Tauri's build packages on Linux. `cargo installer` passes its environment on to the bundle build, so a variable that README asks for, such as `APPIMAGE_EXTRACT_AND_RUN=1`, is set for `cargo installer`.

`--gateway-binary <path>`, or the `RUNVIL_GATEWAY_SIDECAR` environment variable, names another Gateway in place of the staged one, and `cargo installer` passes it on to the bundle build. The Plugins' libraries arrive inside Workshop's bundle.

### The version and the toolchain

`cargo installer` checks the checkout before it runs any command, and refuses one that differs from this workspace in either of two values:

- The `[workspace.package]` version in the checkout's `Cargo.toml` must equal this workspace's. The release file names and the release tag check use this workspace's version, while Tauri names its files with Workshop's.
- The `[toolchain]` channel in the checkout's `rust-toolchain.toml` must equal the one in this repository's `rust-toolchain.toml`. The Cargo that runs the installer also builds Workshop, so Workshop is built on this workspace's toolchain.

To build from a newer Workshop, change `rev` in `[workspace.metadata.workshop]`. When Workshop changed its version or its toolchain channel, change the workspace version in the root `Cargo.toml` or the channel in `rust-toolchain.toml` in the same change.

## Building an installer

The output lands in `target/installer/<triple>/`:

- `publish/`: the installer, and under `--sign` the updater files and their `.sig` signatures. Windows gets `Runvil_<version>_x64-setup.exe`, macOS `Runvil_<version>_<aarch64|x86_64>-setup.dmg`, and Linux `Runvil_<version>_<amd64|aarch64>-setup.run`.
- `payload/` (macOS and Linux): the installed items, `Runvil.app` and `Runvil Gateway.app`, or `Runvil.AppImage` and `runvil-gateway`.
- `ifw/` (macOS and Linux): the Qt IFW definition filled in from `ifw/` in this crate, with the payload copied into its packages.

Workshop's bundle itself is built in the checkout's own `target/`, and the build moves its files from there into the output, so the two target directories must be on one filesystem. When `CARGO_TARGET_DIR` is set, the bundle and the installer output both go to that directory.

Windows needs nothing more: the installer is the NSIS setup from Workshop's bundle, with the Gateway inside. macOS and Linux install the Gateway beside Workshop, so their installers are built here with the Qt Installer Framework, from Workshop's bundle and the Gateway. That build runs the Gateway's `--version` and refuses one that is not the pinned version.

Building the Intel macOS installer on Apple silicon needs Rosetta, since the build runs the x86_64 Gateway to check its version.

### Qt Installer Framework

macOS and Linux installers need Qt IFW 4.11.0 exactly; the build checks `installerbase --version` and refuses any other version. Download the archive for your machine from `https://download.qt.io/online/qtsdkrepository/`:

- macOS (one universal build for both architectures): `mac_x64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603311245ifw-mac-universal.7z`
- Linux x64: `linux_x64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603231357ifw-linux-x64.7z`
- Linux ARM64: `linux_arm64/ifw/tools_ifw_411/qt.tools.ifw.411/4.11.0-0-202603231357ifw-linux-arm64.7z`, which runs only on glibc 2.38 or newer

Check its SHA-256 against the `ifw-sha256` pinned beside the same path in `.github/workflows/installers.yml`. Extract it into a directory you own, with `bsdtar -xf` on macOS or `7z x` on Linux, and point `QTIFWDIR` at the extracted directory that holds `bin/`. Without `QTIFWDIR`, the build uses the first `binarycreator` on `PATH` and the `installerbase` beside it.

On Linux the IFW tools load these libraries even without a display, and `p7zip-full` extracts the archive, as `installers.yml` installs them on Ubuntu:

```text
p7zip-full
libxkbcommon-x11-0 libxcb-icccm4 libxcb-image0 libxcb-keysyms1 libxcb-randr0
libxcb-render-util0 libxcb-shape0 libxcb-shm0 libxcb-sync1 libxcb-xfixes0
libxcb-xkb1 libxcb-util1 libfontconfig1 libfreetype6 libdbus-1-3
```

### Signing

`cargo installer` passes `--sign` on to the bundle build. Without it, the bundle build makes no updater files, and `publish/` holds only the installer. With it, Tauri signs Workshop's updater files with the minisign key in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and `cargo installer` publishes them with their signatures. For a local try, make a throwaway key with the Tauri CLI in the Workshop checkout:

```text
node crates/workshop/node_modules/@tauri-apps/cli/tauri.js signer generate --ci -w <dir>/run.key
```

Installed copies trust only the release key, so files signed with any other key never install as an update.
