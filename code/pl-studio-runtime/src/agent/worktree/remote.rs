use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_tool::execution::{ExecutionBackend, ExecutionOutput, ExecutionRequest};
use pl_tool::remote::SshManager;

use pl_tool::git::GitPolicy;

use super::backend::{changed_files, checked_output, non_empty_head};
use super::{
    WorktreeBackend, WorktreeCreateFailure, WorktreeError, WorktreeStatus, remote_path_text,
};

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct RemoteWorktreeBackend {
    transport: Arc<dyn RemoteWorktreeTransport>,
    repo_root: PathBuf,
    policy: GitPolicy,
}

trait RemoteWorktreeTransport: std::fmt::Debug + Send + Sync {
    fn run<'a>(
        &'a self,
        request: ExecutionRequest,
    ) -> BoxFuture<'a, Result<ExecutionOutput, String>>;

    fn create_directory<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>>;

    fn path_exists<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<bool, String>>;

    fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>>;
}

#[derive(Debug)]
struct SshRemoteWorktreeTransport {
    ssh_manager: Arc<SshManager>,
    server_id: String,
    repo_root: PathBuf,
}

impl SshRemoteWorktreeTransport {
    async fn host(&self) -> Result<pl_tool::remote::RemoteWorkspaceHost, String> {
        self.ssh_manager
            .open_workspace_host(&self.server_id, remote_path_text(&self.repo_root))
            .await
            .map_err(|error| error.to_string())
    }
}

impl RemoteWorktreeTransport for SshRemoteWorktreeTransport {
    fn run<'a>(
        &'a self,
        request: ExecutionRequest,
    ) -> BoxFuture<'a, Result<ExecutionOutput, String>> {
        async move {
            self.host()
                .await?
                .git
                .run(request)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn create_directory<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
        async move {
            self.host()
                .await?
                .files
                .create_directory(relative_path, None)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<bool, String>> {
        async move {
            self.host()
                .await?
                .files
                .stat_optional(relative_path, None)
                .await
                .map(|stat| stat.is_some())
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
        async move {
            self.host()
                .await?
                .files
                .remove_path(relative_path, None, true)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }
}

impl RemoteWorktreeBackend {
    pub fn new(
        ssh_manager: Arc<SshManager>,
        server_id: impl Into<String>,
        repo_root: PathBuf,
    ) -> Self {
        let server_id = server_id.into();
        Self {
            transport: Arc::new(SshRemoteWorktreeTransport {
                ssh_manager,
                server_id,
                repo_root: repo_root.clone(),
            }),
            repo_root,
            policy: GitPolicy::default(),
        }
    }

    async fn run_git(
        &self,
        cwd: &Path,
        args: &[String],
    ) -> Result<pl_tool::execution::ExecutionOutput, WorktreeError> {
        let mut full_args = vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "-c".to_string(),
            format!("safe.directory={}", cwd.display()),
            "-c".to_string(),
            "credential.helper=".to_string(),
        ];
        full_args.extend_from_slice(args);
        self.transport
            .run(ExecutionRequest {
                program: PathBuf::from("git"),
                args: full_args,
                cwd: cwd.to_path_buf(),
                env: BTreeMap::new(),
                timeout: Some(WORKTREE_GIT_TIMEOUT),
            })
            .await
            .map_err(|stderr| WorktreeError::GitStatusUnknown {
                args: args.join(" "),
                stderr,
            })
    }

    fn relative_path(&self, path: &Path) -> Result<String, WorktreeError> {
        self.relative_path_text(&remote_path_text(path))
    }

    /// 把已归一化的 POSIX 路径表达成仓库根下的 workspace-relative 形式。
    ///
    /// 归一化在字符串层完成：宿主形态的 target 无法用 `Path::strip_prefix` 与仓库根匹配，
    /// 因此先把两侧都表达成 POSIX，再按目录边界剥离。这样目录/文件操作与 git 路径参数
    /// 都源自同一个 POSIX 结果。
    fn relative_path_text(&self, path: &str) -> Result<String, WorktreeError> {
        let root = remote_path_text(&self.repo_root);
        let root = root.trim_end_matches('/');
        let relative = if path == root {
            ""
        } else if root == "/" {
            path.strip_prefix('/').unwrap_or(path)
        } else {
            match path.strip_prefix(root) {
                Some(relative) if relative.starts_with('/') => &relative[1..],
                _ => {
                    return Err(WorktreeError::InvalidResource(format!(
                        "{} is outside {}",
                        path,
                        self.repo_root.display()
                    )));
                }
            }
        };
        Ok(relative.to_string())
    }
}

impl WorktreeBackend for RemoteWorktreeBackend {
    fn resolve_repo_root<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "--show-toplevel".to_string()];
            let output = checked_output(&args, self.run_git(path, &args).await?)?;
            non_empty_head(&args, &output.stdout).map(PathBuf::from)
        }
        .boxed()
    }

    fn create_parent<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            // 先归一化为 POSIX，再在 POSIX 空间取父目录：宿主 `Path::parent()` 会在归一化
            // 之前按本地分隔符切分混用形态的 target，从而算出错误的父目录。
            let target = remote_path_text(target_path);
            let parent = target
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .ok_or_else(|| {
                    WorktreeError::InvalidResource("worktree target has no parent".to_string())
                })?;
            self.transport
                .create_directory(self.relative_path_text(parent)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<bool, WorktreeError>> {
        async move {
            self.transport
                .path_exists(self.relative_path(path)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn remove_leaf<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            self.transport
                .remove_path(self.relative_path(target_path)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeCreateFailure>> {
        async move {
            self.policy.validate_branch(branch).map_err(|_| {
                WorktreeCreateFailure::no_side_effects(WorktreeError::UnsafeBranch(
                    branch.to_string(),
                ))
            })?;
            let args = vec![
                "worktree".to_string(),
                "add".to_string(),
                "-b".to_string(),
                branch.to_string(),
                // git 路径参数使用 POSIX 绝对路径：它与 lease 记录的 `path`、会话工作区根
                // 以及远端 workspace handle 根是同一字符串；目录/文件操作所用的
                // workspace-relative 形式是该绝对 POSIX 路径对仓库根的确定性投影。
                remote_path_text(target_path),
                base_commit.to_string(),
            ];
            let output = self
                .run_git(repo_root, &args)
                .await
                .map_err(WorktreeCreateFailure::may_have_created)?;
            checked_output(&args, output)
                .map(|_| ())
                .map_err(WorktreeCreateFailure::no_side_effects)
        }
        .boxed()
    }

    fn resolve_head<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<String, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "HEAD".to_string()];
            let output = checked_output(&args, self.run_git(worktree_path, &args).await?)?;
            non_empty_head(&args, &output.stdout)
        }
        .boxed()
    }

    fn status<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<WorktreeStatus, WorktreeError>> {
        async move {
            let head = self.resolve_head(worktree_path).await?;
            let args = vec![
                "status".to_string(),
                "--porcelain=v1".to_string(),
                "--untracked-files=all".to_string(),
            ];
            let output = checked_output(&args, self.run_git(worktree_path, &args).await?)?;
            Ok(WorktreeStatus {
                head,
                changed_files: changed_files(&output.stdout),
            })
        }
        .boxed()
    }

    fn remove<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let mut args = vec!["worktree".to_string(), "remove".to_string()];
            if force {
                args.push("--force".to_string());
            }
            args.push(remote_path_text(target_path));
            let output = self.run_git(repo_root, &args).await?;
            checked_output(&args, output).map(|_| ())
        }
        .boxed()
    }

    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
            let args = vec!["branch".to_string(), "-D".to_string(), branch.to_string()];
            let output = self.run_git(repo_root, &args).await?;
            checked_output(&args, output).map(|_| ())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::agent::worktree::{WorktreeManager, WorktreeOwnership};

    #[derive(Debug, Default)]
    struct RecordingTransport {
        requests: Mutex<Vec<ExecutionRequest>>,
        directories: Mutex<Vec<String>>,
        removals: Mutex<Vec<String>>,
    }

    impl RemoteWorktreeTransport for RecordingTransport {
        fn run<'a>(
            &'a self,
            request: ExecutionRequest,
        ) -> BoxFuture<'a, Result<ExecutionOutput, String>> {
            async move {
                let stdout = if request.args.ends_with(&["rev-parse".into(), "HEAD".into()]) {
                    "base-commit\n"
                } else {
                    ""
                };
                self.requests.lock().unwrap().push(request);
                Ok(ExecutionOutput {
                    status: 0,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                })
            }
            .boxed()
        }

        fn create_directory<'a>(
            &'a self,
            relative_path: String,
        ) -> BoxFuture<'a, Result<(), String>> {
            async move {
                self.directories.lock().unwrap().push(relative_path);
                Ok(())
            }
            .boxed()
        }

        fn path_exists<'a>(
            &'a self,
            _relative_path: String,
        ) -> BoxFuture<'a, Result<bool, String>> {
            async { Ok(true) }.boxed()
        }

        fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
            async move {
                self.removals.lock().unwrap().push(relative_path);
                Ok(())
            }
            .boxed()
        }
    }

    fn backend(transport: Arc<RecordingTransport>) -> RemoteWorktreeBackend {
        backend_at(transport, "/repo")
    }

    fn backend_at(transport: Arc<RecordingTransport>, repo_root: &str) -> RemoteWorktreeBackend {
        RemoteWorktreeBackend {
            transport,
            repo_root: PathBuf::from(repo_root),
            policy: GitPolicy::default(),
        }
    }

    #[tokio::test]
    async fn ssh_backend_uses_safe_timed_git_for_create_and_cleanup() {
        let transport = Arc::new(RecordingTransport::default());
        let backend = backend(transport.clone());
        let target = PathBuf::from("/repo/.anywork/worktrees/root/child");

        backend
            .create_parent(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend
            .create(
                Path::new("/repo"),
                "pure-agent-child",
                &target,
                "base-commit",
            )
            .await
            .unwrap();
        backend
            .remove(Path::new("/repo"), &target, true)
            .await
            .unwrap();
        backend
            .remove_leaf(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend
            .delete_branch(Path::new("/repo"), "pure-agent-child")
            .await
            .unwrap();

        assert_eq!(
            transport.directories.lock().unwrap().as_slice(),
            [".anywork/worktrees/root"]
        );
        assert_eq!(
            transport.removals.lock().unwrap().as_slice(),
            [".anywork/worktrees/root/child"]
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for request in requests.iter() {
            assert_eq!(request.program, PathBuf::from("git"));
            assert_eq!(request.timeout, Some(Duration::from_secs(120)));
            assert!(
                request
                    .args
                    .windows(2)
                    .any(|args| args == ["-c", "core.hooksPath=/dev/null"])
            );
            assert!(
                request
                    .args
                    .windows(2)
                    .any(|args| args == ["-c", "credential.helper="])
            );
        }
        assert!(requests[0].args.ends_with(&[
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "pure-agent-child".into(),
            "/repo/.anywork/worktrees/root/child".into(),
            "base-commit".into(),
        ]));
        assert!(requests[1].args.ends_with(&[
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            "/repo/.anywork/worktrees/root/child".into(),
        ]));
        assert!(requests[2].args.ends_with(&[
            "branch".into(),
            "-D".into(),
            "pure-agent-child".into(),
        ]));
    }

    /// Project 目录是仓库子目录时，backend 必须以创建时解析出的仓库根为基准：只有以仓库根
    /// 构造的 backend 才能把 Pure-owned worktree 路径表达成 workspace-relative 形式，
    /// 以 Project 目录为基准会判为 "outside"。创建路径与恢复、preview、清理共用该约定。
    #[tokio::test]
    async fn ssh_backend_uses_the_resolved_repository_root_as_its_relative_base() {
        let transport = Arc::new(RecordingTransport::default());
        let repository_root = Path::new("/repo");
        let project_root = Path::new("/repo/packages/app");
        let target = repository_root.join(".anywork/worktrees/root/session");
        let backend = backend_at(transport.clone(), "/repo");

        backend
            .create_parent(repository_root, &target)
            .await
            .unwrap();
        backend.remove_leaf(repository_root, &target).await.unwrap();

        assert_eq!(
            transport.directories.lock().unwrap().as_slice(),
            [".anywork/worktrees/root"]
        );
        assert_eq!(
            transport.removals.lock().unwrap().as_slice(),
            [".anywork/worktrees/root/session"]
        );

        // 反例：以 Project 目录为基准的 backend 无法表达同一路径，必须显式失败而不是
        // 把目录操作重定向到错误的相对路径。
        let project_root_backend = backend_at(transport, "/repo/packages/app");
        let error = project_root_backend
            .create_parent(project_root, &target)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("outside"), "{error}");
    }

    /// F1：Windows 形态的 target 必须归一化为 POSIX 后交给远端 git；目录/文件操作的相对
    /// 路径与 git 路径参数共用同一 POSIX 结果。修复前 git 参数原样携带 `\`，git 会在
    /// 错误位置创建物理 worktree，随后 `resolve_head` 的 cwd 归一化指向不存在的目录而失败。
    #[tokio::test]
    async fn ssh_backend_normalizes_host_shaped_targets_to_posix() {
        let transport = Arc::new(RecordingTransport::default());
        let backend = backend_at(transport.clone(), "/repo");
        // 与现场日志同形：仓库根为 POSIX，客户端 Windows `Path::join` 注入 `\`。
        let target = PathBuf::from("/repo\\.anywork/worktrees\\thread-1\\session");

        backend
            .create_parent(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend
            .create(Path::new("/repo"), "pure-session-thread-1", &target, "base")
            .await
            .unwrap();
        backend
            .remove(Path::new("/repo"), &target, true)
            .await
            .unwrap();
        backend
            .remove_leaf(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend.path_exists(&target).await.unwrap();

        assert_eq!(
            transport.directories.lock().unwrap().as_slice(),
            [".anywork/worktrees/thread-1"]
        );
        assert_eq!(
            transport.removals.lock().unwrap().as_slice(),
            [".anywork/worktrees/thread-1/session"]
        );
        let requests = transport.requests.lock().unwrap();
        for request in requests.iter() {
            for argument in &request.args {
                assert!(
                    !argument.contains('\\'),
                    "跨端 git 参数不得含宿主分隔符: {argument}"
                );
            }
        }
        assert!(requests[0].args.ends_with(&[
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "pure-session-thread-1".into(),
            "/repo/.anywork/worktrees/thread-1/session".into(),
            "base".into(),
        ]));
        assert!(requests[1].args.ends_with(&[
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            "/repo/.anywork/worktrees/thread-1/session".into(),
        ]));
    }

    /// F1：`WorktreeManager::allocate_path` 生成的 target 与 Windows 宿主 `Path::join`
    /// 形态的同一目标必须归一化为同一个 POSIX 路径，并通过同一归一化驱动 git 与目录操作。
    #[tokio::test]
    async fn allocated_target_normalizes_to_one_posix_path_for_git_and_directories() {
        let transport = Arc::new(RecordingTransport::default());
        let backend = backend_at(transport.clone(), "/repo");
        let allocated = WorktreeManager::allocate_path(
            Path::new("/repo"),
            "thread-1",
            &WorktreeOwnership::Session {
                thread_id: "thread-1".to_string(),
            },
        );
        let host_shaped = PathBuf::from("/repo\\.anywork/worktrees\\thread-1\\session");
        let expected = "/repo/.anywork/worktrees/thread-1/session";
        assert_eq!(remote_path_text(&allocated), expected);
        assert_eq!(remote_path_text(&host_shaped), expected);

        backend
            .create_parent(Path::new("/repo"), &host_shaped)
            .await
            .unwrap();
        backend
            .create(
                Path::new("/repo"),
                "pure-session-thread-1",
                &host_shaped,
                "base",
            )
            .await
            .unwrap();

        assert_eq!(
            transport.directories.lock().unwrap().as_slice(),
            [".anywork/worktrees/thread-1"]
        );
        let requests = transport.requests.lock().unwrap();
        assert!(requests[0].args.ends_with(&[
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "pure-session-thread-1".into(),
            expected.into(),
            "base".into(),
        ]));
    }
}
