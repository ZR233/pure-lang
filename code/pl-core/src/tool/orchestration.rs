use std::collections::BTreeMap;

use pl_model::ToolSchema;

use crate::turn::ToolEffect;

const MAX_DEFERRED_TOOLS_PER_NAMESPACE: usize = 8;

/// 当前模型请求可启用的 Responses 工具编排能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolOrchestrationOptions {
    pub tool_search: bool,
    pub programmatic_tool_calling: bool,
}

pub(super) fn orchestrate_tool_schemas(
    tools: impl IntoIterator<Item = (ToolSchema, Option<ToolEffect>)>,
    options: ToolOrchestrationOptions,
) -> Vec<ToolSchema> {
    let mut eager = Vec::new();
    let mut deferred = BTreeMap::<String, DeferredNamespace>::new();
    let mut has_programmatic = false;

    for (mut schema, effect) in tools {
        if options.programmatic_tool_calling && programmatic_eligible(&schema, effect) {
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

        let Some(namespace) = options
            .tool_search
            .then(|| deferred_namespace(schema.name()))
            .flatten()
        else {
            eager.push(schema);
            continue;
        };
        if !matches!(schema, ToolSchema::Function { .. }) {
            eager.push(schema);
            continue;
        }
        deferred
            .entry(namespace.name)
            .or_insert_with(|| DeferredNamespace {
                description: namespace.description,
                tools: Vec::new(),
            })
            .tools
            .push(schema.deferred());
    }

    if !deferred.is_empty() {
        for (name, mut namespace) in deferred {
            namespace
                .tools
                .sort_by(|left, right| left.name().cmp(right.name()));
            let chunk_count = namespace
                .tools
                .len()
                .div_ceil(MAX_DEFERRED_TOOLS_PER_NAMESPACE);
            let mut tools = namespace.tools.into_iter();
            for chunk_index in 0..chunk_count {
                let chunk = tools
                    .by_ref()
                    .take(MAX_DEFERRED_TOOLS_PER_NAMESPACE)
                    .collect::<Vec<_>>();
                let chunk_name = if chunk_count == 1 {
                    name.clone()
                } else {
                    format!("{name}_{}", chunk_index + 1)
                };
                let description = if chunk_count == 1 {
                    namespace.description.clone()
                } else {
                    format!(
                        "{} Part {} of {chunk_count}.",
                        namespace.description,
                        chunk_index + 1
                    )
                };
                eager.push(ToolSchema::namespace(chunk_name, description, chunk));
            }
        }
        eager.push(ToolSchema::ToolSearch);
    }
    if options.programmatic_tool_calling && has_programmatic {
        eager.push(ToolSchema::ProgrammaticToolCalling);
    }
    eager
}

pub fn estimate_tool_schema_tokens(schemas: &[ToolSchema]) -> u64 {
    let bytes = serde_json::to_vec(schemas).map_or(0, |value| value.len() as u64);
    bytes.saturating_add(3) / 4
}

pub fn estimate_tool_result_tokens<'a>(results: impl IntoIterator<Item = &'a str>) -> u64 {
    let bytes = results.into_iter().fold(0_u64, |total, result| {
        total.saturating_add(result.len() as u64)
    });
    bytes.saturating_add(3) / 4
}

fn programmatic_eligible(schema: &ToolSchema, effect: Option<ToolEffect>) -> bool {
    if effect != Some(ToolEffect::Read) || !matches!(schema, ToolSchema::Function { .. }) {
        return false;
    }
    let name = schema.name();
    matches!(
        name,
        "read_file"
            | "list_files"
            | "stat_path"
            | "git_status"
            | "git_diff"
            | "git_workspace_info"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "read_mcp_resource"
    ) || name.starts_with("lsp_query_")
        || name.starts_with("mcp__")
}

fn generic_object_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": true,
    })
}

fn deferred_namespace(name: &str) -> Option<DeferredNamespaceTarget> {
    if let Some(mcp_name) = name.strip_prefix("mcp__") {
        let server = mcp_name
            .split_once("__")
            .map(|(server, _)| server)
            .filter(|server| !server.is_empty());
        let namespace = server.map_or_else(
            || "mcp".to_string(),
            |server| format!("mcp_{}", namespace_component(server)),
        );
        return Some(DeferredNamespaceTarget {
            name: namespace,
            description: server.map_or_else(
                || "Dynamically registered MCP tools.".to_string(),
                |server| format!("Dynamically registered tools from MCP server `{server}`."),
            ),
        });
    }
    if name.starts_with("git_") {
        return Some(DeferredNamespaceTarget::new(
            "git",
            "Git inspection and repository management tools.",
        ));
    }
    if name.starts_with("task_") {
        return Some(DeferredNamespaceTarget::new(
            "task",
            "Task coordination, review, delivery, and completion tools.",
        ));
    }
    if matches!(
        name,
        "spawn_agent"
            | "send_message"
            | "interrupt_agent"
            | "list_agents"
            | "wait_agents"
            | "read_agent_session"
            | "read_agent_submissions"
            | "close_agent"
            | "report_progress"
    ) {
        return Some(DeferredNamespaceTarget::new(
            "agents",
            "Subagent discovery, messaging, waiting, and lifecycle tools.",
        ));
    }
    None
}

fn namespace_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

struct DeferredNamespaceTarget {
    name: String,
    description: String,
}

impl DeferredNamespaceTarget {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

struct DeferredNamespace {
    description: String,
    tools: Vec<ToolSchema>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str) -> ToolSchema {
        ToolSchema::function(name, name, serde_json::json!({"type": "object"}))
    }

    #[test]
    fn unsupported_models_keep_eager_direct_tools() {
        let schemas = orchestrate_tool_schemas(
            [(function("git_status"), Some(ToolEffect::Read))],
            ToolOrchestrationOptions::default(),
        );

        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name(), "git_status");
    }

    #[test]
    fn search_groups_low_frequency_tools_and_programmatic_only_marks_reads() {
        let schemas = orchestrate_tool_schemas(
            [
                (function("git_status"), Some(ToolEffect::Read)),
                (function("git_push"), Some(ToolEffect::BranchControl)),
                (function("exec"), Some(ToolEffect::Process)),
            ],
            ToolOrchestrationOptions {
                tool_search: true,
                programmatic_tool_calling: true,
            },
        );

        let namespace = schemas
            .iter()
            .find(|schema| schema.name() == "git")
            .expect("git namespace");
        let ToolSchema::Namespace { tools, .. } = namespace else {
            panic!("git namespace schema");
        };
        assert_eq!(tools.len(), 2);
        assert!(schemas.iter().any(ToolSchema::is_tool_search));
        assert!(schemas.iter().any(ToolSchema::is_programmatic_tool_calling));
        let git_status = tools
            .iter()
            .find(|tool| tool.name() == "git_status")
            .expect("git_status schema");
        let git_push = tools
            .iter()
            .find(|tool| tool.name() == "git_push")
            .expect("git_push schema");
        assert!(matches!(
            git_status,
            ToolSchema::Function { allowed_callers, .. }
                if allowed_callers.contains(&pl_model::ToolCallerMode::Programmatic)
        ));
        assert!(matches!(
            git_push,
            ToolSchema::Function { allowed_callers, .. } if allowed_callers.is_empty()
        ));
    }

    #[test]
    fn search_groups_mcp_tools_by_server() {
        let schemas = orchestrate_tool_schemas(
            [
                (function("mcp__github__get_pr"), Some(ToolEffect::Read)),
                (function("mcp__slack__search"), Some(ToolEffect::Read)),
            ],
            ToolOrchestrationOptions {
                tool_search: true,
                programmatic_tool_calling: false,
            },
        );

        assert!(schemas.iter().any(|schema| schema.name() == "mcp_github"));
        assert!(schemas.iter().any(|schema| schema.name() == "mcp_slack"));
    }

    #[test]
    fn search_splits_large_namespaces_into_bounded_chunks() {
        let schemas = orchestrate_tool_schemas(
            (0..10).rev().map(|index| {
                (
                    function(&format!("git_tool_{index}")),
                    Some(ToolEffect::Read),
                )
            }),
            ToolOrchestrationOptions {
                tool_search: true,
                programmatic_tool_calling: false,
            },
        );

        let namespaces = schemas
            .iter()
            .filter_map(|schema| match schema {
                ToolSchema::Namespace { name, tools, .. } if name.starts_with("git_") => Some(
                    tools
                        .iter()
                        .map(|tool| tool.name().to_string())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            namespaces,
            vec![
                (0..8)
                    .map(|index| format!("git_tool_{index}"))
                    .collect::<Vec<_>>(),
                (8..10)
                    .map(|index| format!("git_tool_{index}"))
                    .collect::<Vec<_>>(),
            ]
        );
    }

    #[test]
    fn mcp_namespace_preserves_distinct_exposed_server_components() {
        let schemas = orchestrate_tool_schemas(
            [
                (function("mcp__foo-bar__read"), Some(ToolEffect::Read)),
                (function("mcp__foo_bar__read"), Some(ToolEffect::Read)),
                (function("mcp__Foo__read"), Some(ToolEffect::Read)),
                (function("mcp__foo__read"), Some(ToolEffect::Read)),
            ],
            ToolOrchestrationOptions {
                tool_search: true,
                programmatic_tool_calling: false,
            },
        );
        let names = schemas
            .iter()
            .filter(|schema| matches!(schema, ToolSchema::Namespace { .. }))
            .map(ToolSchema::name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"mcp_foo-bar"));
        assert!(names.contains(&"mcp_foo_bar"));
        assert!(names.contains(&"mcp_Foo"));
        assert!(names.contains(&"mcp_foo"));
    }
}
