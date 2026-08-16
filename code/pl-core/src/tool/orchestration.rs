use pl_model::ToolSchema;
use pl_protocol::{PureError, ResponsesContextItem, ResponsesContextItemKind, Result};

use crate::turn::ToolEffect;

use super::source::ToolEntry;

const DEFAULT_CLIENT_TOOL_SEARCH_LIMIT: usize = 8;
const MAX_CLIENT_TOOL_SEARCH_LIMIT: usize = 32;
const MAX_CLIENT_TOOL_SEARCH_QUERY_BYTES: usize = 4_096;

/// 当前模型请求可启用的工具编排能力。
///
/// Tool Search 只有客户端执行一种路径：请求携带 eager schema 与一个 schema
/// 固定的 `tool_search` function 工具，检索在本 Turn 冻结的 catalog 上执行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolOrchestrationOptions {
    pub tool_search: bool,
    pub programmatic_tool_calling: bool,
}

/// 一次 Turn 冻结的完整工具目录。
///
/// `request_schemas` 是实际发送给 provider 的 canonical 化并排序后的 schema
/// 集合；`catalog` 是延迟加载工具的检索目录，变化不影响请求前缀。
#[derive(Debug, Clone, Default)]
pub struct ToolInventory {
    request_schemas: Vec<ToolSchema>,
    catalog: Option<ClientToolSearchCatalog>,
}

impl ToolInventory {
    pub fn request_schemas(&self) -> &[ToolSchema] {
        &self.request_schemas
    }

    pub(crate) fn catalog(&self) -> Option<&ClientToolSearchCatalog> {
        self.catalog.as_ref()
    }

    /// 延迟加载 catalog 的 canonical 哈希；无 catalog 时为 `None`。
    pub fn catalog_fingerprint(&self) -> Option<String> {
        self.catalog
            .as_ref()
            .map(ClientToolSearchCatalog::fingerprint)
    }

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

/// 把 lease 条目编排为一次 Turn 的模型可见工具集合。
///
/// eager/policy 过滤后，携带 namespace 元数据的 Function 工具进入延迟 catalog；
/// 其余工具 eager 上线。programmatic 资格完全由来源元数据驱动
/// （`programmatic_eligible` 且 effect 为 `Read`）。排序 canonical 化只在此处
/// 发生一次。
pub fn orchestrate_tool_inventory(
    entries: &[ToolEntry],
    policy: Option<&crate::AgentExecutionPolicy>,
    options: ToolOrchestrationOptions,
) -> ToolInventory {
    let mut eager = Vec::new();
    let mut has_programmatic = false;
    let mut deferred = Vec::new();

    for entry in entries {
        let tool = entry.tool();
        let effect = tool.effect();
        if policy.is_some_and(|policy| !policy.allows_tool(entry.name(), effect)) {
            continue;
        }
        let mut schema = tool.to_schema();
        if options.programmatic_tool_calling
            && entry.metadata().programmatic_eligible
            && effect == Some(ToolEffect::Read)
            && matches!(schema, ToolSchema::Function { .. })
        {
            let output_schema = match &schema {
                ToolSchema::Function {
                    output_schema: Some(output_schema),
                    ..
                } => output_schema.clone(),
                _ => generic_object_output_schema(),
            };
            schema = schema.allow_programmatic(output_schema);
            has_programmatic = true;
        }
        match entry.metadata().namespace.as_ref() {
            Some(namespace)
                if options.tool_search && matches!(schema, ToolSchema::Function { .. }) =>
            {
                deferred.push(ClientToolSearchEntry::new(
                    namespace.name.clone(),
                    namespace.description.clone(),
                    schema,
                ));
            }
            _ => eager.push(schema),
        }
    }

    let mut catalog = None;
    if !deferred.is_empty() {
        eager.push(client_tool_search_schema());
        catalog = Some(ClientToolSearchCatalog { entries: deferred });
    }
    if options.programmatic_tool_calling && has_programmatic {
        eager.push(ToolSchema::ProgrammaticToolCalling);
    }
    ToolInventory {
        request_schemas: crate::stable_tool_schemas(eager),
        catalog,
    }
}

pub(crate) fn estimate_tool_schema_tokens(schemas: &[ToolSchema]) -> u64 {
    let bytes = serde_json::to_vec(schemas).map_or(0, |value| value.len() as u64);
    bytes.saturating_add(3) / 4
}

pub(crate) fn estimate_tool_result_tokens<'a>(results: impl IntoIterator<Item = &'a str>) -> u64 {
    let bytes = results.into_iter().fold(0_u64, |total, result| {
        total.saturating_add(result.len() as u64)
    });
    bytes.saturating_add(3) / 4
}

/// schema 固定的 client `tool_search` function 工具；不随 catalog 内容变化。
fn client_tool_search_schema() -> ToolSchema {
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

fn generic_object_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": true,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct ClientToolSearchCatalog {
    entries: Vec<ClientToolSearchEntry>,
}

impl ClientToolSearchCatalog {
    fn fingerprint(&self) -> String {
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
    fn search(&self, query: &str, limit: usize) -> Vec<ToolSearchGroup> {
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
struct ToolSearchGroup {
    namespace_name: String,
    namespace_description: String,
    tools: Vec<ToolSchema>,
}

#[derive(Debug, Clone)]
struct ClientToolSearchEntry {
    namespace_name: String,
    namespace_description: String,
    schema: ToolSchema,
    search_text: String,
}

impl ClientToolSearchEntry {
    fn new(namespace_name: String, namespace_description: String, schema: ToolSchema) -> Self {
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{
        NamespaceDescriptor, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput,
        ToolSourceId, ToolSourceMetadata,
    };
    use crate::turn::ToolEffect;
    use futures::FutureExt;

    #[derive(Debug)]
    struct FakeTool {
        name: String,
        description: &'static str,
        effect: Option<ToolEffect>,
        programmatic: bool,
        namespace: Option<NamespaceDescriptor>,
    }

    impl Tool for FakeTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }

        fn effect(&self) -> Option<ToolEffect> {
            self.effect
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
            _context: ToolContext,
        ) -> futures::future::BoxFuture<'a, std::result::Result<ToolOutput, PureError>> {
            async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: OutputTruncation::empty(),
                    output_file: std::path::PathBuf::new(),
                    exit_code: Some(0),
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            }
            .boxed()
        }
    }

    fn entry(tool: FakeTool, source: &ToolSourceId) -> ToolEntry {
        let metadata = ToolSourceMetadata {
            source: source.clone(),
            namespace: tool.namespace.clone(),
            programmatic_eligible: tool.programmatic,
        };
        ToolEntry::new(tool, metadata)
    }

    fn git_entry(name: &str, effect: ToolEffect, programmatic: bool) -> ToolEntry {
        entry(
            FakeTool {
                name: name.to_string(),
                description: "git tool",
                effect: Some(effect),
                programmatic,
                namespace: Some(NamespaceDescriptor::new(
                    "git",
                    "Git inspection and repository management tools.",
                )),
            },
            &ToolSourceId::builtin(),
        )
    }

    fn eager_entry(name: &str, effect: Option<ToolEffect>) -> ToolEntry {
        entry(
            FakeTool {
                name: name.to_string(),
                description: "eager tool",
                effect,
                programmatic: false,
                namespace: None,
            },
            &ToolSourceId::builtin(),
        )
    }

    fn search_options() -> ToolOrchestrationOptions {
        ToolOrchestrationOptions {
            tool_search: true,
            programmatic_tool_calling: false,
        }
    }

    #[test]
    fn namespaced_tools_go_into_catalog_and_eager_stay_in_request() {
        let entries = vec![
            git_entry("git_status", ToolEffect::Read, false),
            eager_entry("exec", Some(ToolEffect::Process)),
        ];

        let inventory = orchestrate_tool_inventory(&entries, None, search_options());

        let names = inventory
            .request_schemas()
            .iter()
            .map(ToolSchema::name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["exec", "tool_search"]);
        assert!(inventory.catalog().is_some());
    }

    #[test]
    fn tool_search_schema_is_constant_regardless_of_catalog() {
        let small = orchestrate_tool_inventory(
            &[git_entry("git_status", ToolEffect::Read, false)],
            None,
            search_options(),
        );
        let large = orchestrate_tool_inventory(
            &[
                git_entry("git_status", ToolEffect::Read, false),
                git_entry("git_diff", ToolEffect::Read, false),
                git_entry("git_branch", ToolEffect::BranchControl, false),
            ],
            None,
            search_options(),
        );

        let search_small = small
            .request_schemas()
            .iter()
            .find(|schema| schema.name() == "tool_search")
            .expect("tool_search schema");
        let search_large = large
            .request_schemas()
            .iter()
            .find(|schema| schema.name() == "tool_search")
            .expect("tool_search schema");
        assert_eq!(
            serde_json::to_string(search_small).unwrap(),
            serde_json::to_string(search_large).unwrap()
        );
        assert_ne!(small.catalog_fingerprint(), large.catalog_fingerprint());
    }

    #[test]
    fn programmatic_eligibility_is_metadata_driven() {
        let declared_read = orchestrate_tool_inventory(
            &[git_entry("git_status", ToolEffect::Read, true)],
            None,
            ToolOrchestrationOptions {
                tool_search: false,
                programmatic_tool_calling: true,
            },
        );
        assert!(
            declared_read
                .request_schemas()
                .iter()
                .any(|schema| schema.name() == "programmatic_tool_calling")
        );

        let without_declaration = orchestrate_tool_inventory(
            &[git_entry("git_status", ToolEffect::Read, false)],
            None,
            ToolOrchestrationOptions {
                tool_search: false,
                programmatic_tool_calling: true,
            },
        );
        assert!(
            !without_declaration
                .request_schemas()
                .iter()
                .any(|schema| schema.name() == "programmatic_tool_calling")
        );

        let non_read = orchestrate_tool_inventory(
            &[git_entry("git_branch", ToolEffect::BranchControl, true)],
            None,
            ToolOrchestrationOptions {
                tool_search: false,
                programmatic_tool_calling: true,
            },
        );
        assert!(
            !non_read
                .request_schemas()
                .iter()
                .any(|schema| schema.name() == "programmatic_tool_calling")
        );
    }

    #[test]
    fn policy_filters_entries_before_orchestration() {
        let entries = vec![
            git_entry("git_status", ToolEffect::Read, false),
            eager_entry("exec", Some(ToolEffect::Process)),
        ];
        let policy = crate::AgentExecutionPolicy {
            visible_tools: crate::ToolVisibilitySet::from_tool_names(["git_status"]),
            allowed_effects: crate::ToolEffectSet::from_effects([ToolEffect::Read]),
            ..crate::AgentExecutionPolicy::default()
        };

        let inventory = orchestrate_tool_inventory(&entries, Some(&policy), search_options());

        let names = inventory
            .request_schemas()
            .iter()
            .map(ToolSchema::name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["tool_search"]);
        assert!(inventory.catalog().is_some());
    }

    #[test]
    fn search_returns_namespace_groups_ranked_by_score() {
        let entries = vec![
            git_entry("git_status", ToolEffect::Read, false),
            git_entry("git_diff", ToolEffect::Read, false),
        ];
        let inventory = orchestrate_tool_inventory(&entries, None, search_options());
        let catalog = inventory.catalog().expect("catalog");

        let results = catalog.search("status", 8);

        let group = &results[0];
        assert_eq!(group.namespace_name, "git");
        assert_eq!(group.tools.len(), 1);
        assert_eq!(group.tools[0].name(), "git_status");
    }

    #[test]
    fn search_respects_limit_cap() {
        let entries: Vec<ToolEntry> = (0..40)
            .map(|index| git_entry(&format!("git_tool_{index}"), ToolEffect::Read, false))
            .collect();
        let inventory = orchestrate_tool_inventory(&entries, None, search_options());
        let catalog = inventory.catalog().expect("catalog");

        let results = catalog.search("git_tool", 100);

        let total: usize = results.iter().map(|group| group.tools.len()).sum();
        assert_eq!(total, MAX_CLIENT_TOOL_SEARCH_LIMIT);
    }

    #[test]
    fn resolve_client_search_calls_produces_grouped_outputs() {
        let entries = vec![git_entry("git_status", ToolEffect::Read, false)];
        let inventory = orchestrate_tool_inventory(&entries, None, search_options());
        let call = ResponsesContextItem {
            kind: ResponsesContextItemKind::ToolSearchCall,
            value: serde_json::json!({
                "type": "tool_search_call",
                "call_id": "call-1",
                "execution": "client",
                "arguments": { "query": "status" },
            }),
        };

        let resolution = inventory.resolve_client_search_calls(&[call]).unwrap();

        assert_eq!(resolution.loaded_tool_count, 1);
        let output = &resolution.outputs[0];
        assert_eq!(output.kind, ResponsesContextItemKind::ToolSearchOutput);
        assert_eq!(output.value["call_id"], "call-1");
        assert_eq!(output.value["status"], "completed");
        assert_eq!(output.value["execution"], "client");
        assert_eq!(output.value["tools"][0]["type"], "namespace");
        assert_eq!(output.value["tools"][0]["name"], "git");
        assert_eq!(output.value["tools"][0]["tools"][0]["name"], "git_status");
        assert_eq!(output.value["tools"][0]["tools"][0]["defer_loading"], true);
    }

    #[test]
    fn resolve_client_search_calls_rejects_invalid_items() {
        let entries = vec![git_entry("git_status", ToolEffect::Read, false)];
        let inventory = orchestrate_tool_inventory(&entries, None, search_options());

        let missing_call_id = ResponsesContextItem {
            kind: ResponsesContextItemKind::ToolSearchCall,
            value: serde_json::json!({
                "type": "tool_search_call",
                "execution": "client",
                "arguments": { "query": "status" },
            }),
        };
        assert!(
            inventory
                .resolve_client_search_calls(&[missing_call_id])
                .is_err()
        );

        let no_catalog =
            orchestrate_tool_inventory(&entries, None, ToolOrchestrationOptions::default());
        let call = ResponsesContextItem {
            kind: ResponsesContextItemKind::ToolSearchCall,
            value: serde_json::json!({
                "type": "tool_search_call",
                "call_id": "call-1",
                "execution": "client",
                "arguments": { "query": "status" },
            }),
        };
        let error = no_catalog.resolve_client_search_calls(&[call]).unwrap_err();
        assert!(error.to_string().contains("frozen search catalog"));
    }
}
