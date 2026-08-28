use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pl_core::config::SkillsConfig;
use pl_core::remote::{
    RemoteSkillProvider, SshAuth, SshConnectionState, SshManager, SshServerProfile,
};
use pl_core::skill::{SkillLoadInvocation, SkillProviderRequest, SkillRegistry};
use pl_core::{
    CommandProcessFinalResult, CommandProcessLifecycle, CommandProcessManager, CommandStartRequest,
    CommandWriteRequest, ExecutionBackend, ExecutionRequest, WorkspaceFileBackend,
    WorkspaceFileListRequest, WorkspaceFileReadBytesRequest, WorkspaceFileReadRequest,
    WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
use pl_lsp::{
    LspCatalogServer, LspCommandSpec, LspHostBackend, LspMissingComponent, LspProbeOutcome,
    LspQuery, LspQueryOperation, LspRepairError, LspResolvedCommand, LspRuntimeRegistry,
    LspServerCatalog, LspServerDefinition, LspServerDriver,
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = required("PURE_SSH_TEST_SERVER")?;
    let workspace = required("PURE_SSH_TEST_WORKSPACE")?;
    let helper = PathBuf::from(required("PURE_REMOTE_HELPER_AARCH64")?);
    let manager = SshManager::new(Some(helper), None);
    let profile = SshServerProfile {
        id: "validation".to_string(),
        name: "aarch64 validation".to_string(),
        host,
        port: 22,
        username: "root".to_string(),
        auth: SshAuth::AgentOrKey {
            identity_file: None,
        },
    };
    manager.save_server(profile).await?;
    let snapshot = manager.test_connection("validation").await?;
    println!("connection={:?}", snapshot.state);

    let host = manager
        .open_workspace_host("validation", workspace.clone())
        .await?;
    let files = Arc::new(host.files.clone());
    files.create_directory("nested".to_string(), None).await?;
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "nested/input.txt".to_string(),
            cwd: None,
            content: "alpha\nbeta\n".to_string(),
        })
        .await?;
    assert_eq!(
        files
            .read_text(WorkspaceFileReadRequest {
                path: "nested/input.txt".to_string(),
                cwd: None,
            })
            .await?,
        "alpha\nbeta\n"
    );
    let stat = files
        .stat(WorkspaceFileStatRequest {
            path: "nested/input.txt".to_string(),
            cwd: None,
        })
        .await?;
    assert!(stat.is_file);
    files
        .copy_path(
            "nested/input.txt".to_string(),
            "nested/copied.txt".to_string(),
            None,
            false,
        )
        .await?;
    files
        .rename_path(
            "nested/copied.txt".to_string(),
            "nested/moved.txt".to_string(),
            None,
        )
        .await?;
    let listing = files
        .list(WorkspaceFileListRequest {
            path: ".".to_string(),
            cwd: None,
            glob: "**/*.txt".to_string(),
            max_files: 20,
            include_dirs: true,
        })
        .await?;
    assert_eq!(
        listing.files,
        vec![
            "nested/input.txt".to_string(),
            "nested/moved.txt".to_string()
        ]
    );

    let processes = CommandProcessManager::new(Arc::new(host.commands.clone()));
    let first = processes
        .start(CommandStartRequest {
            command: "printf 'stdout-one\\n'; printf 'stderr-one\\n' >&2; read line; printf 'stdin:%s\\n' \"$line\"".to_string(),
            cwd: None,
            allow_workspace_escape: false,
            timeout: Duration::from_secs(20),
            yield_time: Duration::from_millis(500),
            max_output_chars: 64 * 1024,
            session_id: "validation".to_string(),
            tool_id: "observable-exec".to_string(),
            call_id: "call-1".to_string(),
            cancellation_token: None,
            output_observer: None,
        })
        .await?;
    assert!(matches!(first.state, CommandProcessLifecycle::Running(_)));
    assert_eq!(first.stdout.content, "stdout-one\n");
    assert_eq!(first.stderr.content, "stderr-one\n");
    let process_id = first.process_id.expect("running process id");
    let final_output = processes
        .write_stdin(CommandWriteRequest {
            process_id,
            chars: "hello\n".to_string(),
            yield_time: Duration::from_secs(5),
            max_output_chars: 64 * 1024,
        })
        .await?;
    assert!(final_output.state.final_result().is_some());
    assert_eq!(final_output.stdout.content, "stdin:hello\n");
    assert_eq!(final_output.stderr.content, "");
    let capture = files
        .read_text(WorkspaceFileReadRequest {
            path: final_output.capture_file.to_string_lossy().into_owned(),
            cwd: None,
        })
        .await?;
    assert!(capture.contains("stdout-one"));
    assert!(capture.contains("stderr-one"));
    assert!(capture.contains("stdin:hello"));

    validate_git(&host.git, files.as_ref()).await?;
    validate_skills(files.clone()).await?;
    validate_image_bytes(files.as_ref()).await?;
    validate_lsp_backend(host.clone(), files.as_ref()).await?;

    let mut connection_state = manager.subscribe_state("validation").await;
    let tree = processes
        .start(CommandStartRequest {
            command: "sleep 60 & echo $! > tree.pid; wait".to_string(),
            cwd: None,
            allow_workspace_escape: false,
            timeout: Duration::from_secs(90),
            yield_time: Duration::from_millis(300),
            max_output_chars: 64 * 1024,
            session_id: "validation".to_string(),
            tool_id: "disconnect-tree".to_string(),
            call_id: "call-tree".to_string(),
            cancellation_token: None,
            output_observer: None,
        })
        .await?;
    let process_id = tree.process_id.expect("tree process must still be running");
    manager.reconnect_server("validation").await?;
    let interrupted = processes
        .write_stdin(CommandWriteRequest {
            process_id,
            chars: String::new(),
            yield_time: Duration::from_secs(5),
            max_output_chars: 64 * 1024,
        })
        .await?;
    assert!(matches!(
        interrupted.state.final_result(),
        Some(CommandProcessFinalResult::Failed { .. })
    ));
    wait_until_ready(&mut connection_state).await?;

    let reconnected = manager.open_workspace_host("validation", workspace).await?;
    let reconnected_files = Arc::new(reconnected.files);
    let cleanup_check = run_remote(
        &reconnected.git,
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "if kill -0 \"$(cat tree.pid)\" 2>/dev/null; then echo alive; else echo dead; fi"
                .to_string(),
        ],
    )
    .await?;
    assert_eq!(cleanup_check.stdout.trim(), "dead");

    for path in [
        "nested",
        "skills",
        ".git",
        ".pure",
        "pixel.png",
        "fixture.py",
        "lsp_fixture.py",
        "tracked.txt",
        "tree.pid",
    ] {
        if reconnected_files
            .stat_optional(path.to_string(), None)
            .await?
            .is_some()
        {
            reconnected_files
                .remove_path(path.to_string(), None, true)
                .await?;
        }
    }

    manager.disconnect_server("validation").await;
    println!("remote SSH validation passed");
    Ok(())
}

async fn validate_git(
    backend: &pl_core::remote::RemoteExecutionBackend,
    files: &pl_core::remote::RemoteWorkspaceFileBackend,
) -> anyhow::Result<()> {
    assert_eq!(
        run_remote(
            backend,
            vec!["git".to_string(), "init".to_string(), "-q".to_string()]
        )
        .await?
        .status,
        0
    );
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "tracked.txt".to_string(),
            cwd: None,
            content: "remote git\n".to_string(),
        })
        .await?;
    for args in [
        vec!["git", "config", "user.email", "remote@example.invalid"],
        vec!["git", "config", "user.name", "Remote Validation"],
        vec!["git", "add", "tracked.txt"],
        vec!["git", "commit", "-q", "-m", "validation"],
        vec![
            "git",
            "worktree",
            "add",
            "-q",
            "-b",
            "validation/worktree",
            ".pure/worktree",
        ],
        vec!["git", "worktree", "remove", "-f", ".pure/worktree"],
    ] {
        let output = run_remote(backend, args.into_iter().map(str::to_string).collect()).await?;
        anyhow::ensure!(output.status == 0, "git failed: {}", output.stderr);
    }
    Ok(())
}

async fn validate_skills(
    files: Arc<pl_core::remote::RemoteWorkspaceFileBackend>,
) -> anyhow::Result<()> {
    files
        .create_directory("skills/remote".to_string(), None)
        .await?;
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "skills/remote/SKILL.md".to_string(),
            cwd: None,
            content: "---\nname: remote-validation\ndescription: Validate remote Skills.\n---\n\n# Remote validation\n"
                .to_string(),
        })
        .await?;
    files
        .create_directory("skills/remote/references".to_string(), None)
        .await?;
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "skills/remote/references/info.md".to_string(),
            cwd: None,
            content: "remote reference".to_string(),
        })
        .await?;
    let registry = SkillRegistry::new();
    let _registration = registry.register(Arc::new(RemoteSkillProvider::new(files)?))?;
    let catalog = registry
        .discover(SkillProviderRequest {
            workspace_root: PathBuf::from("remote-validation"),
            config: SkillsConfig::default(),
            system_dir: None,
            cancellation: CancellationToken::new(),
        })
        .await?;
    assert!(catalog.find("remote-validation").is_some());
    let loaded = catalog
        .load(
            "remote-validation",
            SkillLoadInvocation::Model,
            CancellationToken::new(),
        )
        .await?;
    assert!(loaded.content.contains("# Remote validation"));
    assert_eq!(
        catalog
            .read_resource(
                "remote-validation",
                "references/info.md",
                SkillLoadInvocation::Model,
                CancellationToken::new(),
            )
            .await?,
        "remote reference"
    );
    Ok(())
}

async fn validate_image_bytes(
    files: &pl_core::remote::RemoteWorkspaceFileBackend,
) -> anyhow::Result<()> {
    let png = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    )?;
    files
        .write_bytes_atomic("pixel.png".to_string(), None, &png)
        .await?;
    assert_eq!(
        files
            .read_bytes(WorkspaceFileReadBytesRequest {
                path: "pixel.png".to_string(),
                cwd: None,
                max_bytes: 1024,
            })
            .await?,
        png
    );
    Ok(())
}

async fn validate_lsp_backend(
    host: pl_core::remote::RemoteWorkspaceHost,
    files: &pl_core::remote::RemoteWorkspaceFileBackend,
) -> anyhow::Result<()> {
    let server = r#"import json
import sys

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line == b"\r\n":
            break
        name, value = line.decode().split(":", 1)
        if name.lower() == "content-length":
            length = int(value.strip())
    return json.loads(sys.stdin.buffer.read(length))

def send(value):
    body = json.dumps(value, separators=(",", ":")).encode()
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "exit":
        break
    if "id" not in message:
        continue
    if method == "initialize":
        result = {"capabilities": {"documentSymbolProvider": True}}
    elif method == "textDocument/documentSymbol":
        result = [{"name": "remote_symbol", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}}, "selectionRange": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}}}]
    elif method == "shutdown":
        result = None
    else:
        result = None
    send({"jsonrpc": "2.0", "id": message["id"], "result": result})
"#;
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "lsp_fixture.py".to_string(),
            cwd: None,
            content: server.to_string(),
        })
        .await?;
    files
        .write_text(WorkspaceFileWriteRequest {
            path: "fixture.py".to_string(),
            cwd: None,
            content: "def remote_symbol():\n    pass\n".to_string(),
        })
        .await?;

    let mut catalog = LspServerCatalog::empty();
    catalog.insert(LspCatalogServer {
        definition: LspServerDefinition {
            id: "remote-python-fixture".to_string(),
            display_name: "Remote Python fixture".to_string(),
            language_ids: vec!["python".to_string()],
            extensions: vec![".py".to_string()],
            detection: vec!["fixture.py".to_string()],
            command: LspCommandSpec {
                program: "python3".to_string(),
                args: vec!["-u".to_string(), "lsp_fixture.py".to_string()],
            },
            operations: vec![LspQueryOperation::DocumentSymbol],
        },
        driver: Arc::new(ValidationLspDriver),
    })?;
    let registry = LspRuntimeRegistry::with_catalog(catalog);
    let workspace_root = PathBuf::from(files.canonical_path());
    let host: Arc<dyn LspHostBackend> = Arc::new(host);
    registry
        .reconcile_workspace_membership_with_host(&workspace_root, host)
        .await;
    registry.probe_lsp_server(&workspace_root).await;
    let result = registry
        .query_in_workspace(
            &workspace_root,
            LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(workspace_root.join("fixture.py")),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("python".to_string()),
            },
        )
        .await?;
    assert!(result.success);
    assert!(result.result.contains("remote_symbol"));
    registry.shutdown().await;
    Ok(())
}

struct ValidationLspDriver;

impl LspServerDriver for ValidationLspDriver {
    fn probe<'a>(
        &'a self,
        _command: &'a LspResolvedCommand,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, LspProbeOutcome> {
        Box::pin(std::future::ready(LspProbeOutcome::Ready {
            version: "fixture".to_string(),
        }))
    }

    fn repair<'a>(
        &'a self,
        _component: &'a LspMissingComponent,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, Result<(), LspRepairError>> {
        Box::pin(std::future::ready(Err(LspRepairError::NotSupported)))
    }
}

async fn run_remote(
    backend: &pl_core::remote::RemoteExecutionBackend,
    command: Vec<String>,
) -> anyhow::Result<pl_core::ExecutionOutput> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("remote command must not be empty"))?;
    backend
        .run(ExecutionRequest {
            program: PathBuf::from(program),
            args: args.to_vec(),
            cwd: PathBuf::from("."),
            env: Default::default(),
            timeout: Some(Duration::from_secs(30)),
        })
        .await
        .map_err(anyhow::Error::msg)
}

async fn wait_until_ready(
    state: &mut tokio::sync::watch::Receiver<SshConnectionState>,
) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            state
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("SSH state stream closed"))?;
            if matches!(&*state.borrow(), SshConnectionState::Ready { .. }) {
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("SSH reconnect did not become ready"))??;
    Ok(())
}

fn required(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("{name} must be set"))
}
