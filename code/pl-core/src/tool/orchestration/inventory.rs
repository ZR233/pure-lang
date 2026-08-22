//! 一次 Turn 冻结的工具目录编排:eager/延迟分配、policy 过滤与 schema 投影。

use pl_model::ToolSchema;

use super::super::source::ToolEntry;
use super::search::{ClientToolSearchCatalog, ClientToolSearchEntry, client_tool_search_schema};
use crate::turn::ToolEffect;

/// 当前模型请求可启用的工具编排能力。
///
/// Tool Search 只有客户端执行一种路径：请求携带 eager schema 与一个 schema
/// 固定的 `tool_search` function 工具，检索在本 Turn 冻结的 catalog 上执行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolOrchestrationOptions {
    pub tool_search: bool,
    pub programmatic_tool_calling: bool,
}

/// 一次 Turn 冻结的完整工具目录。
///
/// `request_schemas` 是实际发送给 provider 的 canonical 化并排序后的 schema
/// 集合；`catalog` 是延迟加载工具的检索目录，变化不影响请求前缀。
#[derive(Debug, Clone, Default)]
pub struct ToolInventory {
    pub(super) request_schemas: Vec<ToolSchema>,
    pub(super) catalog: Option<ClientToolSearchCatalog>,
}

impl ToolInventory {
    pub fn request_schemas(&self) -> &[ToolSchema] {
        &self.request_schemas
    }

    pub(crate) fn catalog(&self) -> Option<&ClientToolSearchCatalog> {
        self.catalog.as_ref()
    }

    /// 延迟加载 catalog 的 canonical 哈希；无 catalog 时为 `None`。
    pub fn catalog_fingerprint(&self) -> Option<String> {
        self.catalog
            .as_ref()
            .map(ClientToolSearchCatalog::fingerprint)
    }
}

/// 把 lease 条目编排为一次 Turn 的模型可见工具集合。
///
/// eager/policy 过滤后，携带 namespace 元数据的 Function 工具进入延迟 catalog；
/// 其余工具 eager 上线。programmatic 资格完全由来源元数据驱动
/// （`programmatic_eligible` 且 effect 为 `Read`）。排序 canonical 化只在此处
/// 发生一次。
pub fn orchestrate_tool_inventory(
    entries: &[ToolEntry],
    policy: Option<&crate::AgentExecutionPolicy>,
    options: ToolOrchestrationOptions,
) -> ToolInventory {
    let mut eager = Vec::new();
    let mut has_programmatic = false;
    let mut deferred = Vec::new();

    for entry in entries {
        let tool = entry.tool();
        let effect = tool.effect();
        if policy.is_some_and(|policy| !policy.allows_tool(entry.name(), effect)) {
            continue;
        }
        let mut schema = tool.to_schema();
        if options.programmatic_tool_calling
            && entry.metadata().programmatic_eligible
            && effect == Some(ToolEffect::Read)
            && matches!(schema, ToolSchema::Function { .. })
        {
            let output_schema = match &schema {
                ToolSchema::Function {
                    output_schema: Some(output_schema),
                    ..
                } => output_schema.clone(),
                _ => generic_object_output_schema(),
            };
            schema = schema.allow_programmatic(output_schema);
            has_programmatic = true;
        }
        match entry.metadata().namespace.as_ref() {
            Some(namespace)
                if options.tool_search && matches!(schema, ToolSchema::Function { .. }) =>
            {
                deferred.push(ClientToolSearchEntry::new(
                    namespace.name.clone(),
                    namespace.description.clone(),
                    schema,
                ));
            }
            _ => eager.push(schema),
        }
    }

    let mut catalog = None;
    if !deferred.is_empty() {
        eager.push(client_tool_search_schema());
        catalog = Some(ClientToolSearchCatalog { entries: deferred });
    }
    if options.programmatic_tool_calling && has_programmatic {
        eager.push(ToolSchema::ProgrammaticToolCalling);
    }
    ToolInventory {
        request_schemas: crate::stable_tool_schemas(eager),
        catalog,
    }
}

pub(crate) fn estimate_tool_schema_tokens(schemas: &[ToolSchema]) -> u64 {
    let bytes = serde_json::to_vec(schemas).map_or(0, |value| value.len() as u64);
    bytes.saturating_add(3) / 4
}

pub(crate) fn estimate_tool_result_tokens<'a>(results: impl IntoIterator<Item = &'a str>) -> u64 {
    let bytes = results.into_iter().fold(0_u64, |total, result| {
        total.saturating_add(result.len() as u64)
    });
    bytes.saturating_add(3) / 4
}

fn generic_object_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": true,
    })
}
