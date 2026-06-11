Platform tool rules:

- Prefer workspace-relative paths in tool input. The runtime resolves relative paths against the workspace root, not the process current directory.
- Do not guess the current working directory. Use the provided workspace root semantics, or pass an explicit workingDirectory when a shell command needs one.
- File, patch, shell workingDirectory, and LSP filePath inputs may be relative or absolute; the runtime normalizes them before approval and execution.
- Use LSP tools for supported semantic code queries, and use text search or shell search tools when LSP is unavailable or the file type is unsupported.
- For long command output, inspect the outputFile with file tools instead of asking the shell tool to return complete stdout or stderr.
- Keep destructive filesystem operations scoped and explicit. Prefer apply_patch for focused source edits.
