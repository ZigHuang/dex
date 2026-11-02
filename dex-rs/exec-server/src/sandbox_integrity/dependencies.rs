//! Shared inventory entries and filesystem facts, independent of any checker.

use dex_utils_absolute_path::AbsolutePathBuf;
use std::fs;
use std::io;
use std::path::PathBuf;

pub(super) type Dependency = (&'static str, io::Result<PathBuf>);

pub(super) struct DependencyTarget {
    pub path: AbsolutePathBuf,
    pub metadata: fs::Metadata,
    pub resolved_path: Option<AbsolutePathBuf>,
}

impl DependencyTarget {
    pub(super) fn inspect(path: io::Result<PathBuf>) -> io::Result<Self> {
        let path = AbsolutePathBuf::from_absolute_path(path?)?;
        Ok(Self {
            metadata: fs::metadata(&path)?,
            resolved_path: path.canonicalize().ok(),
            path,
        })
    }
}

pub(super) fn dex_executable() -> Dependency {
    ("dex", std::env::current_exe())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn dex_and_launcher_dependencies(
    request: &dex_sandboxing::SandboxExecRequest,
) -> Vec<Dependency> {
    let mut dependencies = vec![dex_executable()];
    if let Some(exe) = request.command.first() {
        dependencies.push(("sandbox_launcher", Ok(PathBuf::from(exe))));
    }
    dependencies
}
