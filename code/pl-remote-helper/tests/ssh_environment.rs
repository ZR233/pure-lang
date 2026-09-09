//! Real helper stdio contract: startup files must affect every spawned process.
#![cfg(target_os = "linux")]

use pl_protocol::remote::*;
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
};

struct Helper {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Helper {
    async fn start(home: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pl-remote-helper"))
            .env("HOME", home)
            .env("PURE_ENV_REMOVED", "inherited")
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut helper = Self {
            input: child.stdin.take().unwrap(),
            output: child.stdout.take().unwrap(),
            child,
        };
        helper
            .send(RemoteRequest::Hello {
                protocol_version: REMOTE_PROTOCOL_VERSION,
            })
            .await;
        assert!(matches!(
            helper.read().await.0,
            RemoteMessage::Response(RemoteResponse::Hello(_))
        ));
        helper
    }
    async fn send(&mut self, request: RemoteRequest) {
        let bytes = serde_json::to_vec(&RemoteFrameHeader {
            request_id: Some(1),
            message: RemoteMessage::Request(request),
            body_len: 0,
        })
        .unwrap();
        self.input.write_u32(bytes.len() as u32).await.unwrap();
        self.input.write_all(&bytes).await.unwrap();
        self.input.flush().await.unwrap();
    }
    async fn read(&mut self) -> (RemoteMessage, Vec<u8>) {
        tokio::time::timeout(Duration::from_secs(25), async {
            let length = self.output.read_u32().await.unwrap() as usize;
            assert!(
                length <= REMOTE_MAX_HEADER_BYTES,
                "startup output contaminated the protocol"
            );
            let mut header = vec![0; length];
            self.output.read_exact(&mut header).await.unwrap();
            let header: RemoteFrameHeader = serde_json::from_slice(&header).unwrap();
            let mut body = vec![0; header.body_len];
            self.output.read_exact(&mut body).await.unwrap();
            (header.message, body)
        })
        .await
        .expect("helper response deadline")
    }
    async fn exec(
        &mut self,
        home: &Path,
        id: &str,
        environment: BTreeMap<String, String>,
    ) -> Vec<u8> {
        self.send(RemoteRequest::OpenWorkspace {
            path: home.to_str().unwrap().into(),
        })
        .await;
        let (RemoteMessage::Response(RemoteResponse::WorkspaceOpened(workspace)), _) =
            self.read().await
        else {
            panic!("workspace response")
        };
        self.send(RemoteRequest::Spawn(RemoteSpawnRequest {
            process_id: id.into(),
            workspace_id: workspace.workspace_id,
            command:
                "printf '%s|%s|%s' \"$PURE_ENV_MARKER\" \"${PURE_ENV_REMOVED-unset}\" \"$PATH\"; if declare -F pure_env_function >/dev/null; then printf function-leaked; fi"
                    .into(),
            cwd: ".".into(),
            environment,
            capture_path: format!("capture-{id}"),
        }))
        .await;
        let mut bytes = Vec::new();
        loop {
            match self.read().await {
                (RemoteMessage::Event(RemoteEvent::ProcessOutput(_)), body) => bytes.extend(body),
                (RemoteMessage::Event(RemoteEvent::ProcessExit(exit)), _) => {
                    assert_eq!(exit.exit_code, Some(0));
                    break;
                }
                (RemoteMessage::Response(RemoteResponse::ProcessSpawned { .. }), _) => {}
                other => panic!("unexpected spawn response: {other:?}"),
            }
        }
        bytes
    }
    async fn close(mut self) {
        self.send(RemoteRequest::Shutdown).await;
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[tokio::test]
async fn bashrc_exports_unset_path_and_reconnect_are_connection_scoped() {
    let home = tempfile::tempdir().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(
        home.path().join("startup-hook"),
        "export PURE_ENV_MARKER=unexpected-reload\n",
    )
    .unwrap();
    std::fs::write(&rc, "case $- in *i*) ;; *) return;; esac\nprintf 'startup noise\\n'\nprintf 'stderr noise\\n' >&2\nexport PURE_ENV_MARKER='hello world\nsecond line'\nunset PURE_ENV_REMOVED\nexport PATH=/usr/bin:/bin\nexport BASH_ENV=\"$HOME/startup-hook\"\npure_env_function() { :; }\nexport -f pure_env_function\n").unwrap();
    let mut helper = Helper::start(home.path()).await;
    let first = helper.exec(home.path(), "first", BTreeMap::new()).await;
    std::fs::write(
        &rc,
        "export PURE_ENV_MARKER=refreshed\nunset PURE_ENV_REMOVED\nexport PATH=/bin\n",
    )
    .unwrap();
    let second = helper.exec(home.path(), "second", BTreeMap::new()).await;
    let override_env = helper
        .exec(
            home.path(),
            "override",
            BTreeMap::from([
                ("PURE_ENV_MARKER".into(), "override".into()),
                ("PATH".into(), "/bin".into()),
            ]),
        )
        .await;
    helper.close().await;
    let mut helper = Helper::start(home.path()).await;
    let refreshed = helper.exec(home.path(), "refreshed", BTreeMap::new()).await;
    helper.close().await;
    assert_eq!(
        String::from_utf8(first).unwrap(),
        "hello world\nsecond line|unset|/usr/bin:/bin"
    );
    assert_eq!(
        String::from_utf8(second).unwrap(),
        "hello world\nsecond line|unset|/usr/bin:/bin"
    );
    assert_eq!(
        String::from_utf8(override_env).unwrap(),
        "override|unset|/bin"
    );
    assert_eq!(
        String::from_utf8(refreshed).unwrap(),
        "refreshed|unset|/bin"
    );
}

#[tokio::test]
async fn startup_failure_is_reported_without_leaking_script_output() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".bashrc"),
        "printf 'private-startup-value' >&2\nexit 17\n",
    )
    .unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        Command::new(env!("CARGO_BIN_EXE_pl-remote-helper"))
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("shell exited before successful collection"),
        "{error}"
    );
    assert!(!error.contains("private-startup-value"));
}

#[tokio::test]
async fn startup_timeout_reaps_background_descendants() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".bashrc"),
        "sleep 120 &\nprintf '%s' $! > \"$HOME/descendant-pid\"\nwait\n",
    )
    .unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(25),
        Command::new(env!("CARGO_BIN_EXE_pl-remote-helper"))
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("initialization timed out"));
    let pid = std::fs::read_to_string(home.path().join("descendant-pid")).unwrap();
    assert!(
        !Path::new("/proc").join(pid).exists(),
        "startup descendant must be reaped before helper exits"
    );
}
