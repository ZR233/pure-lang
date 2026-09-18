//! SSH 项目根会话 worktree 的真实 localhost 验收。
//!
//! 前置条件：本机 sshd 监听 `localhost:22`、当前用户可凭 `~/.ssh/id_ed25519` 免密登录，
//! 且 `localhost` 的 host key 已在 `~/.ssh/known_hosts` 中（例如
//! `ssh-keyscan localhost >> ~/.ssh/known_hosts`），另有
//! `--features embedded-remote-helpers` 提供的 helper 资产。仅手动执行：
//!
//! ```text
//! cargo test -p pl-studio-runtime --features embedded-remote-helpers \
//!   --test ssh_worktree_acceptance -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use pl_protocol::studio::{CreateThreadRequest, StudioPromptInput};
use pl_protocol::{ThreadModeId, ThreadWorkspaceMode};
use pl_studio_runtime::{
    SshConfigFile, SshServerProfile, StudioHostKind, StudioRecoveryWorktreeOwner, StudioRuntime,
    StudioRuntimeOptions,
};

const ALIAS: &str = "pure-worktree-acceptance";
const REMOTE_REPO: &str = "pure-ssh-worktree-acceptance-repo";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires local sshd with passwordless localhost and embedded remote helpers"]
async fn ssh_project_root_session_worktree_is_created_on_the_remote_repository()
-> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let repository = std::env::temp_dir().join(REMOTE_REPO);
    reset_repository(&repository);
    let options = || StudioRuntimeOptions {
        studio_home: Some(home.path().to_path_buf()),
        host: StudioHostKind::Test,
    };
    let _config_guard = SshConfigGuard;

    let runtime = StudioRuntime::with_options(options()).await?;
    runtime.start_runtime().await?;
    let identity = format!("{}/.ssh/id_ed25519", std::env::var("HOME")?);
    let profile = runtime
        .save_ssh_server(SshServerProfile {
            alias: ALIAS.to_string(),
            host_name: "localhost".to_string(),
            port: 22,
            username: whoami(),
            identity_file: Some(identity),
        })
        .await?;
    let project = runtime
        .open_remote_project(&profile.alias, repository.to_string_lossy().into_owned())
        .await?;
    println!("ACCEPT remote project: {} -> {}", project.id, project.path);

    let created = runtime
        .create_thread_command(
            project.id.clone(),
            CreateThreadRequest {
                title: None,
                input: StudioPromptInput {
                    input_id: "request-1".into(),
                    text: "worktree acceptance".into(),
                    attachment_draft_ids: Vec::new(),
                },
                mode: ThreadModeId::SIMPLE.to_string(),
                workspace_mode: ThreadWorkspaceMode::Worktree,
            },
        )
        .await;
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            println!("ACCEPT create failed: {error:#}");
            println!("ACCEPT recovery: {:#?}", runtime.recovery_issues());
            panic!("远程项目必须能创建 worktree 会话：{error:#}");
        }
    };
    let thread_id = created.thread.id.clone();
    assert_eq!(
        created.thread.workspace_mode,
        ThreadWorkspaceMode::Worktree,
        "命令回执必须是 worktree 会话"
    );
    assert_eq!(
        runtime.read_thread(&thread_id).await?.workspace_mode,
        ThreadWorkspaceMode::Worktree,
        "发布的 Thread 目录事实必须记录 workspaceMode=worktree"
    );

    // 远端物理 worktree：`<repo>/.anywork/worktrees/<thread-id>/session`，HEAD 等于创建时的 base。
    // localhost 验收与远端同一台主机，因此物理路径用本地文件系统复核。
    let base_commit = git(&repository, &["rev-parse", "HEAD"]);
    let worktree: PathBuf = Path::new(&project.path)
        .join(".anywork/worktrees")
        .join(&thread_id)
        .join("session");
    assert!(
        worktree.is_dir(),
        "远端仓库根下必须存在物理 worktree：{}",
        worktree.display()
    );
    assert_eq!(
        git(&worktree, &["rev-parse", "HEAD"]),
        base_commit,
        "worktree 必须从创建时解析出的 HEAD 派生"
    );
    assert!(
        !Path::new(&project.path).join("session").exists(),
        "worktree 不得落到与仓库根不一致的基准上"
    );
    println!("ACCEPT remote worktree: {}", worktree.display());

    // SSH 离线后按 durable lease 对账：lease 记录的 `ssh_alias` 让 preview 必须经远端 backend
    // 失败并保留现场；若 lease 未记录 `ssh_alias`，本地 backend 会在同一路径上成功预览。
    runtime.delete_ssh_server(ALIAS).await?;
    runtime.shutdown().await;
    let reopened = StudioRuntime::with_options(options()).await?;
    reopened.start_runtime().await?;
    reopened.retry_recovery().await?;
    let lease_issue_id = format!("worktree-lease-{thread_id}");
    let issue = wait_for_recovery_issue(&reopened, &lease_issue_id)
        .await
        .unwrap_or_else(|| {
            panic!(
                "SSH 离线后必须发布该 lease 的 Recovery：{:#?}",
                reopened.recovery_issues()
            )
        });
    let preview = issue
        .worktree
        .as_ref()
        .unwrap_or_else(|| panic!("Recovery 必须携带 worktree 预览：{issue:#?}"));
    assert_eq!(
        preview.owner_kind,
        StudioRecoveryWorktreeOwner::Session,
        "根会话 worktree 的 lease 归属必须是 session"
    );
    assert_eq!(
        PathBuf::from(&preview.repository_root),
        Path::new(&project.path)
    );
    assert_eq!(PathBuf::from(&preview.path), worktree);
    assert_eq!(preview.base_commit, base_commit);
    assert!(
        issue.message.contains("worktree preview failed"),
        "SSH 离线时远端 lease 的 preview 必须失败并保留现场：{}",
        issue.message
    );
    assert!(
        preview.head_commit.is_none(),
        "SSH 离线时不得产生远端预览结果：{preview:#?}"
    );
    reopened.shutdown().await;

    cleanup_repository(&repository);
    println!("ACCEPT ALL PASS");
    Ok(())
}

/// 启动期对账是后台任务，发布是异步的；轮询到该 lease 的 Recovery 条目出现为止。
async fn wait_for_recovery_issue(
    runtime: &StudioRuntime,
    issue_id: &str,
) -> Option<pl_studio_runtime::StudioRecoveryIssue> {
    for _ in 0..150 {
        if let Some(issue) = runtime
            .recovery_issues()
            .into_iter()
            .find(|issue| issue.id == issue_id)
        {
            return Some(issue);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    None
}

/// 结束或 panic 时移除写入 `~/.ssh/config` 的管理块，避免污染用户配置。
///
/// `Drop` 不能 await，因此在一次性线程上建立最小 runtime 复用 canonical 的
/// `SshConfigFile::remove_managed`，不手写标记解析。
struct SshConfigGuard;

impl Drop for SshConfigGuard {
    fn drop(&mut self) {
        let _ = std::thread::spawn(|| {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async {
                if let Ok(config) = SshConfigFile::user_default() {
                    let _ = config.remove_managed(ALIAS).await;
                }
            });
        })
        .join();
    }
}

fn reset_repository(path: &Path) {
    cleanup_repository(path);
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init"]);
    git(
        path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
}

fn cleanup_repository(path: &Path) {
    let _ = Command::new("git")
        .current_dir(path)
        .args(["worktree", "prune"])
        .output();
    std::fs::remove_dir_all(path).ok();
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git must launch");
    assert!(
        output.status.success(),
        "git {} in {} failed: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output must be UTF-8")
        .trim()
        .to_string()
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "zhourui".to_string())
}
