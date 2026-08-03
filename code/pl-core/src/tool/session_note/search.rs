use base64::Engine;
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{SearcherBuilder, sinks::UTF8};
use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

use crate::working_set::canonical_content_hash;

use super::{TOOL_SEARCH_SESSION_NOTE, tool_error};

const MAX_QUERY_BYTES: usize = 4 * 1024;
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;
const REGEX_DFA_SIZE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchRequest {
    pub query: String,
    pub case_sensitive: bool,
    pub literal: bool,
    pub context_lines: usize,
    pub limit: usize,
    pub cursor: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchResult {
    pub matches: Vec<SessionNoteMatch>,
    pub count: usize,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionNoteMatch {
    line: usize,
    column: usize,
    text: String,
    before: Vec<ContextLine>,
    after: Vec<ContextLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ContextLine {
    line: usize,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMatch {
    line: usize,
    column: usize,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchCursor {
    revision: u64,
    key: String,
    offset: usize,
}

pub(super) fn search(content: &str, request: SearchRequest) -> Result<SearchResult, PureError> {
    validate_request(&request)?;
    let key = cursor_key(&request);
    let offset = decode_cursor(request.cursor.as_deref(), request.revision, &key)?;
    let requested_matches = offset.saturating_add(request.limit).saturating_add(1);
    let mut matcher_builder = RegexMatcherBuilder::new();
    matcher_builder
        .case_insensitive(!request.case_sensitive)
        .fixed_strings(request.literal)
        .crlf(true)
        .line_terminator(Some(b'\n'))
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT);
    let matcher = matcher_builder.build(&request.query).map_err(|error| {
        tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            format!("invalid search query: {error}"),
        )
    })?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .multi_line(false)
        .max_matches(Some(requested_matches as u64))
        .build();
    let mut raw_matches = Vec::new();
    searcher
        .search_slice(
            &matcher,
            content.as_bytes(),
            UTF8(|line_number, line| {
                let column = matcher
                    .find(line.as_bytes())
                    .ok()
                    .flatten()
                    .map_or(1, |matched| matched.start().saturating_add(1));
                raw_matches.push(RawMatch {
                    line: usize::try_from(line_number).unwrap_or(usize::MAX),
                    column,
                    text: trim_line_ending(line).to_string(),
                });
                Ok(true)
            }),
        )
        .map_err(|error| {
            tool_error(
                TOOL_SEARCH_SESSION_NOTE,
                format!("session note search failed: {error}"),
            )
        })?;

    let end = offset.saturating_add(request.limit).min(raw_matches.len());
    let has_more = end < raw_matches.len();
    let lines = content.lines().collect::<Vec<_>>();
    let matches = raw_matches
        .get(offset..end)
        .unwrap_or_default()
        .iter()
        .map(|item| with_context(item, &lines, request.context_lines))
        .collect::<Vec<_>>();
    let next_cursor = has_more.then(|| encode_cursor(request.revision, &key, end));
    Ok(SearchResult {
        count: matches.len(),
        matches,
        next_cursor,
    })
}

fn validate_request(request: &SearchRequest) -> Result<(), PureError> {
    if request.query.is_empty() {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            "query must not be empty",
        ));
    }
    if request.query.len() > MAX_QUERY_BYTES {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            format!("query exceeds {MAX_QUERY_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn cursor_key(request: &SearchRequest) -> String {
    canonical_content_hash(
        serde_json::json!({
            "query": request.query,
            "caseSensitive": request.case_sensitive,
            "literal": request.literal,
            "contextLines": request.context_lines,
        })
        .to_string()
        .as_bytes(),
    )
}

fn decode_cursor(cursor: Option<&str>, revision: u64, key: &str) -> Result<usize, PureError> {
    let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) else {
        return Ok(0);
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| {
            tool_error(
                TOOL_SEARCH_SESSION_NOTE,
                "invalid session note search cursor; omit cursor on the first page or pass the exact nextCursor returned by the previous search",
            )
        })?;
    let cursor: SearchCursor = serde_json::from_slice(&bytes).map_err(|_| {
        tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            "invalid session note search cursor; omit cursor on the first page or pass the exact nextCursor returned by the previous search",
        )
    })?;
    if cursor.revision != revision {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            format!(
                "session note search cursor is stale: cursor revision {}, current revision {revision}",
                cursor.revision
            ),
        ));
    }
    if cursor.key != key {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            "session note search cursor does not belong to this query",
        ));
    }
    Ok(cursor.offset)
}

fn encode_cursor(revision: u64, key: &str, offset: usize) -> String {
    let cursor = serde_json::to_vec(&SearchCursor {
        revision,
        key: key.to_string(),
        offset,
    })
    .expect("session note cursor serialization must succeed");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cursor)
}

fn with_context(item: &RawMatch, lines: &[&str], context_lines: usize) -> SessionNoteMatch {
    let line_index = item.line.saturating_sub(1);
    let before_start = line_index.saturating_sub(context_lines);
    let before = lines
        .get(before_start..line_index.min(lines.len()))
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(offset, text)| ContextLine {
            line: before_start + offset + 1,
            text: (*text).to_string(),
        })
        .collect();
    let after_start = line_index.saturating_add(1).min(lines.len());
    let after_end = after_start.saturating_add(context_lines).min(lines.len());
    let after = lines[after_start..after_end]
        .iter()
        .enumerate()
        .map(|(offset, text)| ContextLine {
            line: after_start + offset + 1,
            text: (*text).to_string(),
        })
        .collect();
    SessionNoteMatch {
        line: item.line,
        column: item.column,
        text: item.text.clone(),
        before,
        after,
    }
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line)
}
