//! Boot planning and detached Gateway spawn.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, OnceLock};

use anyhow::Context as _;
use workshop_server_api::Config;
use workshop_server_api::discovery::{
    CancellationToken, GatewayDiscoveryFile, Resolution, SidecarError, ValidatedConnection,
};

use super::identity::GatewayAttachment;
use super::supervisor::{RecoveryCandidate, RecoveryOwnership, launch_and_attach_cancellable};

/// The Gateway executable's file name, which test fixtures plant.
#[cfg(all(test, windows))]
pub(super) const GATEWAY_EXE_NAME: &str = "runvil-gateway.exe";
/// The Gateway executable's file name, which test fixtures plant.
#[cfg(all(test, not(windows)))]
pub(super) const GATEWAY_EXE_NAME: &str = "runvil-gateway";

#[cfg(windows)]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const WINDOWS_DETACHED_CREATION_FLAGS: u32 = CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS;
#[cfg(windows)]
const WINDOWS_BREAKAWAY_CREATION_FLAGS: u32 =
    CREATE_BREAKAWAY_FROM_JOB | WINDOWS_DETACHED_CREATION_FLAGS;

/// What the boot decision concluded.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum GatewayPlan {
    /// A live gateway discovery file exists.
    Attach(GatewayDiscoveryFile),
    /// No live file exists and an installed Gateway can be launched.
    Launch(PathBuf),
    /// Explicit configuration is the only available endpoint.
    ConfigOnly,
    /// No attachment path exists.
    Fail,
}

/// The result of a launch election, retaining the spawned pid only for the
/// branch that actually created a child.
#[derive(Debug)]
pub(super) enum RecoveryLaunch {
    /// A race winner had already published a live Gateway.
    Attached(GatewayDiscoveryFile),
    /// This process spawned a child and observed a gateway discovery file.
    Launched {
        child_pid: u32,
        file: GatewayDiscoveryFile,
    },
}

/// Connects the Gateway for boot.
///
/// # Errors
/// Returns an error when no attachment path exists or launch fails.
pub(crate) fn ensure_gateway(config: &Config) -> anyhow::Result<GatewayAttachment> {
    let installed = installed_gateway()?;
    let explicit = !config.gateway.base_url.is_empty();
    let Some(run_dir) = workshop_server_api::discovery::default_run_dir() else {
        return if explicit {
            Ok(GatewayAttachment::Config)
        } else {
            Err(no_gateway_error())
        };
    };
    match plan_gateway(
        &run_dir,
        installed,
        explicit,
        workshop_server_api::discovery::resolve,
    ) {
        GatewayPlan::Attach(file) => validated_attachment(file),
        GatewayPlan::ConfigOnly => Ok(GatewayAttachment::Config),
        GatewayPlan::Fail => Err(no_gateway_error()),
        GatewayPlan::Launch(exe) => {
            if sibling_is_placeholder(&exe) {
                return Err(placeholder_gateway_error(&exe));
            }
            launch_and_attach_cancellable(&run_dir, &exe, &CancellationToken::new())
                .and_then(validated_recovery_attachment)
                .context("launch the sidecar gateway")
        }
    }
}

/// Retains the selected process proof instead of reducing it to file fields.
fn validated_attachment(file: GatewayDiscoveryFile) -> anyhow::Result<GatewayAttachment> {
    ValidatedConnection::validate(file)
        .map(GatewayAttachment::Sidecar)
        .context("validate the selected gateway process identity")
}

fn validated_recovery_attachment(recovery: RecoveryLaunch) -> anyhow::Result<GatewayAttachment> {
    match recovery {
        RecoveryLaunch::Attached(file) => validated_attachment(file),
        RecoveryLaunch::Launched { child_pid, file } => {
            let validated = ValidatedConnection::validate(file)
                .context("validate the launched gateway process identity")?;
            Ok(
                match RecoveryCandidate::authenticate(child_pid, validated) {
                    RecoveryOwnership::Owned(candidate) => GatewayAttachment::Launched(candidate),
                    RecoveryOwnership::Unowned(unowned) => GatewayAttachment::Sidecar(unowned),
                },
            )
        }
    }
}

/// Chooses attach, launch, configured fallback, or failure.
pub(super) fn plan_gateway(
    run_dir: &Path,
    installed: Option<PathBuf>,
    explicit_config: bool,
    resolve: fn(&Path) -> Result<Resolution, SidecarError>,
) -> GatewayPlan {
    match resolve(run_dir) {
        Ok(Resolution::Attach(file)) => return GatewayPlan::Attach(file),
        Ok(_) => {}
        Err(error) => {
            eprintln!("could not resolve the gateway discovery file: {error}");
        }
    }
    match installed {
        Some(exe) => GatewayPlan::Launch(exe),
        None if explicit_config => GatewayPlan::ConfigOnly,
        None => GatewayPlan::Fail,
    }
}

/// Locates the installed Gateway for the running Workshop: beside it on
/// Windows and in development, as the sibling `Runvil Gateway.app` on
/// macOS, and beside `Runvil.AppImage` on Linux, where Workshop runs from
/// the AppImage's mount.
///
/// # Errors
/// Returns an error when the executable path cannot be read.
pub(super) fn installed_gateway() -> anyhow::Result<Option<PathBuf>> {
    let exe = std::env::current_exe().context("locate the executable")?;
    let appimage = workshop_server_api::discovery::running_appimage(
        &exe,
        std::env::var_os("APPIMAGE").as_deref(),
        std::env::var_os("APPDIR").as_deref(),
    );
    Ok(workshop_server_api::discovery::installed_gateway(
        &exe,
        appimage.as_deref(),
    ))
}

/// The text `tools/fetch-gateway-sidecar.mjs --stub` writes, and the only
/// content a file may hold and still be called a Gateway. Checked before a
/// launch because Windows answers a text file with "This version of %1 is
/// not compatible with the version of Windows you're running" and no clue
/// which file was at fault.
const STUB_PREFIX: &str = "This is a placeholder staged by";

/// Whether the installed Gateway is a compile-only stub rather than an
/// executable. A real Gateway starts with `MZ` on Windows and something
/// other than this text everywhere else, so a byte-prefix compare is the
/// whole test.
pub(super) fn sibling_is_placeholder(exe: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(exe) else {
        return false;
    };
    let mut prefix = vec![0_u8; STUB_PREFIX.len()];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    prefix.truncate(read);
    prefix == STUB_PREFIX.as_bytes()
}

/// The failure for a stub sidecar, naming the file and the one command that
/// replaces it with a runnable Gateway.
pub(super) fn placeholder_gateway_error(exe: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "the gateway beside this program is a compile-only stub, not an executable: {}; \
         run `node tools/fetch-gateway-sidecar.mjs` from the repository root to stage the \
         pinned Gateway release, then rebuild",
        exe.display()
    )
}

/// Builds the loud boot failure naming both supported remedies.
pub(super) fn no_gateway_error() -> anyhow::Error {
    anyhow::anyhow!(
        "no gateway configured or running; install the Gateway component, which \
         puts runvil-gateway where Workshop looks for it, or set \
         gateway.base_url and gateway.api_key in workshop.toml to attach to \
         a gateway over the network"
    )
}

#[cfg(windows)]
pub(super) fn spawn_detached_windows_with<T, Spawn>(mut spawn: Spawn) -> std::io::Result<T>
where
    Spawn: FnMut(u32) -> std::io::Result<T>,
{
    match spawn(WINDOWS_BREAKAWAY_CREATION_FLAGS) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Hosted runners can forbid breakaway from their job object.
            // https://github.com/actions/runner/issues/595
            spawn(WINDOWS_DETACHED_CREATION_FLAGS)
        }
        result => result,
    }
}

fn detached_command(exe: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

/// Spawns the Gateway detached from the desktop app's lifetime. The reaper
/// thread records the child's exit status in `exit`, so a launch wait can
/// stop as soon as the child dies instead of running out its budget.
pub(super) fn spawn_detached(exe: &Path, exit: Arc<OnceLock<ExitStatus>>) -> std::io::Result<u32> {
    #[cfg(windows)]
    let mut child = spawn_detached_windows_with(|flags| {
        use std::os::windows::process::CommandExt as _;
        detached_command(exe).creation_flags(flags).spawn()
    })?;
    #[cfg(unix)]
    let mut child = {
        use std::os::unix::process::CommandExt as _;
        let mut command = detached_command(exe);
        command.process_group(0);
        command.spawn()?
    };
    #[cfg(not(any(windows, unix)))]
    let mut child = detached_command(exe).spawn()?;
    let child_pid = child.id();
    if let Err(error) = std::thread::Builder::new().spawn(move || {
        if let Ok(status) = child.wait() {
            let _ = exit.set(status);
        }
    }) {
        eprintln!("could not spawn the gateway reaper thread; the child goes unreaped: {error}");
    }
    Ok(child_pid)
}
