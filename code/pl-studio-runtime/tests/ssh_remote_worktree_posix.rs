//! 远端 worktree 会话创建的跨端路径 POSIX 化真实 SSH 验收。
//!
//! 复现现场缺陷：客户端宿主（Windows）把 `WorktreeManager::allocate_path` 的结果（仓库根为
//! POSIX，`Path::join` 注入 `\`）原样作为 `git worktree add` 的路径参数发给远端，git 在错误
//! 位置创建物理 worktree，随后 `resolve_head` 的 cwd 归一化指向不存在的目录而失败。本测试以
//! 真实 `SshManager` + 真实 `RemoteWorktreeBackend`、用 Windows 形态 target 驱动，断言远端物理
//! 目录落在 `<repo>/.anywork/worktrees/<id>/session`、HEAD 等于 base，并且清理成功。
//!
//! 前置条件：
//! - 目标主机可免密 SSH 登录，且 host key 已在 `~/.ssh/known_hosts`（例如
//!   `ssh-keyscan <host> >> ~/.ssh/known_hosts`）；
//! - 已构建远端 helper（`cargo xtask build-remote-helper --target x86_64-unknown-linux-musl`），
//!   默认取自 `dist/remote-helper/x86_64-unknown-linux-musl/pl-remote-helper`（含相邻 `.sha256`）。
//!
//! 环境变量（主机可配置，不硬编码地址）：
//! - `PURE_SSH_TEST_SERVER`（主机名/IP，必填）、`PURE_SSH_TEST_USERNAME`（必填）；
//! - `PURE_SSH_TEST_PORT`（默认 22）、`PURE_SSH_TEST_IDENTITY`（可选私钥）；
//! - `PURE_REMOTE_HELPER_BIN`（可选，覆盖 helper 二进制路径）。
//!
//! 仅手动执行：
//!
//! ```text
//! PURE_SSH_TEST_SERVER=10.3.10.194 PURE_SSH_TEST_USERNAME=runner \
//!   cargo test -p pl-studio-runtime --test ssh_remote_worktree_posix -- --ignored --nocapture
//! ```
//!
//! 该测试只使用显式临时 ssh config（`-F`），不读写用户 `~/.ssh/config`；结束时会经
//! `SshManager::delete_server` 回收自己的管理块并删除远端临时仓库。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pl_studio_runtime::agent::worktree::{RemoteWorktreeBackend, WorktreeBackend};
use pl_tool::execution::{ExecutionBackend, ExecutionOutput, ExecutionRequest};
use pl_tool::remote::{RemoteWorkspaceHost, SshConfigFile, SshManager, SshServerProfile};

const REPO_PREFIX: &str = "pure-posix-worktree-acceptance";
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

fn helper_bin() -> PathBuf {
    std::env::var_os("PURE_REMOTE_HELPER_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../dist/remote-helper/x86_64-unknown-linux-musl/pl-remote-helper")
        })
}

async fn run_program(
    host: &RemoteWorkspaceHost,
    program: &str,
    cwd: &str,
    args: Vec<String>,
) -> ExecutionOutput {
    host.git
        .run(ExecutionRequest {
            program: PathBuf::from(program),
            args,
            cwd: PathBuf::from(cwd),
            env: BTreeMap::new(),
            timeout: Some(GIT_TIMEOUT),
        })
        .await
        .expect("remote command must launch")
}

async fn run_git(host: &RemoteWorkspaceHost, cwd: &str, args: &[&str]) -> ExecutionOutput {
    run_program(
        host,
        "git",
        cwd,
        args.iter().map(|value| value.to_string()).collect(),
    )
    .await
}

async fn expect_git(host: &RemoteWorkspaceHost, cwd: &str, args: &[&str]) -> String {
    let output = run_git(host, cwd, args).await;
    assert!(
        output.status == 0,
        "git {} failed ({}): {}",
        args.join(" "),
        output.status,
        output.stderr
    );
    output.stdout.trim().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a reachable SSH host with passwordless auth and a built remote helper; see module docs"]
async fn remote_worktree_create_is_posix_for_windows_shaped_target() -> anyhow::Result<()> {
    let host_name = std::env::var("PURE_SSH_TEST_SERVER")?;
    let username = std::env::var("PURE_SSH_TEST_USERNAME")?;
    let port = std::env::var("PURE_SSH_TEST_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(22);
    let identity = std::env::var("PURE_SSH_TEST_IDENTITY").ok();
    let helper = helper_bin();
    anyhow::ensure!(
        helper.is_file(),
        "remote helper binary is missing: {}",
        helper.display()
    );

    let alias = format!("{REPO_PREFIX}-{}", std::process::id());
    let config_dir = tempfile::tempdir()?;
    let config_path = config_dir.path().join("ssh_config");
    let manager = Arc::new(
        SshManager::new(None, Some(helper)).with_ssh_config(SshConfigFile::at(&config_path)),
    );
    manager
        .save_server(SshServerProfile {
            alias: alias.clone(),
            host_name,
            port,
            username,
            identity_file: identity,
        })
        .await?;

    let repo_abs = format!("/tmp/{REPO_PREFIX}-{}", std::process::id());
    let tmp_host = manager
        .open_workspace_host(&alias, "/tmp".to_string())
        .await?;
    run_program(&tmp_host, "rm", ".", vec!["-rf".into(), repo_abs.clone()]).await;
    run_program(&tmp_host, "mkdir", ".", vec!["-p".into(), repo_abs.clone()]).await;

    let repo_host = manager
        .open_workspace_host(&alias, repo_abs.clone())
        .await?;
    expect_git(&repo_host, ".", &["init", "-q"]).await;
    expect_git(
        &repo_host,
        ".",
        &[
            "-c",
            "user.name=Acceptance",
            "-c",
            "user.email=acceptance@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    )
    .await;
    let base = expect_git(&repo_host, ".", &["rev-parse", "HEAD"]).await;
    anyhow::ensure!(base.len() == 40, "unexpected base commit {base}");

    // Windows 形态 target：仓库根为 POSIX，客户端 `Path::join` 注入 `\`（与现场日志同形）。
    let thread_id = "thread-posix-acceptance";
    let branch = format!("pure-session-{thread_id}");
    let win_target = PathBuf::from(format!(
        "{repo_abs}\\.anywork/worktrees\\{thread_id}\\session"
    ));
    let expected = format!("{repo_abs}/.anywork/worktrees/{thread_id}/session");
    // 归一化后必须与期望 POSIX 目标一致（修复前该形式直接进入 git 参数）。
    assert_eq!(win_target.to_string_lossy().replace('\\', "/"), expected);
    println!("ACCEPT remote repo: {repo_abs} (base {base})");
    println!("ACCEPT windows-shaped target: {}", win_target.display());

    let backend =
        RemoteWorktreeBackend::new(manager.clone(), alias.clone(), PathBuf::from(&repo_abs))
            .map_err(|error| {
                anyhow::anyhow!("construct remote worktree backend failed: {error}")
            })?;
    backend
        .create_parent(Path::new(&repo_abs), &win_target)
        .await
        .map_err(|error| anyhow::anyhow!("create_parent failed: {error}"))?;
    backend
        .create(Path::new(&repo_abs), &branch, &win_target, &base)
        .await
        .map_err(|error| anyhow::anyhow!("create failed: {error}"))?;

    // 远端事实：物理 worktree 落在仓库根下的会话路径，HEAD 等于创建时 base。
    let session_host = manager
        .open_workspace_host(&alias, expected.clone())
        .await
        .map_err(|error| anyhow::anyhow!("session worktree is missing on the remote: {error}"))?;
    let head = expect_git(&session_host, ".", &["rev-parse", "HEAD"]).await;
    assert_eq!(head, base, "worktree HEAD 必须等于创建时解析出的 base");
    let listing = expect_git(&repo_host, ".", &["worktree", "list", "--porcelain"]).await;
    anyhow::ensure!(
        listing.contains(&expected),
        "git worktree list must register the POSIX session path: {listing}"
    );
    // 错误基准不得出现 worktree。
    anyhow::ensure!(
        manager
            .open_workspace_host(&alias, format!("{repo_abs}/session"))
            .await
            .is_err(),
        "worktree 不得落到与仓库根不一致的基准上"
    );
    println!("ACCEPT remote worktree: {expected} (HEAD {head})");

    // 清理顺序与 `WorktreeManager::discard` 一致：注销注册，仅在目录仍存在时删除目录，
    // 最后删除 Pure-owned 分支；注销被拒且目录仍在时不得绕过 Git 直接删除。
    let registration_error = backend
        .remove(Path::new(&repo_abs), &win_target, true)
        .await
        .err();
    let still_exists = backend
        .path_exists(&win_target)
        .await
        .map_err(|error| anyhow::anyhow!("path_exists failed: {error}"))?;
    anyhow::ensure!(
        !(registration_error.is_some() && still_exists),
        "worktree registration removal failed but the directory remains: {registration_error:?}"
    );
    if still_exists {
        backend
            .remove_leaf(Path::new(&repo_abs), &win_target)
            .await
            .map_err(|error| anyhow::anyhow!("remove_leaf failed: {error}"))?;
    }
    backend
        .delete_branch(Path::new(&repo_abs), &branch)
        .await
        .map_err(|error| anyhow::anyhow!("delete_branch failed: {error}"))?;
    // `open_workspace_host` 会复用进程内缓存的 workspace handle，因此清理效果必须直接问远端。
    let gone = run_program(&repo_host, "test", ".", vec!["-d".into(), expected.clone()]).await;
    anyhow::ensure!(gone.status != 0, "清理后远端 session 目录必须消失");

    // 回收远端临时仓库与本地 ssh config 管理块。
    run_program(&tmp_host, "rm", ".", vec!["-rf".into(), repo_abs.clone()]).await;
    manager.delete_server(&alias).await?;
    let _ = manager.shutdown().await;
    println!("ACCEPT ALL PASS");
    Ok(())
}
