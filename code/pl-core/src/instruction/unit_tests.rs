use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{ExecutionEnvironment, ExecutionOs, ExecutionTransport, ShellDialect};
use pl_model::model::ModelInfo;
use pl_protocol::MessageRole;
use pretty_assertions::assert_eq;

use super::*;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-instruction-{name}-{stamp}"))
}

fn instruction_skill(
    root: &std::path::Path,
    name: &str,
    description: &str,
) -> crate::skill::SkillMetadata {
    let path = root.join("skills").join(name);
    crate::skill::SkillMetadata {
        name: name.to_string(),
        description: description.to_string(),
        category: None,
        platforms: Vec::new(),
        source: crate::skill::SkillSourceKind::Project,
        path: path.clone(),
        provider_id: crate::skill::SkillProviderId::new("test").unwrap(),
        invocation: crate::skill::SkillInvocationPolicy::default(),
        resource_base: crate::skill::SkillResourceBase::Directory { path },
    }
}

#[test]
fn platform_prompt_tracks_runtime_shell_and_target() {
    let dir = temp_dir("platform-matrix");
    fs::create_dir_all(&dir).unwrap();
    let model = ModelInfo::fallback("test-model");
    let cases = [
        (
            ExecutionOs::Linux,
            ShellDialect::Bash,
            "/bin/bash",
            "local process",
            "bash -c",
        ),
        (
            ExecutionOs::Linux,
            ShellDialect::Sh,
            "/bin/sh",
            "local process",
            "sh -c",
        ),
        (
            ExecutionOs::Windows,
            ShellDialect::Pwsh,
            "pwsh.exe",
            "local process",
            "pwsh -NoProfile",
        ),
        (
            ExecutionOs::Windows,
            ShellDialect::Cmd,
            "cmd.exe",
            "local process",
            "cmd /C",
        ),
    ];
    for (os, shell, path, target, syntax) in cases {
        let os_label = os.as_str().to_string();
        let shell_label = shell.as_str();
        let environment = ExecutionEnvironment {
            transport: ExecutionTransport::Local,
            os,
            shell,
            shell_path: path.into(),
        };
        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            instructions: None,
            skills: None,
            skill_catalog: None,
            execution_profile: None,
            model: &model,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_documents: None,
            workspace_instructions: None,
            subagent_constraint: None,
            skill_suggestions: None,
            execution_environment: Some(&environment),
        })
        .unwrap();
        let platform = &snapshot.developer[0];
        assert_eq!(
            platform.source.label,
            format!("platform: {os_label}/{shell_label}")
        );
        assert!(platform.content.contains(target));
        assert!(platform.content.contains(path));
        assert!(platform.content.contains(syntax));
        assert!(
            platform
                .content
                .contains("Absolute local paths are accepted only")
        );
        assert!(
            !platform
                .content
                .contains("Never pass the remote canonical workspace root")
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ssh_platform_prompt_never_uses_local_platform_template() {
    let dir = temp_dir("platform-ssh");
    fs::create_dir_all(&dir).unwrap();
    let environment = ExecutionEnvironment {
        transport: ExecutionTransport::Ssh,
        os: ExecutionOs::Linux,
        shell: ShellDialect::Sh,
        shell_path: "/bin/sh".into(),
    };
    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: None,
        skill_catalog: None,
        execution_profile: None,
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: Some(&environment),
    })
    .unwrap();
    let platform = &snapshot.developer[0];
    assert_eq!(platform.source.label, "platform: linux/sh");
    assert!(platform.content.contains("SSH remote helper"));
    assert!(platform.content.contains(
        "For `exec.cwd`, use only a workspace-relative path: `.` for the workspace root"
    ));
    assert!(
        platform
            .content
            .contains("Never pass the remote canonical workspace root")
    );
    assert!(
        !platform
            .content
            .contains("`exec.cwd`, and LSP filePath inputs may be relative or absolute")
    );
    assert!(
        !platform
            .content
            .contains("Absolute local paths are accepted only")
    );
    assert!(!platform.content.contains("Current platform: windows"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn profile_base_override_snapshot_constructs_host_instruction_block() {
    let snapshot =
        InstructionSnapshot::profile_base_override("mai-team instructions", "host prompt");

    assert_eq!(
        snapshot,
        InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource {
                    kind: InstructionSourceKind::ProfileBaseOverride,
                    label: "mai-team instructions".to_string(),
                    path: None,
                },
                content: "host prompt".to_string(),
            },
            developer: Vec::new(),
            user: Vec::new(),
        }
    );
}

#[test]
fn assembles_three_layers_in_stable_order() {
    let dir = temp_dir("order");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("AGENTS.md"), "project rules").unwrap();
    let config = crate::config::InstructionsConfig {
        developer: "config dev".to_string(),
        user: "config user".to_string(),
        ..crate::config::InstructionsConfig::default()
    };

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config),
        skills: None,
        skill_catalog: None,
        execution_profile: Some(ExecutionInstructionProfile {
            label: "test",
            instructions: "mode instructions",
        }),
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: Some("subagent rule"),
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(
        snapshot
            .developer
            .iter()
            .map(|block| block.source.kind)
            .collect::<Vec<_>>(),
        vec![
            InstructionSourceKind::ExecutionProfile,
            InstructionSourceKind::Platform,
            InstructionSourceKind::ConfigDeveloper,
            InstructionSourceKind::SubagentConstraint
        ]
    );
    assert_eq!(
        snapshot
            .user
            .iter()
            .map(|block| block.source.kind)
            .collect::<Vec<_>>(),
        vec![
            InstructionSourceKind::ConfigUser,
            InstructionSourceKind::ProjectDoc
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn uses_preloaded_workspace_documents_for_non_local_workspace() {
    let workspace_root = temp_dir("remote");
    let config = crate::config::InstructionsConfig::default();
    let documents = crate::workspace::WorkspaceInstructions {
        documents: vec![crate::workspace::WorkspaceInstructionDocument {
            path: std::path::PathBuf::from("/home/zhourui/opensource/pure-lang/AGENTS.md"),
            content: "remote project rules".to_string(),
            bytes: "remote project rules".len(),
        }],
    };

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config),
        skills: None,
        skill_catalog: None,
        execution_profile: None,
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &workspace_root,
        current_dir: &workspace_root,
        workspace_documents: Some(&documents),
        workspace_instructions: Some("remote project rules"),
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(snapshot.user.len(), 1);
    assert_eq!(
        snapshot.user[0].source.kind,
        InstructionSourceKind::ProjectDoc
    );
    assert_eq!(snapshot.user[0].content, "remote project rules");
}

#[test]
fn platform_block_is_after_mode_and_before_config_developer() {
    let dir = temp_dir("platform-order");
    fs::create_dir_all(&dir).unwrap();
    let config = crate::config::InstructionsConfig {
        developer: "config dev".to_string(),
        ..crate::config::InstructionsConfig::default()
    };

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config),
        skills: None,
        skill_catalog: None,
        execution_profile: Some(ExecutionInstructionProfile {
            label: "test",
            instructions: "mode instructions",
        }),
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(
        snapshot.developer[0].source.kind,
        InstructionSourceKind::ExecutionProfile
    );
    assert_eq!(
        snapshot.developer[1].source.kind,
        InstructionSourceKind::Platform
    );
    assert_eq!(
        snapshot.developer[2].source.kind,
        InstructionSourceKind::ConfigDeveloper
    );
    let environment = crate::ExecutionEnvironment::detect_local();
    assert_eq!(
        snapshot.developer[1].source.label,
        format!(
            "platform: {}/{}",
            environment.os.as_str(),
            environment.shell.as_str()
        )
    );
    assert!(
        snapshot.developer[1]
            .content
            .contains("Runtime execution environment")
    );
    assert!(
        snapshot.developer[1]
            .content
            .contains(&environment.shell_path_display())
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn filters_empty_blocks_and_uses_model_base() {
    let dir = temp_dir("empty");
    fs::create_dir_all(&dir).unwrap();
    let mut model = ModelInfo::fallback("test-model");
    model.base_instructions = "model base".to_string();

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: None,
        skill_catalog: None,
        execution_profile: None,
        model: &model,
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: Some(""),
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(snapshot.base.content, "model base");
    assert_eq!(snapshot.user, Vec::new());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn profile_can_override_base_and_add_context_blocks() {
    let dir = temp_dir("profile");
    fs::create_dir_all(&dir).unwrap();
    let profile = InstructionProfile::new()
        .with_base_system_prompt("profile base")
        .with_developer_block("runtime", "profile developer")
        .with_user_context_block("host", "profile user");

    let snapshot = InstructionAssembler::assemble_with_profile(
        InstructionAssemblyRequest {
            instructions: None,
            skills: None,
            skill_catalog: None,
            execution_profile: None,
            model: &ModelInfo::fallback("test-model"),
            workspace_root: &dir,
            current_dir: &dir,
            workspace_documents: None,
            workspace_instructions: Some("workspace"),
            subagent_constraint: None,
            skill_suggestions: None,
            execution_environment: None,
        },
        &profile,
    )
    .unwrap();
    let bundle = snapshot.to_bundle();

    assert_eq!(snapshot.base.content, "profile base");
    assert_eq!(
        snapshot.base.source.kind,
        InstructionSourceKind::ProfileBaseOverride
    );
    assert!(
        bundle
            .prelude_messages
            .iter()
            .any(|message| message.content.text_value().contains("profile developer"))
    );
    assert!(
        bundle
            .prelude_messages
            .iter()
            .any(|message| message.content.text_value().contains("profile user"))
    );
    assert_eq!(
        snapshot
            .user
            .iter()
            .map(|block| block.source.kind)
            .collect::<Vec<_>>(),
        vec![
            InstructionSourceKind::WorkspaceFallback,
            InstructionSourceKind::ProfileUser,
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bundle_orders_fixed_layers_from_global_to_workspace() {
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
            content: "base".to_string(),
        },
        developer: vec![
            InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::Platform, "platform"),
                content: "platform".to_string(),
            },
            InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ExecutionProfile, "mode"),
                content: "mode".to_string(),
            },
            InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::Skills, "skills"),
                content: "skills".to_string(),
            },
        ],
        user: vec![
            InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ConfigUser, "user"),
                content: "global user".to_string(),
            },
            InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ProjectDoc, "project"),
                content: "workspace".to_string(),
            },
        ],
    };

    let bundle = snapshot.to_bundle();

    assert_eq!(bundle.instructions, "base");
    assert_eq!(bundle.prelude_messages.len(), 5);
    assert_eq!(
        bundle
            .prelude_messages
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![
            MessageRole::System,
            MessageRole::User,
            MessageRole::System,
            MessageRole::System,
            MessageRole::User,
        ]
    );
    assert_eq!(
        bundle
            .prelude_messages
            .iter()
            .map(|message| message.content.text_value())
            .map(|text| text.lines().next().unwrap_or_default().to_string())
            .collect::<Vec<_>>(),
        vec![
            "# Global Developer Instructions".to_string(),
            "# Global User Context".to_string(),
            "# Mode and Role Instructions".to_string(),
            "# Skill Instructions".to_string(),
            "# Workspace Context".to_string(),
        ]
    );
}

#[test]
fn visible_tool_group_guidance_has_its_own_system_prefix_section() {
    let group = crate::ToolGroupId::new("mcp:business");
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
            content: "base".to_string(),
        },
        developer: Vec::new(),
        user: Vec::new(),
    }
    .with_tool_group_instructions([(&group, "Use only for approved business keys.")]);

    let bundle = snapshot.to_bundle();

    assert_eq!(bundle.prelude_messages.len(), 1);
    assert_eq!(bundle.prelude_messages[0].role, MessageRole::System);
    let content = bundle.prelude_messages[0].content.text_value();
    assert!(content.starts_with("# Tool Group Instructions"));
    assert!(content.contains("## tool group mcp:business"));
    assert!(content.contains("Use only for approved business keys."));
    assert!(bundle.prefix_section_hashes.contains_key("toolGroups"));
}

#[test]
fn direct_skill_invocation_is_a_transient_user_instruction_group() {
    let mut snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
            content: "base".to_string(),
        },
        developer: Vec::new(),
        user: Vec::new(),
    };
    snapshot.push_skill_invocation("follow the review skill");

    let bundle = snapshot.to_bundle();

    assert_eq!(bundle.prelude_messages.len(), 1);
    assert_eq!(bundle.prelude_messages[0].role, MessageRole::User);
    let content = bundle.prelude_messages[0].content.text_value();
    assert!(content.starts_with("# Turn Skill Instructions"));
    assert!(content.contains("follow the review skill"));
    assert!(bundle.prefix_section_hashes.contains_key("turnSkills"));
}

#[test]
fn config_base_override_replaces_model_base() {
    let dir = temp_dir("base-override");
    fs::create_dir_all(&dir).unwrap();
    let config = crate::config::InstructionsConfig {
        base_override: "config base".to_string(),
        ..crate::config::InstructionsConfig::default()
    };
    let mut model = ModelInfo::fallback("test-model");
    model.base_instructions = "model base".to_string();

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config),
        skills: None,
        skill_catalog: None,
        execution_profile: Some(ExecutionInstructionProfile {
            label: "test",
            instructions: "mode instructions",
        }),
        model: &model,
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(snapshot.base.content, "config base");
    assert_eq!(
        snapshot.base.source.kind,
        InstructionSourceKind::ConfigBaseOverride
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn built_in_base_requires_doc_first_and_final_review() {
    let dir = temp_dir("built-in-base-doc-flow");
    fs::create_dir_all(&dir).unwrap();

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: None,
        skill_catalog: None,
        execution_profile: None,
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert_eq!(
        snapshot.base.source.kind,
        InstructionSourceKind::BuiltInBase
    );
    assert!(snapshot.base.content.contains("再开始写代码"));
    assert!(snapshot.base.content.contains("整体回看计划"));
    assert!(snapshot.base.content.contains("首次调用工具前必须输出一句"));
    assert!(snapshot.base.content.contains("每次 commentary 只写 1 句"));
    assert!(snapshot.base.content.contains("final 只出现一次"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn disabled_skills_do_not_inject_a_frozen_catalog() {
    let dir = temp_dir("skills-disabled");
    fs::create_dir_all(&dir).unwrap();
    let skills = crate::config::SkillsConfig {
        enabled: false,
        ..crate::config::SkillsConfig::default()
    };
    let catalog = crate::skill::SkillCatalog {
        project_dir: dir.join("skills"),
        skills: Vec::new(),
        warnings: Vec::new(),
        complete: true,
    };

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: Some(&skills),
        skill_catalog: Some(&catalog),
        execution_profile: None,
        model: &ModelInfo::fallback("test-model"),
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: None,
        execution_environment: None,
    })
    .unwrap();

    assert!(
        snapshot
            .developer
            .iter()
            .all(|block| block.source.kind != InstructionSourceKind::Skills)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_suggestions_are_turn_overlays_and_exclude_loaded_names() {
    let dir = temp_dir("skill-suggestions");
    fs::create_dir_all(&dir).unwrap();
    let skills = crate::config::SkillsConfig::default();
    let catalog = crate::skill::SkillCatalog {
        project_dir: dir.join("skills"),
        skills: vec![
            instruction_skill(
                &dir,
                "release-build-triage",
                "Diagnose Rust release linker and Cargo profile failures.",
            ),
            instruction_skill(&dir, "rust-formatting", "Format Rust source files."),
        ],
        warnings: Vec::new(),
        complete: true,
    };
    let model = ModelInfo::fallback("test-model");
    let first = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: Some(&skills),
        skill_catalog: Some(&catalog),
        execution_profile: None,
        model: &model,
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: Some(SkillSuggestionRequest {
            query: "diagnose a Rust release linker failure",
            excluded_names: &[],
        }),
        execution_environment: None,
    })
    .unwrap();
    let excluded = vec!["RELEASE-BUILD-TRIAGE".to_string()];
    let second = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: Some(&skills),
        skill_catalog: Some(&catalog),
        execution_profile: None,
        model: &model,
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: Some(SkillSuggestionRequest {
            query: "diagnose a Rust release linker failure",
            excluded_names: &excluded,
        }),
        execution_environment: None,
    })
    .unwrap();

    let first_catalog = first
        .developer
        .iter()
        .find(|block| block.source.kind == InstructionSourceKind::Skills)
        .unwrap();
    let second_catalog = second
        .developer
        .iter()
        .find(|block| block.source.kind == InstructionSourceKind::Skills)
        .unwrap();
    assert_eq!(first_catalog.content, second_catalog.content);
    let suggestion = first.user.last().unwrap();
    assert_eq!(
        suggestion.source.kind,
        InstructionSourceKind::SkillSuggestions
    );
    assert!(suggestion.content.contains("release-build-triage"));
    assert!(suggestion.content.contains("skill_view"));
    let turn_overlay = first
        .to_bundle()
        .prelude_messages
        .into_iter()
        .find(|message| {
            message
                .content
                .text_value()
                .starts_with("# Turn Skill Instructions")
        })
        .expect("Skill suggestions must enter the model-visible Turn overlay");
    assert_eq!(turn_overlay.role, MessageRole::User);
    assert!(
        turn_overlay
            .content
            .text_value()
            .contains("<skill_suggestions>")
    );
    let excluded_suggestion = second
        .user
        .iter()
        .find(|block| block.source.kind == InstructionSourceKind::SkillSuggestions)
        .unwrap();
    assert!(!excluded_suggestion.content.contains("release-build-triage"));
    assert!(excluded_suggestion.content.contains("rust-formatting"));
    assert!(
        first
            .user
            .iter()
            .all(|block| block.source.kind != InstructionSourceKind::SkillInvocation)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unmatched_skill_query_does_not_add_a_turn_overlay() {
    let dir = temp_dir("skill-suggestions-empty");
    fs::create_dir_all(&dir).unwrap();
    let skills = crate::config::SkillsConfig::default();
    let catalog = crate::skill::SkillCatalog {
        project_dir: dir.join("skills"),
        skills: vec![instruction_skill(&dir, "rust-formatting", "Format Rust")],
        warnings: Vec::new(),
        complete: true,
    };
    let model = ModelInfo::fallback("test-model");

    let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: None,
        skills: Some(&skills),
        skill_catalog: Some(&catalog),
        execution_profile: None,
        model: &model,
        workspace_root: &dir,
        current_dir: &dir,
        workspace_documents: None,
        workspace_instructions: None,
        subagent_constraint: None,
        skill_suggestions: Some(SkillSuggestionRequest {
            query: "tell me a joke about penguins",
            excluded_names: &[],
        }),
        execution_environment: None,
    })
    .unwrap();

    assert!(
        snapshot
            .user
            .iter()
            .all(|block| block.source.kind != InstructionSourceKind::SkillSuggestions)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn force_dispatch_is_added_to_clone_only() {
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
            content: "base".to_string(),
        },
        developer: Vec::new(),
        user: Vec::new(),
    };

    let forced = snapshot.clone().with_subagent_force("force");

    assert_eq!(snapshot.developer.len(), 0);
    assert_eq!(forced.developer.len(), 1);
    assert_eq!(
        forced.developer[0].source.kind,
        InstructionSourceKind::SubagentForce
    );
}

#[test]
fn subagent_constraint_is_added_to_clone_only() {
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
            content: "base".to_string(),
        },
        developer: Vec::new(),
        user: Vec::new(),
    };

    let constrained = snapshot.clone().with_subagent_constraint("constraint");

    assert_eq!(snapshot.developer.len(), 0);
    assert_eq!(constrained.developer.len(), 1);
    assert_eq!(
        constrained.developer[0].source.kind,
        InstructionSourceKind::SubagentConstraint
    );
}
