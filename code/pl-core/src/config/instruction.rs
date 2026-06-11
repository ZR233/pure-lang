use serde::{Deserialize, Serialize};

pub const DEFAULT_PROJECT_DOC_MAX_BYTES: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionsConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_override: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub developer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    #[serde(default = "default_project_doc_max_bytes")]
    pub project_doc_max_bytes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_doc_fallback_filenames: Vec<String>,
}

impl Default for InstructionsConfig {
    fn default() -> Self {
        Self {
            base_override: String::new(),
            developer: String::new(),
            user: String::new(),
            project_doc_max_bytes: DEFAULT_PROJECT_DOC_MAX_BYTES,
            project_doc_fallback_filenames: Vec::new(),
        }
    }
}

impl InstructionsConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn default_project_doc_max_bytes() -> usize {
    DEFAULT_PROJECT_DOC_MAX_BYTES
}
