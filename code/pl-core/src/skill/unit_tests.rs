use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::scanning::{find_skill_files, support_file_path, validate_skill_document};
use super::system::install_system_skills;
use super::{
    MAX_SKILL_SCAN_DEPTH, SKILL_FILE_NAME, SkillCatalog, SkillMetadata, SkillSourceKind,
    SkillsConfig, build_skills_prompt, bump_project_view,
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

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    assert_eq!(catalog.skills.len(), 1);
    assert_eq!(catalog.skills[0].description, "project");
    assert_eq!(catalog.skills[0].source, SkillSourceKind::Project);
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
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

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

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
        path: skill_dir,
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
fn installs_system_skills_with_marker() {
    let user = temp_dir("system-install-user");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };

    let system_dir = install_system_skills(&config).unwrap();

    assert!(
        system_dir
            .join("skill-creator")
            .join(SKILL_FILE_NAME)
            .exists()
    );
    assert!(
        system_dir
            .join("subagent-workflow")
            .join(SKILL_FILE_NAME)
            .exists()
    );
    let studio_config_doc = system_dir.join("studio-config").join(SKILL_FILE_NAME);
    assert!(
        studio_config_doc.exists(),
        "installed studio-config skill document must exist"
    );
    assert!(system_dir.join(super::SYSTEM_MARKER_FILE_NAME).exists());

    let content = fs::read_to_string(&studio_config_doc)
        .expect("installed studio-config skill document must be readable");
    let metadata = validate_skill_document(&content, Some("studio-config"))
        .expect("installed studio-config skill document must have valid frontmatter");

    assert_eq!(
        metadata.name, "studio-config",
        "installed studio-config skill name must match its directory"
    );
    assert_eq!(
        metadata.category.as_deref(),
        Some("guides"),
        "installed studio-config skill must keep the guides category"
    );
    assert!(
        content.contains("~/.pure/config.toml"),
        "installed studio-config skill must document the canonical config path"
    );

    fs::remove_dir_all(user).unwrap();
}

#[test]
fn system_marker_hit_skips_rewrite() {
    let user = temp_dir("system-marker-user");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    let system_dir = install_system_skills(&config).unwrap();
    let sentinel = system_dir.join("sentinel.txt");
    fs::write(&sentinel, "keep").unwrap();

    install_system_skills(&config).unwrap();

    assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep");
    fs::remove_dir_all(user).unwrap();
}

#[test]
fn stale_system_marker_refreshes_cache() {
    let user = temp_dir("system-refresh-user");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    let system_dir = install_system_skills(&config).unwrap();
    let stale = system_dir.join("stale.txt");
    fs::write(&stale, "remove").unwrap();
    fs::write(system_dir.join(super::SYSTEM_MARKER_FILE_NAME), "stale\n").unwrap();

    install_system_skills(&config).unwrap();

    assert!(!stale.exists());
    assert!(
        system_dir
            .join("skill-creator")
            .join(SKILL_FILE_NAME)
            .exists()
    );
    fs::remove_dir_all(user).unwrap();
}

#[test]
fn discovers_system_skills_between_user_and_external_priority() {
    let workspace = temp_dir("system-priority-workspace");
    let user = temp_dir("system-priority-user");
    let external = temp_dir("system-priority-external");
    write_skill(&user.join("shared"), "shared", "user");
    write_skill(&external.join("skill-creator"), "skill-creator", "external");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    config
        .external_dirs
        .push(external.to_string_lossy().to_string());
    install_system_skills(&config).unwrap();

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    let shared = catalog.find("shared").unwrap();
    let creator = catalog.find("skill-creator").unwrap();
    assert_eq!(shared.source, SkillSourceKind::User);
    assert_eq!(creator.source, SkillSourceKind::System);
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
    fs::remove_dir_all(external).unwrap();
}

#[test]
fn project_skill_shadows_system_skill() {
    let workspace = temp_dir("system-shadow-workspace");
    let user = temp_dir("system-shadow-user");
    write_skill(
        &workspace.join("skills").join("skill-creator"),
        "skill-creator",
        "project override",
    );
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    install_system_skills(&config).unwrap();

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    let creator = catalog.find("skill-creator").unwrap();
    assert_eq!(creator.source, SkillSourceKind::Project);
    assert_eq!(creator.description, "project override");
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
}

#[test]
fn user_root_does_not_scan_system_cache_as_user_scope() {
    let workspace = temp_dir("system-skip-workspace");
    let user = temp_dir("system-skip-user");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    install_system_skills(&config).unwrap();
    config.system.enabled = false;

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    assert!(catalog.find("skill-creator").is_none());
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
}

#[test]
fn system_can_be_disabled() {
    let workspace = temp_dir("system-disabled-workspace");
    let user = temp_dir("system-disabled-user");
    let mut config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    config.system.enabled = false;

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    assert!(catalog.find("skill-creator").is_none());
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(user);
}

#[test]
fn disabled_filters_system_skill_by_name() {
    let workspace = temp_dir("system-disabled-name-workspace");
    let user = temp_dir("system-disabled-name-user");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        disabled: vec!["skill-creator".to_string()],
        ..SkillsConfig::default()
    };
    install_system_skills(&config).unwrap();

    let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

    assert!(catalog.find("skill-creator").is_none());
    assert!(catalog.find("subagent-workflow").is_some());
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
}

#[test]
fn skills_prompt_includes_system_readonly_guidance() {
    let workspace = temp_dir("system-prompt-workspace");
    let user = temp_dir("system-prompt-user");
    let config = SkillsConfig {
        user_dir: user.to_string_lossy().to_string(),
        ..SkillsConfig::default()
    };
    install_system_skills(&config).unwrap();

    let prompt = build_skills_prompt(&workspace, &config).unwrap().unwrap();

    assert!(prompt.contains("skill-creator"));
    assert!(prompt.contains("System/User/External skills 是只读来源"));
    let _ = fs::remove_dir_all(workspace);
    fs::remove_dir_all(user).unwrap();
}
