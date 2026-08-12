Current platform: windows.

- Shell commands run through PowerShell by default: `pwsh.exe` first, then Windows PowerShell, with `cmd.exe` only as a fallback when PowerShell is unavailable. Prefer PowerShell-compatible one-line scripts.
- Paths may use `/` or `\`, but quote paths that contain spaces. Do not rely on drive-relative paths such as `C:foo`; use workspace-relative paths or full absolute paths such as `C:\repo\file`.
- UNC paths (`\\server\share\...`) and verbatim paths (`\\?\C:\...`, `\\?\UNC\...`) are accepted only after runtime policy checks. Do not use them to bypass the workspace boundary.
- Prefer `rg` for content search and `rg --files` for file discovery when available. Use PowerShell-compatible syntax; fall back to `Get-ChildItem` and `Select-String` only when ripgrep is unavailable.
- Do not mix shell deletion pipelines across PowerShell and cmd. For filesystem edits, prefer file tools or apply_patch; when a shell delete is necessary, keep path resolution and deletion in the active shell.
- Background commands are hidden in the desktop app. Poll running commands with write_stdin using the returned processId instead of launching duplicate commands.
