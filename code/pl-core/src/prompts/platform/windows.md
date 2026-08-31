目标为 Windows workspace；其实际 shell dialect 和路径由上方 Runtime execution environment 段声明。不要假设 PowerShell 一定可用。

- 使用运行时段声明的 shell 语法，优先使用与该 shell 兼容的短命令。
- Paths may use `/` or `\`, but quote paths that contain spaces. Do not rely on drive-relative paths such as `C:foo`; use workspace-relative paths or full absolute paths such as `C:\repo\file`.
- UNC paths (`\\server\share\...`) and verbatim paths (`\\?\C:\...`, `\\?\UNC\...`) are accepted only after runtime policy checks. Do not use them to bypass the workspace boundary.
- Prefer `rg` for content search and `rg --files` for file discovery when available. Keep fallback search commands compatible with the runtime shell.
- Do not mix shell deletion pipelines across PowerShell and cmd. For filesystem edits, prefer file tools or apply_patch; when a shell delete is necessary, keep path resolution and deletion in the active shell.
- Background commands are hidden in the desktop app. Poll running commands with write_stdin using the returned processId instead of launching duplicate commands.
