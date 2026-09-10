//! Cancellation helper for a child still owned by the non-Linux command backend.

use pl_remote_helper::process::configure_background_command;
use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;
use tokio::process::Command as TokioCommand;

#[cfg(not(target_os = "linux"))]
pub(crate) async fn terminate_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        let mut command = TokioCommand::new("taskkill");
        command
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut command);
        let _ = command.status().await;
    }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let mut terminate = TokioCommand::new("kill");
        terminate
            .args(["-TERM", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut terminate);
        let delivered = terminate
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false);
        if delivered {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let mut kill = TokioCommand::new("kill");
        kill.args(["-KILL", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut kill);
        let _ = kill.status().await;
    }
}
