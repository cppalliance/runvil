//! `workshop-plugin-std` - the std Plugin as the dynamic library Workshop
//! ships in its Plugin directory: `plugin_std.dll` on Windows,
//! `libplugin_std.so` on Linux, and `libplugin_std.dylib` on macOS.
//!
//! The Plugin's code lives in its own repository,
//! `cppalliance/runvil-plugin-std`, which the root `Cargo.toml` pins by
//! commit. Cargo gives a git dependency's own library a file name with a
//! hash in it, which no test and no installer could find, so this crate
//! links the Plugin in and exports its entry point under a file name the
//! workspace fixes. Cargo writes that file to `deps` in the profile's
//! directory whenever it builds the server, and `cargo workshop` copies
//! it from there into the Plugin directory.
//!
//! ## Invariants
//!
//! - Tier: library Plugin; may depend on no `workshop-*` crate, and only
//!   the server may name it. `cargo test -p build-xtask` enforces the
//!   tiers.
//! - Holds no code of its own beyond the export of `runvil_plugin`.
//! - The server depends on this crate only so that Cargo builds the
//!   library. No Workshop crate links it in or calls it: Workshop reaches
//!   the Plugin through the library alone.
//! - Names the Plugin SDK at the commit the pinned std Plugin was built
//!   against. Cargo links one copy of the contract into a build, so a pin
//!   that disagrees fails to resolve.

runvil_plugin_sdk::export_plugin!(std_plugin::Std);
