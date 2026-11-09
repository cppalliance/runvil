# workshop

- Unsafe is confined to the Windows bridge (`src/bridge.rs`): dense working COM with documented failure modes and the crate's only unsafe code; its module-level `#[expect(unsafe_code)]` is deliberate. No other module contains unsafe code.
- The desktop app does not read Gateway configuration, own the Gateway discovery file, or kill the Gateway as part of ordinary teardown.
- Workshop carries the Gateway inside its bundle only on Windows: only `tauri.windows.conf.json` declares `externalBin`, and `build.rs` fills the sidecar slot only for Windows targets. On macOS and Linux the Gateway is its own install beside Workshop, found through `workshop-gateway-discovery`'s installed-peer lookup (`installed_gateway`).
- The desktop app never replaces the Gateway's files. The Gateway it ships is the release `gateway-pin/pin.json` names, from `cppalliance/runvil-gateway`.
- In `installer.nsi`, the Speech to Text section has no payload: `-Finalize` records the choice as the `InstallSTT` value the Gateway reads when it writes its first config, and never under `/UPDATE`. `/COMPONENTS=` and the exit code 2 (a refused component list) exist for CI; `.github/installers/cell.ps1` asserts them, so a change to either updates it.
- `tauri.unsigned.conf.json` is for local builds without `--sign` only; every CI build signs.
