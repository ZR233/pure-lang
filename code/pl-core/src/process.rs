use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

use tokio::process::Command as TokioCommand;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) fn configure_background_command(command: &mut TokioCommand) {
    command.kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

pub(crate) fn configure_background_std_command(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

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

pub(crate) fn terminate_process_tree_sync(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut command);
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let mut terminate = std::process::Command::new("kill");
        terminate
            .args(["-TERM", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut terminate);
        let delivered = terminate
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if delivered {
            std::thread::sleep(Duration::from_secs(2));
        }
        let mut kill = std::process::Command::new("kill");
        kill.args(["-KILL", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut kill);
        let _ = kill.status();
    }
}
