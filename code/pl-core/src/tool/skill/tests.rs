use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::*;
use crate::session::AgentSession;
use crate::turn::TurnOptions;

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-skill-tool-{name}-{stamp}"))
}

fn skill_content(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n")
}

fn tool_context(workspace_root: PathBuf) -> ToolContext {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    ToolContext {
        event_tx,
        options: TurnOptions::default(),
        workspace_access: super::super::WorkspaceAccess::WorkspaceOnly,
        workspace: crate::tool::AgentWorkspace::local(workspace_root),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        lsp_runtime: None,
        parent_session: Arc::new(AgentSession::new()),
        working_set: crate::TurnWorkingSetHandle::default(),
        tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
    }
}

fn write_project_skill(workspace: &Path, name: &str) {
    let skill_dir = workspace.join("skills").join(name);
    fs::create_dir_all(skill_dir.join("references")).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        skill_content(name, "Project skill"),
    )
    .unwrap();
    fs::write(skill_dir.join("references/example.md"), "support").unwrap();
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[cfg(unix)]
fn remove_directory_link(link: &Path) {
    fs::remove_file(link).unwrap();
}

#[cfg(windows)]
fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).unwrap();
}

fn activation_from_output(output: &ToolOutput) -> &SkillActivation {
    let Some(ToolRuntimeEvent::SkillActivated { activation }) = output.runtime_events.first()
    else {
        panic!("expected skill activation")
    };
    activation
}

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SkillViewOutputSnapshot {
    success: bool,
    skill: SkillMetadata,
    file_path: String,
    support_files: Vec<crate::skill::SkillFile>,
    content: String,
}

#[test]
fn create_writes_project_skill() {
    let workspace = temp_dir("create");
    let catalog = SkillCatalog {
        project_dir: workspace.join("skills"),
        skills: Vec::new(),
        warnings: Vec::new(),
    };
    let input = CreateSkillInput {
        target: SkillTargetInput {
            name: "local-flow".to_string(),
        },
        content: skill_content("local-flow", "Local flow"),
        category: None,
    };

    create_skill("skill_manage", &catalog, input).unwrap();

    assert!(workspace.join("skills/local-flow/SKILL.md").exists());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn skill_inputs_and_outputs_flatten_shared_fields() {
    let input = serde_json::from_value::<SkillManageInput>(serde_json::json!({
        "action": "create",
        "name": "local-flow",
        "content": "body",
        "category": null,
    }))
    .unwrap();
    let SkillManageInput::Create(input) = input else {
        panic!("expected create action");
    };
    assert_eq!(input.target.name, "local-flow");

    let output = serde_json::to_value(SkillPathOutput {
        action: SkillActionOutput {
            success: true,
            action: "create",
            name: "local-flow",
        },
        path: Path::new("skills/local-flow"),
    })
    .unwrap();
    assert_eq!(
        output,
        serde_json::json!({
            "success": true,
            "action": "create",
            "name": "local-flow",
            "path": "skills/local-flow",
        })
    );
}

#[test]
fn skill_manage_schema_is_a_provider_object_union() {
    let tool = SkillManageTool::new(SkillsConfig::default());
    let schema = tool.input_schema();

    assert_eq!(schema["type"], "object");
    assert!(schema["oneOf"].is_array());
    assert!(schema.get("additionalProperties").is_none());
    let input = deserialize_tool_input::<SkillManageInput>(
        tool.name(),
        json!({
            "action": "create",
            "name": "local-flow",
            "content": "body",
        }),
    )
    .expect("valid skill_manage action");
    assert!(matches!(input, SkillManageInput::Create(_)));

    let error = deserialize_tool_input::<SkillManageInput>(
        tool.name(),
        json!({
            "action": "create",
            "name": "local-flow",
            "content": "body",
            "unexpected": true,
        }),
    )
    .expect_err("unknown skill_manage field");
    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn patch_accepts_json_escaped_markdown_old_string() {
    let workspace = temp_dir("patch-escaped-old-string");
    let skill_dir = workspace.join("skills/local-flow");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: local-flow\ndescription: Local flow\n---\n# local-flow\n\nSnippet: `\"unknown\\nusage\"`\n",
    )
    .unwrap();
    let catalog = SkillCatalog {
        project_dir: workspace.join("skills"),
        skills: vec![SkillMetadata {
            name: "local-flow".to_string(),
            description: "Local flow".to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            path: skill_dir.clone(),
        }],
        warnings: Vec::new(),
    };
    let input = PatchSkillInput {
        target: SkillTargetInput {
            name: "local-flow".to_string(),
        },
        old_string: r#"Snippet: `\"unknown\\nusage\"`"#.to_string(),
        new_string: "Snippet: `\"known\\nusage\"`".to_string(),
        replace_mode: None,
    };

    patch_skill("skill_manage", &catalog, input).unwrap();

    let updated = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(updated.contains("Snippet: `\"known\\nusage\"`"));
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn rejects_readonly_skill_patch() {
    let catalog = SkillCatalog {
        project_dir: PathBuf::from("project/skills"),
        skills: vec![SkillMetadata {
            name: "shared".to_string(),
            description: "shared".to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::System,
            path: PathBuf::from("user/shared"),
        }],
        warnings: Vec::new(),
    };

    let error = writable_project_skill("skill_manage", &catalog, "shared")
        .unwrap_err()
        .to_string();

    assert!(error.contains("read-only system"));
}

#[tokio::test]
async fn skill_view_success_emits_skill_activation() {
    let workspace = temp_dir("view-activation");
    write_project_skill(&workspace, "local-flow");
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let output = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "local-flow"}),
                session_id: "turn-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap();
    let activation = activation_from_output(&output);

    assert_eq!(activation.name, "local-flow");
    assert_eq!(activation.source, "project");
    assert_eq!(activation.turn_id, "turn-1");
    assert_eq!(activation.tool_call_id, "call-1");
    assert!(activation.path.ends_with("local-flow"));
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn skill_view_records_project_usage_independent_of_product_mode() {
    let workspace = temp_dir("view-task-readonly");
    write_project_skill(&workspace, "local-flow");
    let usage_path = workspace.join("skills/local-flow/.usage.json");
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let output = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "local-flow"}),
                session_id: "turn-task".to_string(),
                tool_id: "call-task".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap();

    assert_eq!(activation_from_output(&output).name, "local-flow");
    assert!(usage_path.exists());
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn skill_view_support_file_success_activates_parent_skill() {
    let workspace = temp_dir("view-support-activation");
    write_project_skill(&workspace, "local-flow");
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let output = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "local-flow", "filePath": "references/example.md"}),
                session_id: "turn-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap();
    let activation = activation_from_output(&output);

    assert_eq!(activation.name, "local-flow");
    assert_eq!(activation.source, "project");
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn skill_view_main_file_alias_reads_skill_and_lists_support_files() {
    let workspace = temp_dir("view-main-alias");
    write_project_skill(&workspace, "local-flow");
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let output = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "local-flow", "filePath": "SKILL.md"}),
                session_id: "turn-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap();
    let result = serde_json::from_str::<SkillViewOutputSnapshot>(&output.description).unwrap();

    assert_eq!(activation_from_output(&output).name, "local-flow");
    assert!(result.success);
    assert_eq!(result.skill.name, "local-flow");
    assert_eq!(result.file_path, "SKILL.md");
    assert_eq!(result.support_files[0].path, "references/example.md");
    assert!(result.content.contains("# local-flow"));
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn skill_view_failure_does_not_emit_activation() {
    let workspace = temp_dir("view-failure");
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let error = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "missing"}),
                session_id: "turn-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("skill not found"));
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn skill_discovery_skips_linked_skill_directories() {
    let workspace = temp_dir("linked-discovery");
    let outside = temp_dir("linked-discovery-target");
    fs::create_dir_all(workspace.join("skills")).unwrap();
    write_project_skill(&outside, "linked-flow");
    create_directory_link(
        &outside.join("skills/linked-flow"),
        &workspace.join("skills/linked-flow"),
    );
    let tool = SkillViewTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let error = tool
        .execute(
            ToolInput {
                arguments: json!({"name": "linked-flow"}),
                session_id: "turn-linked".to_string(),
                tool_id: "call-linked".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("skill not found"), "{error}");
    remove_directory_link(&workspace.join("skills/linked-flow"));
    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn skill_manage_rejects_linked_support_directory() {
    let workspace = temp_dir("linked-support-write");
    let outside = temp_dir("linked-support-write-target");
    write_project_skill(&workspace, "local-flow");
    fs::remove_dir_all(workspace.join("skills/local-flow/references")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    create_directory_link(&outside, &workspace.join("skills/local-flow/references"));
    let tool = SkillManageTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    let error = tool
        .execute(
            ToolInput {
                arguments: json!({
                    "action": "writeFile",
                    "name": "local-flow",
                    "filePath": "references/new.md",
                    "fileContent": "blocked"
                }),
                session_id: "turn-linked".to_string(),
                tool_id: "call-linked".to_string(),
                revision_base: 0,
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("reparse point"), "{error}");
    assert!(!outside.join("new.md").exists());
    remove_directory_link(&workspace.join("skills/local-flow/references"));
    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn skill_delete_unlinks_support_directory_without_touching_target() {
    let workspace = temp_dir("linked-support-delete");
    let outside = temp_dir("linked-support-delete-target");
    write_project_skill(&workspace, "local-flow");
    fs::remove_dir_all(workspace.join("skills/local-flow/references")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("kept.md"), "kept").unwrap();
    create_directory_link(&outside, &workspace.join("skills/local-flow/references"));
    let tool = SkillManageTool::new(SkillsConfig {
        project_dir: "skills".to_string(),
        ..SkillsConfig::default()
    });

    tool.execute(
        ToolInput {
            arguments: json!({
                "action": "delete",
                "name": "local-flow"
            }),
            session_id: "turn-linked".to_string(),
            tool_id: "call-linked".to_string(),
            revision_base: 0,
        },
        tool_context(workspace.clone()),
    )
    .await
    .unwrap();

    assert!(!workspace.join("skills/local-flow").exists());
    assert_eq!(fs::read_to_string(outside.join("kept.md")).unwrap(), "kept");
    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
