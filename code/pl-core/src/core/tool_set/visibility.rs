use std::collections::BTreeSet;

use pl_model::ToolSchema;

use crate::config::ToolCapabilityConfig;

/// 共享工具 schema 导出选项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SharedToolSchemaOptions {
    pub exec: bool,
    pub workspace_files: bool,
    pub ask_user: bool,
    pub git: bool,
    pub todo: bool,
    pub plan_exit: bool,
}

impl SharedToolSchemaOptions {
    pub fn from_capabilities(capabilities: &ToolCapabilityConfig) -> Self {
        Self {
            exec: capabilities.exec,
            workspace_files: capabilities.workspace_files,
            ask_user: capabilities.ask_user,
            git: capabilities.git,
            todo: true,
            plan_exit: true,
        }
    }

    pub fn with_plan_exit(mut self, enabled: bool) -> Self {
        self.plan_exit = enabled;
        self
    }
}

/// 模型可见工具名集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolVisibilitySet {
    names: BTreeSet<String>,
}

impl ToolVisibilitySet {
    pub fn empty() -> Self {
        Self {
            names: BTreeSet::new(),
        }
    }

    pub fn from_tool_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut set = Self::empty();
        set.extend_tool_names(names);
        set
    }

    pub fn with_tool_names<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extend_tool_names(names);
        self
    }

    pub fn extend_tool_names<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.names.extend(names.into_iter().map(Into::into));
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.names.iter()
    }

    pub fn into_names(self) -> BTreeSet<String> {
        self.names
    }

    pub fn filter_schemas<I>(&self, schemas: I) -> Vec<ToolSchema>
    where
        I: IntoIterator<Item = ToolSchema>,
    {
        schemas
            .into_iter()
            .filter(|schema| self.contains(schema.name()))
            .collect()
    }
}
