use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::types::{LspDiagnostic, LspQueryOperation};
use crate::uri::uri_display_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormattedLspResult {
    pub text: String,
    pub result_count: Option<usize>,
    pub file_count: Option<usize>,
}

impl FormattedLspResult {
    fn new(
        text: impl Into<String>,
        result_count: Option<usize>,
        file_count: Option<usize>,
    ) -> Self {
        Self {
            text: text.into(),
            result_count,
            file_count,
        }
    }
}

pub(crate) fn format_lsp_result(
    operation: LspQueryOperation,
    value: &Value,
    workspace_root: &Path,
) -> FormattedLspResult {
    match operation {
        LspQueryOperation::GoToDefinition | LspQueryOperation::GoToImplementation => {
            format_locations(value, workspace_root, "definition")
        }
        LspQueryOperation::FindReferences => format_locations(value, workspace_root, "reference"),
        LspQueryOperation::Hover => format_hover(value),
        LspQueryOperation::DocumentSymbol => format_document_symbols(value),
        LspQueryOperation::WorkspaceSymbol => format_workspace_symbols(value, workspace_root),
        LspQueryOperation::PrepareCallHierarchy
        | LspQueryOperation::IncomingCalls
        | LspQueryOperation::OutgoingCalls => format_call_items(value, workspace_root),
        LspQueryOperation::Diagnostics => FormattedLspResult::new(value.to_string(), None, None),
    }
}

pub(crate) fn format_diagnostics(
    diagnostics: &[LspDiagnostic],
    max_results: usize,
) -> FormattedLspResult {
    if diagnostics.is_empty() {
        return FormattedLspResult::new("No diagnostics.", Some(0), Some(0));
    }
    let mut files = BTreeSet::new();
    let mut lines = Vec::new();
    for diagnostic in diagnostics.iter().take(max_results) {
        files.insert(diagnostic.path.clone());
        let code = diagnostic
            .code
            .as_ref()
            .map(|code| format!(" [{code}]"))
            .unwrap_or_default();
        let source = diagnostic
            .source
            .as_ref()
            .map(|source| format!(" ({source})"))
            .unwrap_or_default();
        lines.push(format!(
            "{}:{}:{} {}{}{}",
            diagnostic.path,
            diagnostic.range.start.line + 1,
            diagnostic.range.start.character + 1,
            diagnostic.message,
            code,
            source
        ));
    }
    let omitted = diagnostics.len().saturating_sub(lines.len());
    if omitted > 0 {
        lines.push(format!("... {omitted} more diagnostics omitted"));
    }
    FormattedLspResult::new(lines.join("\n"), Some(diagnostics.len()), Some(files.len()))
}

fn format_locations(value: &Value, workspace_root: &Path, noun: &str) -> FormattedLspResult {
    let locations = collect_locations(value);
    if locations.is_empty() {
        return FormattedLspResult::new(format!("No {noun}s found."), Some(0), Some(0));
    }
    let mut files = BTreeSet::new();
    let lines = locations
        .iter()
        .map(|location| {
            files.insert(location.path.clone());
            format!("{}:{}:{}", location.path, location.line, location.character)
        })
        .collect::<Vec<_>>();
    let prefix = if locations.len() == 1 {
        format!("Found 1 {noun}:")
    } else {
        format!(
            "Found {} {noun}s across {} files:",
            locations.len(),
            files.len()
        )
    };
    let text = format!("{prefix}\n{}", lines.join("\n"));
    let _ = workspace_root;
    FormattedLspResult::new(text, Some(locations.len()), Some(files.len()))
}

fn format_hover(value: &Value) -> FormattedLspResult {
    let Some(contents) = value.get("contents") else {
        return FormattedLspResult::new("No hover information found.", Some(0), None);
    };
    let text = markup_text(contents);
    if text.trim().is_empty() {
        FormattedLspResult::new("No hover information found.", Some(0), None)
    } else {
        FormattedLspResult::new(text, Some(1), None)
    }
}

fn format_document_symbols(value: &Value) -> FormattedLspResult {
    let Some(items) = value.as_array() else {
        return FormattedLspResult::new("No document symbols found.", Some(0), None);
    };
    if items.is_empty() {
        return FormattedLspResult::new("No document symbols found.", Some(0), None);
    }
    let mut lines = Vec::new();
    for item in items {
        push_document_symbol(item, 0, &mut lines);
    }
    FormattedLspResult::new(lines.join("\n"), Some(lines.len()), None)
}

fn format_workspace_symbols(value: &Value, workspace_root: &Path) -> FormattedLspResult {
    let Some(items) = value.as_array() else {
        return FormattedLspResult::new("No workspace symbols found.", Some(0), Some(0));
    };
    if items.is_empty() {
        return FormattedLspResult::new("No workspace symbols found.", Some(0), Some(0));
    }
    let mut files = BTreeSet::new();
    let mut lines = Vec::new();
    for item in items.iter().take(200) {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let kind = item.get("kind").and_then(Value::as_u64).unwrap_or_default();
        let uri = item
            .get("location")
            .and_then(|location| location.get("uri"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let path = uri_display_path(uri, Some(workspace_root));
        files.insert(path.clone());
        lines.push(format!("{name} kind={kind} {path}"));
    }
    let omitted = items.len().saturating_sub(lines.len());
    if omitted > 0 {
        lines.push(format!("... {omitted} more symbols omitted"));
    }
    FormattedLspResult::new(lines.join("\n"), Some(items.len()), Some(files.len()))
}

fn format_call_items(value: &Value, workspace_root: &Path) -> FormattedLspResult {
    let Some(items) = value.as_array() else {
        return FormattedLspResult::new("No call hierarchy results found.", Some(0), Some(0));
    };
    if items.is_empty() {
        return FormattedLspResult::new("No call hierarchy results found.", Some(0), Some(0));
    }
    let mut files = BTreeSet::new();
    let lines = items
        .iter()
        .map(|item| {
            let target = item.get("from").or_else(|| item.get("to")).unwrap_or(item);
            let name = target
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let uri = target.get("uri").and_then(Value::as_str).unwrap_or("");
            let path = uri_display_path(uri, Some(workspace_root));
            files.insert(path.clone());
            format!("{name} {path}")
        })
        .collect::<Vec<_>>();
    FormattedLspResult::new(lines.join("\n"), Some(items.len()), Some(files.len()))
}

fn push_document_symbol(item: &Value, depth: usize, lines: &mut Vec<String>) {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let kind = item.get("kind").and_then(Value::as_u64).unwrap_or_default();
    let indent = "  ".repeat(depth);
    lines.push(format!("{indent}{name} kind={kind}"));
    if let Some(children) = item.get("children").and_then(Value::as_array) {
        for child in children {
            push_document_symbol(child, depth + 1, lines);
        }
    }
}

#[derive(Debug, Clone)]
struct LocationText {
    path: String,
    line: u64,
    character: u64,
}

fn collect_locations(value: &Value) -> Vec<LocationText> {
    match value {
        Value::Array(items) => items.iter().filter_map(location_text).collect(),
        Value::Null => Vec::new(),
        item => location_text(item).into_iter().collect(),
    }
}

fn location_text(value: &Value) -> Option<LocationText> {
    let uri = value
        .get("uri")
        .or_else(|| value.get("targetUri"))
        .and_then(Value::as_str)?;
    let range = value
        .get("range")
        .or_else(|| value.get("targetSelectionRange"))
        .or_else(|| value.get("targetRange"))?;
    let start = range.get("start")?;
    Some(LocationText {
        path: uri_display_path(uri, None),
        line: start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1,
        character: start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1,
    })
}

fn markup_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(markup_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| object.get("language").and_then(Value::as_str))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn formats_location_arrays() {
        let value = serde_json::json!([
            {
                "uri": "file:///C:/repo/src/lib.rs",
                "range": {"start": {"line": 4, "character": 2}, "end": {"line": 4, "character": 5}}
            }
        ]);

        let result = format_lsp_result(
            LspQueryOperation::GoToDefinition,
            &value,
            Path::new("C:/repo"),
        );

        assert_eq!(result.result_count, Some(1));
        assert!(result.text.contains("src/lib.rs:5:3"));
    }

    #[test]
    fn formats_hover_markup() {
        let value = serde_json::json!({"contents": {"kind": "markdown", "value": "fn main()"}});

        let result = format_lsp_result(LspQueryOperation::Hover, &value, Path::new("."));

        assert_eq!(result.text, "fn main()");
    }
}
