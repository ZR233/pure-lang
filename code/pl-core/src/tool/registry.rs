use std::fmt;

use pl_model::ToolSchema;

use super::{Tool, lsp_tool_for_language};

/// 工具注册表。
///
/// 管理已注册的工具实例，提供按名称查找和 schema 收集能力。
#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.name()).collect();
        f.debug_struct("ToolRegistry")
            .field("tools", &names)
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        assert!(
            self.get(tool.name()).is_none(),
            "duplicate tool name: {}",
            tool.name()
        );
        self.tools.push(Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| &**t)
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.iter().map(|t| t.to_schema()).collect()
    }

    /// 按宿主数据化策略过滤模型可见 schema。
    pub fn schemas_for_policy(&self, policy: &crate::AgentExecutionPolicy) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .filter(|tool| policy.allows_tool(tool.name(), tool.effect()))
            .map(|tool| tool.to_schema())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 移除指定名称的工具（用于动态卸载）。
    pub fn unregister(&mut self, name: &str) -> bool {
        let len_before = self.tools.len();
        self.tools.retain(|tool| tool.name() != name);
        self.tools.len() != len_before
    }

    /// 注册当前可用的语言 LSP 工具。
    ///
    /// 遍历 `available_languages()` 返回的语言列表，为每个语言注册一个
    /// `LspLanguageTool`。同时移除之前注册但已不再可用的语言工具。
    pub async fn register_lsp_languages(
        &mut self,
        registry: &pl_lsp::LspRuntimeRegistry,
    ) -> Vec<String> {
        let available = registry.available_languages().await;
        self.sync_lsp_language_tools(registry, available)
    }

    pub async fn register_lsp_languages_for_workspace(
        &mut self,
        registry: &pl_lsp::LspRuntimeRegistry,
        workspace_root: impl AsRef<std::path::Path>,
    ) -> Vec<String> {
        let available = registry
            .available_languages_for_workspace(workspace_root)
            .await;
        self.sync_lsp_language_tools(registry, available)
    }

    pub(super) fn sync_lsp_language_tools(
        &mut self,
        registry: &pl_lsp::LspRuntimeRegistry,
        available: Vec<pl_lsp::LanguageToolInfo>,
    ) -> Vec<String> {
        let tool_names: Vec<String> = available
            .iter()
            .map(|info| format!("lsp_query_{}", info.language_id))
            .collect();
        self.tools.retain(|tool| {
            let name = tool.name();
            if name.starts_with("lsp_query_") {
                tool_names.iter().any(|tn| tn == name)
            } else {
                true
            }
        });
        let mut registered = Vec::new();
        for info in &available {
            let lang_id = &info.language_id;
            let tool_name = format!("lsp_query_{lang_id}");
            if self.get(&tool_name).is_none() {
                self.tools
                    .push(lsp_tool_for_language(info, registry.clone()));
            }
            if !registered.contains(&info.language_id) {
                registered.push(info.language_id.clone());
            }
        }
        registered
    }
}
