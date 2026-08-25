use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_model::ModelInfo;
use pl_protocol::{MessageContent, MessageRole};
use pretty_assertions::assert_eq;

use super::*;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-instruction-{name}-{stamp}"))
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
        workspace_instructions: None,
        subagent_constraint: Some("subagent rule"),
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
        workspace_instructions: None,
        subagent_constraint: None,
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
    assert!(snapshot.developer[1].content.contains("workspace root"));
    if cfg!(windows) {
        assert_eq!(snapshot.developer[1].source.label, "platform: windows");
        assert!(
            snapshot.developer[1]
                .content
                .contains("Current platform: windows.")
        );
        assert!(
            !snapshot.developer[1]
                .content
                .contains("Current platform: unix.")
        );
    }
    if cfg!(unix) {
        assert_eq!(snapshot.developer[1].source.label, "platform: unix");
        assert!(
            snapshot.developer[1]
                .content
                .contains("Current platform: unix.")
        );
        assert!(
            !snapshot.developer[1]
                .content
                .contains("Current platform: windows.")
        );
    }
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
        workspace_instructions: Some(""),
        subagent_constraint: None,
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
            workspace_instructions: Some("workspace"),
            subagent_constraint: None,
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
    assert!(bundle.prelude_messages.iter().any(|message| {
        matches!(
            &message.content,
            MessageContent::Text(text) if text.contains("profile developer")
        )
    }));
    assert!(bundle.prelude_messages.iter().any(|message| {
        matches!(
            &message.content,
            MessageContent::Text(text) if text.contains("profile user")
        )
    }));
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
            .map(|message| match &message.content {
                MessageContent::Text(text) => text.lines().next().unwrap_or_default(),
                _ => panic!("fixed instruction groups must use text messages"),
            })
            .collect::<Vec<_>>(),
        vec![
            "# Global Developer Instructions",
            "# Global User Context",
            "# Mode and Role Instructions",
            "# Skill Instructions",
            "# Workspace Context",
        ]
    );
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
        workspace_instructions: None,
        subagent_constraint: None,
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
        workspace_instructions: None,
        subagent_constraint: None,
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
        workspace_instructions: None,
        subagent_constraint: None,
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
