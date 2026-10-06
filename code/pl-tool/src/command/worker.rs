//! Host-chosen process-worker executable shared by local execution, MCP and LSP.

use std::path::Path;

/// A host-selected `pl-remote-helper` executable used as the per-child process supervisor.
///
/// The value owns whatever keeps the executable image alive (for example an open file
/// descriptor addressed through `/proc/self/fd`); cloning it must not release that
/// resource while any supervised child still relies on it.
pub(crate) trait LocalWorkerExecutable: std::fmt::Debug + Send + Sync {
    fn worker_path(&self) -> &Path;
}

impl<T: AsRef<Path> + std::fmt::Debug + Send + Sync> LocalWorkerExecutable for T {
    fn worker_path(&self) -> &Path {
        self.as_ref()
    }
}
