use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::session::CoreSession;
use crate::turn::{CompileMode, TurnOptions};

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
        mode: CompileMode::Auto,
        workspace_root,
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        agent_supervisor: crate::AgentSupervisor::default(),
        lsp_runtime: None,
        parent_session: Arc::new(CoreSession::new()),
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
    let input = SkillManageInput {
        action: SkillManageAction::Create,
        name: "local-flow".to_string(),
        content: Some(skill_content("local-flow", "Local flow")),
        category: None,
        file_path: None,
        file_content: None,
        old_string: None,
        new_string: None,
        replace_mode: None,
        absorbed_into: None,
    };

    create_skill("skill_manage", &catalog, input).unwrap();

    assert!(workspace.join("skills/local-flow/SKILL.md").exists());
    fs::remove_dir_all(workspace).unwrap();
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
    let input = SkillManageInput {
        action: SkillManageAction::Patch,
        name: "local-flow".to_string(),
        content: None,
        category: None,
        file_path: None,
        file_content: None,
        old_string: Some(r#"Snippet: `\"unknown\\nusage\"`"#.to_string()),
        new_string: Some("Snippet: `\"known\\nusage\"`".to_string()),
        replace_mode: None,
        absorbed_into: None,
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
