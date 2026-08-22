//! 工具编排与 client tool search 的目录冻结、检索与解析测试。

use futures::FutureExt;
use pl_model::ToolSchema;
use pl_protocol::{PureError, ResponsesContextItem, ResponsesContextItemKind};
use pretty_assertions::assert_eq;

use super::search::MAX_CLIENT_TOOL_SEARCH_LIMIT;
use super::*;
use crate::tool::source::ToolEntry;
use crate::tool::{
    NamespaceDescriptor, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, ToolSourceId,
    ToolSourceMetadata,
};
use crate::turn::ToolEffect;

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
