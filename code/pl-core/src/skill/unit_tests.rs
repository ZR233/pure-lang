use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::scanning::{find_skill_files, support_file_path, validate_skill_document};
use super::{
    MAX_SKILL_SCAN_DEPTH, SKILL_FILE_NAME, SkillCatalog, SkillMetadata, SkillSourceKind,
    SkillsConfig, build_skills_prompt, build_skills_prompt_from_catalog, bump_project_view,
};

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    env::temp_dir().join(format!("pure-skill-{name}-{stamp}"))
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join(SKILL_FILE_NAME),
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n"),
    )
    .unwrap();
}

fn discover_without_agents_home(
    workspace: &Path,
    config: &SkillsConfig,
    system: Option<&Path>,
) -> SkillCatalog {
    SkillCatalog::discover_with_agents_user_dir(workspace, config, system, None).unwrap()
}

fn catalog_skill(name: &str, description: &str) -> SkillMetadata {
    let path = PathBuf::from("skills").join(name);
    SkillMetadata {
        name: name.to_string(),
        description: description.to_string(),
        category: None,
        platforms: Vec::new(),
        source: SkillSourceKind::Project,
        path: path.clone(),
        provider_id: super::SkillProviderId::new("test").unwrap(),
        invocation: super::SkillInvocationPolicy::default(),
        resource_base: super::SkillResourceBase::Directory { path },
    }
}

#[test]
fn parses_valid_frontmatter() {
    let content = "---\nname: rust-flow\ndescription: Rust flow\nplatforms: [windows]\n---\nBody";

    let metadata = validate_skill_document(content, Some("rust-flow")).unwrap();

    assert_eq!(metadata.name, "rust-flow");
    assert_eq!(metadata.description, "Rust flow");
    assert_eq!(metadata.platforms, vec!["windows".to_string()]);
}

#[test]
fn rejects_missing_frontmatter() {
    let error = validate_skill_document("# Nope", None)
        .unwrap_err()
        .to_string();

    assert!(error.contains("frontmatter"));
}

#[test]
fn project_source_shadows_user_and_external() {
    let workspace = temp_dir("shadow-workspace");
    let user = temp_dir("shadow-user");
    let external = temp_dir("shadow-external");
    write_skill(
        &workspace.join("skills").join("shared"),
        "shared",
        "project",
    );
    write_skill(&user.join("shared"), "shared", "user");
    write_skill(&external.join("shared"), "shared", "external");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    config.system.enabled = false;
    config
        .external_dirs
        .push(external.to_string_lossy().to_string());

    let catalog = discover_without_agents_home(&workspace, &config, None);

    assert_eq!(catalog.skills.len(), 1);
    assert_eq!(catalog.skills[0].description, "project");
    assert_eq!(catalog.skills[0].source, SkillSourceKind::Project);
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(external).unwrap();
}

#[test]
fn disabled_skills_are_filtered() {
    let workspace = temp_dir("disabled");
    write_skill(&workspace.join("skills").join("hidden"), "hidden", "hidden");
    let mut config = SkillsConfig {
        disabled: vec!["hidden".to_string()],
        ..SkillsConfig::default()
    };
    config.system.enabled = false;

    let catalog = discover_without_agents_home(&workspace, &config, None);

    assert!(catalog.skills.is_empty());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn scan_stops_below_discovered_skill() {
    let root = temp_dir("parent-stops-scan");
    let parent = root.join("parent");
    let child = parent.join("child");
    write_skill(&parent, "parent", "parent");
    write_skill(&child, "child", "child");

    assert_eq!(find_skill_files(&root), vec![parent.join(SKILL_FILE_NAME)]);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scan_respects_maximum_directory_depth() {
    let root = temp_dir("maximum-depth");
    let included = (0..MAX_SKILL_SCAN_DEPTH).fold(root.clone(), |path, index| {
        path.join(format!("level-{index}"))
    });
    let excluded = included.join("too-deep");
    write_skill(&included, "included", "included");
    write_skill(&excluded, "excluded", "excluded");

    assert_eq!(
        find_skill_files(&root),
        vec![included.join(SKILL_FILE_NAME)]
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn usage_update_replaces_existing_file_atomically() {
    let project = temp_dir("usage-replace");
    let skill_dir = project.join("skills").join("usage");
    write_skill(&skill_dir, "usage", "usage");
    let skill = SkillMetadata {
        name: "usage".to_string(),
        description: "usage".to_string(),
        category: None,
        platforms: Vec::new(),
        source: SkillSourceKind::Project,
        path: skill_dir.clone(),
        provider_id: super::SkillProviderId::new("local-filesystem").unwrap(),
        invocation: super::SkillInvocationPolicy::default(),
        resource_base: super::SkillResourceBase::Directory {
            path: skill_dir.clone(),
        },
    };

    bump_project_view(&project, &skill).unwrap();
    bump_project_view(&project, &skill).unwrap();

    let usage: super::SkillUsage =
        serde_json::from_str(&fs::read_to_string(skill_dir.join(super::USAGE_FILE_NAME)).unwrap())
            .unwrap();
    assert_eq!(usage.views, 2);
    assert_eq!(usage.uses, 2);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn corrupted_usage_is_observable() {
    let project = temp_dir("usage-corrupted");
    let skill_dir = project.join("skills").join("usage");
    write_skill(&skill_dir, "usage", "usage");
    fs::write(skill_dir.join(super::USAGE_FILE_NAME), "not-json").unwrap();
    let skill = SkillMetadata {
        name: "usage".to_string(),
        description: "usage".to_string(),
        category: None,
        platforms: Vec::new(),
        source: SkillSourceKind::Project,
        path: skill_dir.clone(),
        provider_id: super::SkillProviderId::new("local-filesystem").unwrap(),
        invocation: super::SkillInvocationPolicy::default(),
        resource_base: super::SkillResourceBase::Directory { path: skill_dir },
    };

    let error = bump_project_view(&project, &skill).unwrap_err().to_string();

    assert!(error.contains("failed to parse skill usage"));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn support_file_rejects_traversal() {
    let error = support_file_path("../AGENTS.md").unwrap_err().to_string();

    assert!(error.contains("relative"));
}

#[test]
fn support_file_requires_allowed_directory() {
    let error = support_file_path("notes/file.md").unwrap_err().to_string();

    assert!(error.contains("support file path"));
}

#[test]
fn agents_user_directory_is_discovered_between_configured_user_and_system() {
    let workspace = temp_dir("agents-user-workspace");
    let configured_user = temp_dir("agents-configured-user");
    let home = temp_dir("agents-home");
    let agents_user = home.join(".agents").join("skills");
    let system = temp_dir("agents-system");
    write_skill(&configured_user.join("shared"), "shared", "configured user");
    write_skill(&agents_user.join("shared"), "shared", "agents user");
    write_skill(
        &agents_user.join("agents-only"),
        "agents-only",
        "agents user",
    );
    write_skill(&system.join("agents-only"), "agents-only", "system");
    let config = SkillsConfig {
        user_dir: configured_user.to_string_lossy().into_owned(),
        ..SkillsConfig::default()
    };

    let catalog = SkillCatalog::discover_with_agents_user_dir(
        &workspace,
        &config,
        Some(&system),
        Some(&agents_user),
    )
    .unwrap();

    assert_eq!(
        catalog.find("shared").unwrap().description,
        "configured user"
    );
    let agents_only = catalog.find("agents-only").unwrap();
    assert_eq!(agents_only.description, "agents user");
    assert_eq!(agents_only.source, SkillSourceKind::User);
    assert_eq!(agents_only.path, agents_user.join("agents-only"));
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(configured_user).unwrap();
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(system).unwrap();
}

#[test]
fn configured_and_agents_user_same_directory_is_scanned_once() {
    let workspace = temp_dir("agents-dedup-workspace");
    let agents_user = temp_dir("agents-dedup-user");
    let invalid = agents_user.join("invalid");
    fs::create_dir_all(&invalid).unwrap();
    fs::write(invalid.join(SKILL_FILE_NAME), "missing frontmatter").unwrap();
    let config = SkillsConfig {
        user_dir: agents_user.to_string_lossy().into_owned(),
        ..SkillsConfig::default()
    };

    let catalog =
        SkillCatalog::discover_with_agents_user_dir(&workspace, &config, None, Some(&agents_user))
            .unwrap();

    assert_eq!(catalog.warnings.len(), 1);
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(agents_user).unwrap();
}

#[test]
fn discovers_system_skills_between_user_and_external_priority() {
    let workspace = temp_dir("system-priority-workspace");
    let user = temp_dir("system-priority-user");
    let system = temp_dir("system-priority-system");
    let external = temp_dir("system-priority-external");
    write_skill(&user.join("shared"), "shared", "user");
    write_skill(&system.join("skill-creator"), "skill-creator", "system");
    write_skill(&external.join("skill-creator"), "skill-creator", "external");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    config
        .external_dirs
        .push(external.to_string_lossy().to_string());
    let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

    let shared = catalog.find("shared").unwrap();
    let creator = catalog.find("skill-creator").unwrap();
    assert_eq!(shared.source, SkillSourceKind::User);
    assert_eq!(creator.source, SkillSourceKind::System);
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(system).unwrap();
    fs::remove_dir_all(external).unwrap();
}

#[test]
fn project_skill_shadows_system_skill() {
    let workspace = temp_dir("system-shadow-workspace");
    let user = temp_dir("system-shadow-user");
    let system = temp_dir("system-shadow-system");
    write_skill(
        &workspace.join("skills").join("skill-creator"),
        "skill-creator",
        "project override",
    );
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    write_skill(&system.join("skill-creator"), "skill-creator", "system");

    let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

    let creator = catalog.find("skill-creator").unwrap();
    assert_eq!(creator.source, SkillSourceKind::Project);
    assert_eq!(creator.description, "project override");
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(system).unwrap();
}

#[test]
fn system_directory_is_not_derived_from_user_directory() {
    let workspace = temp_dir("system-independent-workspace");
    let user = temp_dir("system-independent-user");
    let explicit_system = temp_dir("system-independent-explicit");
    write_skill(
        &user.join(".system").join("legacy"),
        "legacy",
        "legacy system",
    );
    write_skill(
        &explicit_system.join("current"),
        "current",
        "current system",
    );
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };

    let without_system = discover_without_agents_home(&workspace, &config, None);
    let with_system = discover_without_agents_home(&workspace, &config, Some(&explicit_system));

    assert!(without_system.find("legacy").is_none());
    assert!(without_system.find("current").is_none());
    assert!(with_system.find("legacy").is_none());
    assert_eq!(
        with_system.find("current").unwrap().source,
        SkillSourceKind::System
    );
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
    fs::remove_dir_all(explicit_system).unwrap();
}

#[test]
fn system_can_be_disabled() {
    let workspace = temp_dir("system-disabled-workspace");
    let user = temp_dir("system-disabled-user");
    let system = temp_dir("system-disabled-system");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    config.system.enabled = false;
    write_skill(&system.join("skill-creator"), "skill-creator", "system");

    let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

    assert!(catalog.find("skill-creator").is_none());
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(system).unwrap();
}

#[test]
fn disabled_filters_system_skill_by_name() {
    let workspace = temp_dir("system-disabled-name-workspace");
    let user = temp_dir("system-disabled-name-user");
    let system = temp_dir("system-disabled-name-system");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        disabled: vec!["skill-creator".to_string()],
        ..SkillsConfig::default()
    };
    write_skill(&system.join("skill-creator"), "skill-creator", "system");
    write_skill(
        &system.join("subagent-workflow"),
        "subagent-workflow",
        "system",
    );

    let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

    assert!(catalog.find("skill-creator").is_none());
    assert!(catalog.find("subagent-workflow").is_some());
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(system).unwrap();
}

#[test]
fn skills_prompt_includes_system_readonly_guidance() {
    let workspace = temp_dir("system-prompt-workspace");
    let user = temp_dir("system-prompt-user");
    let system = temp_dir("system-prompt-system");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    write_skill(&system.join("skill-creator"), "skill-creator", "system");

    let prompt = build_skills_prompt(&workspace, &config, Some(&system))
        .unwrap()
        .unwrap();

    assert!(prompt.contains("skill-creator"));
    assert!(prompt.contains("System/User/External skills 是只读来源"));
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
    fs::remove_dir_all(system).unwrap();
}

#[test]
fn skills_prompt_sorts_and_only_normalizes_the_model_projection() {
    let long_description = format!("first\n\tsecond   {}", "界".repeat(510));
    let mut hidden = catalog_skill("hidden", "not model visible");
    hidden.invocation.model_invocable = false;
    let catalog = SkillCatalog {
        project_dir: PathBuf::from("skills"),
        skills: vec![
            catalog_skill("Zulu", "last"),
            catalog_skill("alpha", &long_description),
            hidden,
        ],
        warnings: Vec::new(),
        complete: true,
    };

    let prompt = build_skills_prompt_from_catalog(&catalog);

    assert!(prompt.find("`alpha`").unwrap() < prompt.find("`Zulu`").unwrap());
    assert!(!prompt.contains("`hidden`"));
    assert!(prompt.contains("first second"));
    let projected = prompt
        .lines()
        .find(|line| line.starts_with("- `alpha`:"))
        .unwrap();
    assert_eq!(
        projected
            .strip_prefix("- `alpha`: ")
            .unwrap()
            .chars()
            .count(),
        500
    );
    assert!(projected.ends_with("..."));
    assert_eq!(catalog.find("alpha").unwrap().description, long_description);
}
