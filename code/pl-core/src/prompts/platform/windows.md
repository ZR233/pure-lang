Current platform: windows.

- Shell commands run through Windows command execution. Prefer PowerShell-compatible commands when composing one-line scripts.
- Paths may use `/` or `\`, but quote paths that contain spaces. Do not rely on drive-relative paths such as `C:foo`; use workspace-relative paths or full absolute paths such as `C:\repo\file`.
- UNC paths (`\\server\share\...`) and verbatim paths (`\\?\C:\...`, `\\?\UNC\...`) are accepted only after runtime policy checks. Do not use them to bypass the workspace boundary.
- Prefer `rg` for search when available. Use `Get-ChildItem` for PowerShell enumeration when a shell command is genuinely needed.
- Do not mix shell deletion pipelines across PowerShell and cmd. For filesystem edits, prefer file tools or apply_patch; when a shell delete is necessary, keep path resolution and deletion in one shell.
- Background commands are hidden in the desktop app. Poll running commands with write_stdin using the returned processId instead of launching duplicate commands.
