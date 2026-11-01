use std::path::PathBuf;

use dex_utils_absolute_path::AbsolutePathBuf;

/// Runtime paths needed by exec-server child processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecServerRuntimePaths {
    /// Stable path to the Dex executable used to launch hidden helper modes.
    pub dex_self_exe: AbsolutePathBuf,
    /// Path to the Linux sandbox helper alias used when the platform sandbox
    /// needs to re-enter Dex by argv0.
    pub dex_linux_sandbox_exe: Option<AbsolutePathBuf>,
}

impl ExecServerRuntimePaths {
    pub fn from_optional_paths(
        dex_self_exe: Option<PathBuf>,
        dex_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let dex_self_exe = dex_self_exe.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Dex executable path is not configured",
            )
        })?;
        Self::new(dex_self_exe, dex_linux_sandbox_exe)
    }

    pub fn new(
        dex_self_exe: PathBuf,
        dex_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            dex_self_exe: absolute_path(dex_self_exe)?,
            dex_linux_sandbox_exe: dex_linux_sandbox_exe.map(absolute_path).transpose()?,
        })
    }
}

fn absolute_path(path: PathBuf) -> std::io::Result<AbsolutePathBuf> {
    AbsolutePathBuf::from_absolute_path(path.as_path())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}
