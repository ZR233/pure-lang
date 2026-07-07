use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use pl_protocol::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use super::backend::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecRequest,
};
use super::helpers::{parse_input, preview_error, shell_quote, tool_error};
use super::schema::TOOL_APPLY_PATCH;

#[derive(Debug, Deserialize)]
struct ApplyPatchInput {
    input: String,
    cwd: Option<String>,
}

pub(super) async fn apply_patch<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: ApplyPatchInput = parse_input(arguments, TOOL_APPLY_PATCH)?;
    let cwd = resolve_patch_cwd(backend, input.cwd).await?;
    let parsed = Patch::parse(&input.input)?;
    let mut working = Vec::new();
    for operation in &parsed.operations {
        let source = resolve_relative_container_path(&cwd, operation.path())?;
        let destination = operation
            .move_to()
            .map(|path| resolve_relative_container_path(&cwd, path))
            .transpose()?;
        let existing = match operation {
            PatchOperation::Add { .. } => None,
            PatchOperation::Update { .. } | PatchOperation::Delete { .. } => {
                Some(read_container_text(backend, &source).await?)
            }
        };
        let change = operation.compute(existing.as_deref())?;
        working.push(PreparedPatchOperation {
            source,
            destination,
            change,
        });
    }

    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut deleted = Vec::new();
    let mut moved = Vec::new();
    for operation in working {
        match operation.change {
            PatchChange::Add { content } => {
                write_container_text(backend, &operation.source, &content).await?;
                added.push(operation.source);
            }
            PatchChange::Update { content } => {
                if let Some(destination) = operation.destination {
                    write_container_text(backend, &destination, &content).await?;
                    if destination != operation.source {
                        remove_container_path(backend, &operation.source).await?;
                        moved.push(json!({
                            "from": operation.source,
                            "to": destination,
                        }));
                    }
                } else {
                    write_container_text(backend, &operation.source, &content).await?;
                }
                updated.push(operation.source);
            }
            PatchChange::Delete => {
                remove_container_path(backend, &operation.source).await?;
                deleted.push(operation.source);
            }
        }
    }
    let mut changed_files = BTreeSet::new();
    changed_files.extend(added.iter().cloned());
    changed_files.extend(updated.iter().cloned());
    changed_files.extend(deleted.iter().cloned());
    for item in &moved {
        if let Some(to) = item.get("to").and_then(Value::as_str) {
            changed_files.insert(to.to_string());
        }
    }
    Ok(json!({
        "cwd": cwd,
        "added": added,
        "updated": updated,
        "deleted": deleted,
        "moved": moved,
        "changed_files": changed_files.into_iter().collect::<Vec<_>>(),
        "stdout": "apply_patch completed",
        "stderr": "",
    }))
}

async fn resolve_patch_cwd<B>(backend: &B, cwd: Option<String>) -> Result<String>
where
    B: ContainerBackend,
{
    if let Some(cwd) = cwd {
        return Ok(cwd);
    }
    let output = backend
        .exec(ContainerExecRequest {
            command:
                "if [ -d /workspace/repo ]; then printf /workspace/repo; else printf /workspace; fi"
                    .to_string(),
            cwd: Some("/".to_string()),
            timeout_secs: Some(10),
            output_bytes_cap: None,
            cancellation_token: None,
        })
        .await?;
    if output.status != 0 {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            format!(
                "apply_patch failed to resolve cwd: {}",
                preview_error(&output.stderr, &output.stdout)
            ),
        ));
    }
    Ok(output.stdout.trim().to_string())
}

async fn read_container_text<B>(backend: &B, container_path: &str) -> Result<String>
where
    B: ContainerBackend,
{
    let command = format!("test -f {}", shell_quote(container_path));
    let output = backend
        .exec(ContainerExecRequest {
            command,
            cwd: Some("/".to_string()),
            timeout_secs: Some(10),
            output_bytes_cap: None,
            cancellation_token: None,
        })
        .await?;
    if output.status != 0 {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            format!("file not found: {container_path}"),
        ));
    }
    let bytes = backend
        .copy_from(ContainerCopyFromRequest {
            path: container_path.to_string(),
            archive: false,
        })
        .await
        .map_err(|error| {
            tool_error(
                TOOL_APPLY_PATCH,
                format!("failed to read `{container_path}` for apply_patch: {error}"),
            )
        })?;
    String::from_utf8(bytes).map_err(|error| {
        tool_error(
            TOOL_APPLY_PATCH,
            format!("failed to decode `{container_path}` as UTF-8: {error}"),
        )
    })
}

async fn write_container_text<B>(backend: &B, container_path: &str, content: &str) -> Result<()>
where
    B: ContainerBackend,
{
    backend
        .copy_to(ContainerCopyToRequest {
            path: container_path.to_string(),
            content: content.as_bytes().to_vec(),
        })
        .await
        .map_err(|error| {
            tool_error(
                TOOL_APPLY_PATCH,
                format!("failed to write `{container_path}` for apply_patch: {error}"),
            )
        })
}

async fn remove_container_path<B>(backend: &B, container_path: &str) -> Result<()>
where
    B: ContainerBackend,
{
    let output = backend
        .exec(ContainerExecRequest {
            command: format!("rm -f -- {}", shell_quote(container_path)),
            cwd: Some("/".to_string()),
            timeout_secs: Some(20),
            output_bytes_cap: None,
            cancellation_token: None,
        })
        .await?;
    if output.status != 0 {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            format!(
                "failed to remove `{container_path}` for apply_patch: {}",
                preview_error(&output.stderr, &output.stdout)
            ),
        ));
    }
    Ok(())
}

struct PreparedPatchOperation {
    source: String,
    destination: Option<String>,
    change: PatchChange,
}

enum PatchChange {
    Add { content: String },
    Update { content: String },
    Delete,
}

struct Patch {
    operations: Vec<PatchOperation>,
}

enum PatchOperation {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<PatchHunk>,
    },
    Delete {
        path: String,
    },
}

impl PatchOperation {
    fn path(&self) -> &str {
        match self {
            PatchOperation::Add { path, .. }
            | PatchOperation::Update { path, .. }
            | PatchOperation::Delete { path } => path,
        }
    }

    fn move_to(&self) -> Option<&str> {
        match self {
            PatchOperation::Update { move_to, .. } => move_to.as_deref(),
            PatchOperation::Add { .. } | PatchOperation::Delete { .. } => None,
        }
    }

    fn compute(&self, existing: Option<&str>) -> Result<PatchChange> {
        match self {
            PatchOperation::Add { lines, .. } => Ok(PatchChange::Add {
                content: lines.join(""),
            }),
            PatchOperation::Delete { .. } => {
                existing
                    .ok_or_else(|| tool_error(TOOL_APPLY_PATCH, "delete target does not exist"))?;
                Ok(PatchChange::Delete)
            }
            PatchOperation::Update { hunks, .. } => {
                let existing = existing
                    .ok_or_else(|| tool_error(TOOL_APPLY_PATCH, "update target does not exist"))?;
                let mut content_lines = split_preserving_newlines(existing);
                let mut search_start = 0;
                for hunk in hunks {
                    let old_lines = hunk
                        .lines
                        .iter()
                        .filter_map(|line| match line {
                            PatchLine::Context(text) | PatchLine::Remove(text) => {
                                Some(text.clone())
                            }
                            PatchLine::Add(_) => None,
                        })
                        .collect::<Vec<_>>();
                    let new_lines = hunk
                        .lines
                        .iter()
                        .filter_map(|line| match line {
                            PatchLine::Context(text) | PatchLine::Add(text) => Some(text.clone()),
                            PatchLine::Remove(_) => None,
                        })
                        .collect::<Vec<_>>();
                    if old_lines.is_empty() {
                        return Err(tool_error(
                            TOOL_APPLY_PATCH,
                            "update hunk must include context or removed lines",
                        ));
                    }
                    let index = find_subsequence(&content_lines, &old_lines, search_start)
                        .ok_or_else(|| {
                            tool_error(
                                TOOL_APPLY_PATCH,
                                "apply_patch hunk context did not match target file",
                            )
                        })?;
                    content_lines.splice(index..index + old_lines.len(), new_lines.clone());
                    search_start = index.saturating_add(new_lines.len());
                }
                Ok(PatchChange::Update {
                    content: content_lines.join(""),
                })
            }
        }
    }
}

struct PatchHunk {
    lines: Vec<PatchLine>,
}

enum PatchLine {
    Context(String),
    Add(String),
    Remove(String),
}

impl Patch {
    fn parse(input: &str) -> Result<Self> {
        let normalized = input.replace("\r\n", "\n");
        let mut lines = normalized.split_inclusive('\n').collect::<Vec<_>>();
        if normalized.ends_with('\n') {
        } else if let Some(last) = normalized.rsplit('\n').next()
            && !last.is_empty()
            && lines.last().is_none_or(|line| *line != last)
        {
            lines.push(last);
        }
        let mut cursor = 0;
        expect_marker(&lines, &mut cursor, "*** Begin Patch")?;
        let mut operations = Vec::new();
        while cursor < lines.len() {
            let marker = trim_line_ending(lines[cursor]);
            if marker == "*** End Patch" {
                cursor += 1;
                break;
            }
            if let Some(path) = marker.strip_prefix("*** Add File: ") {
                cursor += 1;
                let mut content = Vec::new();
                while cursor < lines.len() {
                    let line = lines[cursor];
                    let trimmed = trim_line_ending(line);
                    if trimmed.starts_with("*** ") {
                        break;
                    }
                    let Some(rest) = line.strip_prefix('+') else {
                        return Err(tool_error(
                            TOOL_APPLY_PATCH,
                            "add file lines must start with `+`",
                        ));
                    };
                    content.push(rest.to_string());
                    cursor += 1;
                }
                operations.push(PatchOperation::Add {
                    path: path.to_string(),
                    lines: content,
                });
                continue;
            }
            if let Some(path) = marker.strip_prefix("*** Delete File: ") {
                cursor += 1;
                operations.push(PatchOperation::Delete {
                    path: path.to_string(),
                });
                continue;
            }
            if let Some(path) = marker.strip_prefix("*** Update File: ") {
                cursor += 1;
                let mut move_to = None;
                if cursor < lines.len()
                    && let Some(dest) =
                        trim_line_ending(lines[cursor]).strip_prefix("*** Move to: ")
                {
                    move_to = Some(dest.to_string());
                    cursor += 1;
                }
                let mut hunks = Vec::new();
                while cursor < lines.len() {
                    let marker = trim_line_ending(lines[cursor]);
                    if marker.starts_with("*** ") {
                        break;
                    }
                    if !marker.starts_with("@@") {
                        return Err(tool_error(
                            TOOL_APPLY_PATCH,
                            "update hunks must start with `@@`",
                        ));
                    }
                    cursor += 1;
                    let mut hunk_lines = Vec::new();
                    while cursor < lines.len() {
                        let line = lines[cursor];
                        let trimmed = trim_line_ending(line);
                        if trimmed.starts_with("@@") || trimmed.starts_with("*** ") {
                            break;
                        }
                        if trimmed == "*** End of File" {
                            cursor += 1;
                            break;
                        }
                        let Some(prefix) = line.chars().next() else {
                            return Err(tool_error(TOOL_APPLY_PATCH, "empty hunk line is invalid"));
                        };
                        let text = line[prefix.len_utf8()..].to_string();
                        match prefix {
                            ' ' => hunk_lines.push(PatchLine::Context(text)),
                            '+' => hunk_lines.push(PatchLine::Add(text)),
                            '-' => hunk_lines.push(PatchLine::Remove(text)),
                            _ => {
                                return Err(tool_error(
                                    TOOL_APPLY_PATCH,
                                    "hunk lines must start with space, `+`, or `-`",
                                ));
                            }
                        }
                        cursor += 1;
                    }
                    hunks.push(PatchHunk { lines: hunk_lines });
                }
                if hunks.is_empty() {
                    return Err(tool_error(
                        TOOL_APPLY_PATCH,
                        "update file requires at least one hunk",
                    ));
                }
                operations.push(PatchOperation::Update {
                    path: path.to_string(),
                    move_to,
                    hunks,
                });
                continue;
            }
            return Err(tool_error(
                TOOL_APPLY_PATCH,
                format!("invalid patch marker `{marker}`"),
            ));
        }
        if cursor == 0 || operations.is_empty() {
            return Err(tool_error(
                TOOL_APPLY_PATCH,
                "patch must include at least one operation",
            ));
        }
        if cursor < lines.len() && lines[cursor..].iter().any(|line| !line.trim().is_empty()) {
            return Err(tool_error(
                TOOL_APPLY_PATCH,
                "unexpected content after patch end",
            ));
        }
        Ok(Self { operations })
    }
}

fn expect_marker(lines: &[&str], cursor: &mut usize, expected: &str) -> Result<()> {
    if lines
        .get(*cursor)
        .map(|line| trim_line_ending(line) == expected)
        .unwrap_or(false)
    {
        *cursor += 1;
        return Ok(());
    }
    Err(tool_error(
        TOOL_APPLY_PATCH,
        format!("patch must start with `{expected}`"),
    ))
}

fn split_preserving_newlines(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut lines = value
        .split_inclusive('\n')
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !value.ends_with('\n')
        && let Some(last) = value.rsplit('\n').next()
    {
        if let Some(existing_last) = lines.last_mut() {
            if existing_last.ends_with('\n') {
                lines.push(last.to_string());
            }
        } else {
            lines.push(last.to_string());
        }
    }
    lines
}

fn find_subsequence(haystack: &[String], needle: &[String], start: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (start..=haystack.len().saturating_sub(needle.len()))
        .find(|&index| haystack[index..index + needle.len()] == *needle)
}

fn resolve_relative_container_path(cwd: &str, raw_path: &str) -> Result<String> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            "patch file path cannot be empty",
        ));
    }
    let path = Path::new(raw_path);
    if path.is_absolute() {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            "patch file paths must be relative",
        ));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(tool_error(
                    TOOL_APPLY_PATCH,
                    "patch file paths cannot contain `..`",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(tool_error(
                    TOOL_APPLY_PATCH,
                    "patch file paths must be relative",
                ));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(tool_error(
            TOOL_APPLY_PATCH,
            "patch file path cannot be empty",
        ));
    }
    let mut full = PathBuf::from(cwd);
    full.push(clean);
    Ok(full.to_string_lossy().replace('\\', "/"))
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line)
}

pub(super) const APPLY_PATCH_DESCRIPTION: &str = r#"Use the `apply_patch` tool to edit files.

The patch format is a stripped-down, file-oriented diff envelope:

*** Begin Patch
*** Add File: hello.txt
+Hello world
*** Update File: src/app.py
@@
-print("Hi")
+print("Hello, world!")
*** Delete File: obsolete.txt
*** End Patch

Each operation starts with one of:
- `*** Add File: <path>`
- `*** Delete File: <path>`
- `*** Update File: <path>`

Update sections contain hunks introduced by `@@`; hunk lines start with a space, `-`, or `+`. File references must be relative, never absolute."#;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn patch_add_update_delete_and_move_parse() {
        let patch = Patch::parse(
            "*** Begin Patch\n*** Add File: a.txt\n+one\n*** Update File: b.txt\n*** Move to: c.txt\n@@\n-old\n+new\n*** Delete File: d.txt\n*** End Patch\n",
        )
        .expect("parse patch");
        assert_eq!(patch.operations.len(), 3);
    }

    #[test]
    fn patch_rejects_absolute_or_parent_paths() {
        assert!(resolve_relative_container_path("/workspace/repo", "/tmp/a").is_err());
        assert!(resolve_relative_container_path("/workspace/repo", "../a").is_err());
    }

    #[test]
    fn update_hunk_applies_context() {
        let operation = PatchOperation::Update {
            path: "a.txt".to_string(),
            move_to: None,
            hunks: vec![PatchHunk {
                lines: vec![
                    PatchLine::Context("one\n".to_string()),
                    PatchLine::Remove("two\n".to_string()),
                    PatchLine::Add("dos\n".to_string()),
                    PatchLine::Context("three\n".to_string()),
                ],
            }],
        };
        let PatchChange::Update { content } = operation.compute(Some("one\ntwo\nthree\n")).unwrap()
        else {
            panic!("expected update");
        };
        assert_eq!(content, "one\ndos\nthree\n");
    }
}
