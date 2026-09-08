//! Pure SSH remote helper 的最小 stdio 服务。

pub use pl_protocol::remote::RemoteError;

/// Host-side physical process supervision, shared by local execution and the SSH service.
#[cfg(target_os = "linux")]
pub mod client;

#[cfg(target_os = "linux")]
mod codec;
#[cfg(target_os = "linux")]
mod path;
#[cfg(target_os = "linux")]
mod server;

#[cfg(target_os = "linux")]
pub use server::run_stdio;

/// Failure of the physical SSH helper service.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("remote helper stdio failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote helper resource cleanup failed: {0:?}")]
    Cleanup(RemoteError),
    #[error("the SSH helper executable requires Linux")]
    UnsupportedPlatform,
}

/// The bundled SSH helper targets Linux; other hosts may still use its client adapters.
/// # Errors
/// Always reports UnsupportedPlatform on non-Linux targets.
#[cfg(not(target_os = "linux"))]
pub async fn run_stdio() -> Result<(), ServerError> {
    Err(ServerError::UnsupportedPlatform)
}
