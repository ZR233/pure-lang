//! client `tool_search`:延迟 catalog 检索、调用解析与 `tool_search_output` 投影。

use pl_model::ToolSchema;
use pl_protocol::{PureError, ResponsesContextItem, ResponsesContextItemKind, Result};

use super::inventory::ToolInventory;

const DEFAULT_CLIENT_TOOL_SEARCH_LIMIT: usize = 8;
pub(super) const MAX_CLIENT_TOOL_SEARCH_LIMIT: usize = 32;
const MAX_CLIENT_TOOL_SEARCH_QUERY_BYTES: usize = 4_096;

/// client tool search 的解析结果。
#[derive(Debug, Clone, Default)]
pub(crate) struct ClientToolSearchResolution {
    pub(crate) outputs: Vec<ResponsesContextItem>,
    pub(crate) loaded_tool_count: u64,
    /// 每个调用的展示摘要（query 与按 namespace 分组的工具名），供 timeline 投影。
    pub(crate) summaries: Vec<ClientToolSearchCallSummary>,
}

/// 单次 client `tool_search` 调用的展示摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientToolSearchCallSummary {
    pub(crate) call_id: String,
    pub(crate) query: String,
    /// (namespace, 工具名列表)；保持检索排名顺序。
    pub(crate) groups: Vec<(String, Vec<String>)>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientToolSearchArguments {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

impl ToolInventory {
    /// 解析 provider 响应中的 client `tool_search_call` 项。
    ///
    /// 在冻结 catalog 上检索并把每个调用产出为带工具定义分组的
    /// `tool_search_output` Responses 上下文项。catalog 缺失、call_id 缺失或
    /// 参数非法都返回 typed 协议错误。
    ///
    /// # Errors
    ///
    /// 没有 catalog、`call_id` 为空、`arguments` 非法、query 为空或超长、
    /// `limit` 为 0 时返回协议错误。
    pub(crate) fn resolve_client_search_calls(
        &self,
        items: &[ResponsesContextItem],
    ) -> Result<ClientToolSearchResolution> {
        let mut resolution = ClientToolSearchResolution::default();
        for item in items {
            if item.kind != ResponsesContextItemKind::ToolSearchCall {
                continue;
            }
            let execution = item
                .value
                .get("execution")
                .and_then(serde_json::Value::as_str);
            match execution {
                Some("client") => {}
                Some(other) => {
                    return Err(client_tool_search_protocol_error(format!(
                        "unsupported execution `{other}`"
                    )));
                }
                None => {
                    return Err(client_tool_search_protocol_error("missing execution"));
                }
            }
            let catalog = self.catalog.as_ref().ok_or_else(|| {
                client_tool_search_protocol_error(
                    "provider returned a client call without a frozen search catalog",
                )
            })?;
            let call_id = item
                .value
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .filter(|call_id| !call_id.trim().is_empty())
                .ok_or_else(|| client_tool_search_protocol_error("missing call_id"))?;
            let arguments = item
                .value
                .get("arguments")
                .cloned()
                .ok_or_else(|| client_tool_search_protocol_error("missing arguments"))?;
            let arguments = serde_json::from_value::<ClientToolSearchArguments>(arguments)
                .map_err(|error| {
                    client_tool_search_protocol_error(format!("invalid arguments: {error}"))
                })?;
            let query = arguments.query.trim();
            if query.is_empty() {
                return Err(client_tool_search_protocol_error("query must not be empty"));
            }
            if query.len() > MAX_CLIENT_TOOL_SEARCH_QUERY_BYTES {
                return Err(client_tool_search_protocol_error(format!(
                    "query exceeds {MAX_CLIENT_TOOL_SEARCH_QUERY_BYTES} bytes"
                )));
            }
            let requested_limit = arguments.limit.unwrap_or(DEFAULT_CLIENT_TOOL_SEARCH_LIMIT);
            if requested_limit == 0 {
                return Err(client_tool_search_protocol_error(
                    "limit must be greater than zero",
                ));
            }
            let groups = catalog.search(query, requested_limit.min(MAX_CLIENT_TOOL_SEARCH_LIMIT));
            resolution.loaded_tool_count = resolution
                .loaded_tool_count
                .saturating_add(count_loaded_tools(&groups));
            resolution.summaries.push(ClientToolSearchCallSummary {
                call_id: call_id.to_string(),
                query: query.to_string(),
                groups: groups
                    .iter()
                    .map(|group| {
                        (
                            group.namespace_name.clone(),
                            group
                                .tools
                                .iter()
                                .map(|tool| tool.name().to_string())
                                .collect(),
                        )
                    })
                    .collect(),
            });
            resolution
                .outputs
                .push(client_tool_search_output(call_id, &groups)?);
        }
        Ok(resolution)
    }
}

/// schema 固定的 client `tool_search` function 工具；不随 catalog 内容变化。
pub(super) fn client_tool_search_schema() -> ToolSchema {
    ToolSchema::function(
        "tool_search",
        "Search the deferred tool catalog for this turn. Matching tool schemas are loaded for the next model call.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for deferred tools."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_CLIENT_TOOL_SEARCH_LIMIT,
                    "description": format!(
                        "Maximum number of tools to return. Defaults to {DEFAULT_CLIENT_TOOL_SEARCH_LIMIT}."
                    )
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ClientToolSearchCatalog {
    pub(super) entries: Vec<ClientToolSearchEntry>,
}

impl ClientToolSearchCatalog {
    pub(super) fn fingerprint(&self) -> String {
        let value = serde_json::json!(
            self.entries
                .iter()
                .map(|entry| serde_json::json!({
                    "namespace": entry.namespace_name,
                    "namespaceDescription": entry.namespace_description,
                    "tool": entry.schema,
                }))
                .collect::<Vec<_>>()
        );
        crate::canonical_json_hash(&value)
    }

    /// 确定性词项评分检索；返回按 namespace 分组的工具定义（保持排名顺序）。
    ///
    /// 返回工具数封顶 [`MAX_CLIENT_TOOL_SEARCH_LIMIT`]。
    pub(super) fn search(&self, query: &str, limit: usize) -> Vec<ToolSearchGroup> {
        let limit = limit.min(MAX_CLIENT_TOOL_SEARCH_LIMIT);
        let query = query.to_lowercase();
        let terms = search_terms(&query);
        let mut ranked = self
            .entries
            .iter()
            .filter_map(|entry| {
                let score = score_search_entry(entry, &query, &terms);
                (score > 0).then_some((score, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| left.schema.name().cmp(right.schema.name()))
                .then_with(|| left.namespace_name.cmp(&right.namespace_name))
        });

        let mut groups: Vec<ToolSearchGroup> = Vec::new();
        for (_, entry) in ranked.into_iter().take(limit) {
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.namespace_name == entry.namespace_name)
            {
                group.tools.push(entry.schema.clone());
                continue;
            }
            groups.push(ToolSearchGroup {
                namespace_name: entry.namespace_name.clone(),
                namespace_description: entry.namespace_description.clone(),
                tools: vec![entry.schema.clone()],
            });
        }
        groups
    }
}

/// 检索结果按 namespace 分组的工具定义。
#[derive(Debug, Clone)]
pub(super) struct ToolSearchGroup {
    pub(super) namespace_name: String,
    pub(super) namespace_description: String,
    pub(super) tools: Vec<ToolSchema>,
}

#[derive(Debug, Clone)]
pub(super) struct ClientToolSearchEntry {
    namespace_name: String,
    namespace_description: String,
    schema: ToolSchema,
    search_text: String,
}

impl ClientToolSearchEntry {
    pub(super) fn new(
        namespace_name: String,
        namespace_description: String,
        schema: ToolSchema,
    ) -> Self {
        let schema_text = serde_json::to_string(&schema).unwrap_or_default();
        let search_text = format!(
            "{namespace_name} {namespace_description} {} {} {} {schema_text}",
            schema.name(),
            schema.name().replace(['_', '-'], " "),
            schema.description(),
        )
        .to_lowercase();
        Self {
            namespace_name,
            namespace_description,
            schema,
            search_text,
        }
    }
}

fn search_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
    {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_string());
        }
    }
    terms
}

fn score_search_entry(entry: &ClientToolSearchEntry, query: &str, terms: &[String]) -> u64 {
    let name = entry.schema.name().to_lowercase();
    let description = entry.schema.description().to_lowercase();
    let mut score = 0_u64;
    if name == query {
        score = score.saturating_add(1_000);
    } else if name.replace(['_', '-'], " ") == query {
        score = score.saturating_add(800);
    } else if name.contains(query) {
        score = score.saturating_add(200);
    }
    if description.contains(query) || entry.search_text.contains(query) {
        score = score.saturating_add(100);
    }
    for term in terms {
        if name == *term {
            score = score.saturating_add(100);
        } else if name.contains(term) {
            score = score.saturating_add(40);
        }
        if description.contains(term) {
            score = score.saturating_add(20);
        } else if entry.search_text.contains(term) {
            score = score.saturating_add(5);
        }
    }
    score
}

/// 生成与 `tool_search_call` 配对的 `tool_search_output` 上下文项。
fn client_tool_search_output(
    call_id: &str,
    groups: &[ToolSearchGroup],
) -> Result<ResponsesContextItem> {
    if call_id.trim().is_empty() {
        return Err(client_tool_search_protocol_error(
            "client tool_search output requires a non-empty call_id",
        ));
    }
    if groups.iter().any(|group| {
        group
            .tools
            .iter()
            .any(|tool| !is_loadable_tool_schema(tool))
    }) {
        return Err(client_tool_search_protocol_error(
            "client tool_search output contains a non-loadable tool schema",
        ));
    }
    let value = serde_json::json!({
        "type": "tool_search_output",
        "call_id": call_id,
        "status": "completed",
        "execution": "client",
        "tools": groups.iter().map(|group| serde_json::json!({
            "type": "namespace",
            "name": group.namespace_name,
            "description": group.namespace_description,
            "tools": group.tools.iter().map(catalog_tool_wire_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    Ok(ResponsesContextItem {
        kind: ResponsesContextItemKind::ToolSearchOutput,
        value,
    })
}

fn is_loadable_tool_schema(tool: &ToolSchema) -> bool {
    matches!(
        tool,
        ToolSchema::Function { .. } | ToolSchema::Custom { .. }
    )
}

/// 把 catalog 工具 schema 投影为 `tool_search_output` wire 中的定义形状。
///
/// catalog 条目按定义延迟加载，wire 固定携带 `defer_loading: true`。
fn catalog_tool_wire_json(tool: &ToolSchema) -> serde_json::Value {
    match tool {
        ToolSchema::Function {
            name,
            description,
            input_schema,
            allowed_callers,
            output_schema,
        } => {
            let mut value = serde_json::json!({
                "type": "function",
                "name": name,
                "description": description,
                "parameters": input_schema,
                "defer_loading": true,
            });
            if !allowed_callers.is_empty() {
                value["allowed_callers"] =
                    serde_json::to_value(allowed_callers).unwrap_or(serde_json::Value::Null);
            }
            if let Some(output_schema) = output_schema {
                value["output_schema"] = output_schema.clone();
            }
            value
        }
        // catalog 只收录 Function 工具；其余变体不会出现在输出中。
        ToolSchema::Custom { .. }
        | ToolSchema::ProgrammaticToolCalling
        | ToolSchema::WebSearch { .. } => serde_json::json!({}),
    }
}

fn count_loaded_tools(groups: &[ToolSearchGroup]) -> u64 {
    groups.iter().map(|group| group.tools.len() as u64).sum()
}

fn client_tool_search_protocol_error(message: impl Into<String>) -> PureError {
    PureError::LlmError(format!(
        "provider response protocol error: client tool_search {}",
        message.into()
    ))
}
