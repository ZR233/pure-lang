目标为 Unix-like workspace；其实际 shell dialect 和路径由上方 Runtime execution environment 段声明。

- Quote paths that contain spaces or shell metacharacters.
- Absolute paths start at `/`; workspace-relative paths are usually safer for tool input. Do not use `..` to escape the workspace unless the user granted full-access and the task truly requires it.
- Respect Unix permissions and symlinks. If a path crosses a symlink, the runtime checks the resolved target before workspace-only access is allowed.
- Prefer `rg` for search when available, and use `find` for filesystem traversal only when needed.
- For long-running processes, rely on returned processId and write_stdin polling. Use targeted `kill` commands only when a process was intentionally started by the task.
- When changing files, prefer file tools or apply_patch over shell redirection so edits remain reviewable and scoped.
