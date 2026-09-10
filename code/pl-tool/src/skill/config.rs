//! Skill discovery options supplied by the host. This module performs no persistence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_learn: bool,
    #[serde(default)]
    pub system: SystemSkillsConfig,
    #[serde(default = "default_project_skills_dir")]
    pub project_dir: String,
    #[serde(default = "default_user_skills_dir")]
    pub user_dir: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(default = "default_auto_learn_min_tool_calls")]
    pub auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemSkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_learn: true,
            system: SystemSkillsConfig::default(),
            project_dir: default_project_skills_dir(),
            user_dir: default_user_skills_dir(),
            external_dirs: Vec::new(),
            disabled: Vec::new(),
            auto_learn_min_tool_calls: default_auto_learn_min_tool_calls(),
        }
    }
}

impl Default for SystemSkillsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl SkillsConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn default_true() -> bool {
    true
}

fn default_project_skills_dir() -> String {
    ".agents/skills".to_string()
}

fn default_user_skills_dir() -> String {
    "~/.local/share/pl/skills".to_string()
}

fn default_auto_learn_min_tool_calls() -> u32 {
    5
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn skills_config_uses_standard_project_directory_without_overriding_explicit_values() {
        assert_eq!(SkillsConfig::default().project_dir, ".agents/skills");

        let missing_project_dir: SkillsConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(missing_project_dir.project_dir, ".agents/skills");

        let explicit_legacy_dir: SkillsConfig =
            serde_json::from_str(r#"{"project_dir":"skills"}"#).unwrap();
        assert_eq!(explicit_legacy_dir.project_dir, "skills");
    }
}
