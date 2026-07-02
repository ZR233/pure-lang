use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

pub const ZHIPU_CODING_PLAN_BASE_URL: &str = "https://open.bigmodel.cn/api/coding/paas/v4";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderInfo {
    pub provider_kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub default_model: String,
    pub bearer_token: Option<String>,
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub tool_wire_policy: ToolWirePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    OpenAiCompatibleChat,
    #[default]
    DeepSeek,
    Zhipu,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolWirePolicy {
    NativeCustomTools,
    #[default]
    FunctionFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    Freeform,
}

impl ProviderInfo {
    pub fn default_provider() -> Self {
        Self::deepseek(None)
    }

    pub fn openai(base_url: Option<String>) -> Self {
        Self {
            provider_kind: ProviderKind::OpenAi,
            name: "OpenAI".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            default_model: "gpt-5.5".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
        }
    }

    pub fn deepseek(base_url: Option<String>) -> Self {
        Self {
            provider_kind: ProviderKind::DeepSeek,
            name: "DeepSeek".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            default_model: "deepseek-v4-flash".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        }
    }

    pub fn zhipu(base_url: Option<String>) -> Self {
        Self {
            provider_kind: ProviderKind::Zhipu,
            name: "Zhipu".into(),
            base_url: base_url.unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".into()),
            default_model: "glm-5.2".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        }
    }

    pub fn zhipu_coding_plan(base_url: Option<String>) -> Self {
        Self {
            provider_kind: ProviderKind::Zhipu,
            name: "Zhipu Coding Plan".into(),
            base_url: base_url.unwrap_or_else(|| ZHIPU_CODING_PLAN_BASE_URL.into()),
            default_model: "glm-5.2".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        }
    }

    pub fn openai_compatible_chat(
        name: impl Into<String>,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            provider_kind: ProviderKind::OpenAiCompatibleChat,
            name: name.into(),
            base_url: base_url.into(),
            default_model: default_model.into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        }
    }

    pub fn uses_native_custom_tools(&self) -> bool {
        matches!(self.tool_wire_policy, ToolWirePolicy::NativeCustomTools)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn default_provider_is_deepseek() {
        let info = ProviderInfo::default_provider();

        assert_eq!(info.provider_kind, ProviderKind::DeepSeek);
        assert_eq!(info.name, "DeepSeek");
        assert_eq!(info.default_model, "deepseek-v4-flash");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }

    #[test]
    fn openai_uses_responses_profile_defaults() {
        let info = ProviderInfo::openai(None);

        assert_eq!(info.provider_kind, ProviderKind::OpenAi);
        assert_eq!(info.name, "OpenAI");
        assert_eq!(info.base_url, "https://api.openai.com/v1");
        assert_eq!(info.default_model, "gpt-5.5");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::NativeCustomTools);
    }

    #[test]
    fn zhipu_uses_single_provider_kind() {
        let info = ProviderInfo::zhipu(None);

        assert_eq!(info.provider_kind, ProviderKind::Zhipu);
        assert_eq!(info.base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(info.default_model, "glm-5.2");
    }

    #[test]
    fn zhipu_coding_plan_uses_zhipu_runtime_with_coding_endpoint() {
        let info = ProviderInfo::zhipu_coding_plan(None);

        assert_eq!(info.provider_kind, ProviderKind::Zhipu);
        assert_eq!(info.name, "Zhipu Coding Plan");
        assert_eq!(info.base_url, ZHIPU_CODING_PLAN_BASE_URL);
        assert_eq!(info.default_model, "glm-5.2");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }

    #[test]
    fn openai_compatible_chat_provider_can_express_mimo() {
        let info = ProviderInfo::openai_compatible_chat(
            "MiMo",
            "https://mimo.example.com/v1",
            "mimo-chat",
        );

        assert_eq!(info.provider_kind, ProviderKind::OpenAiCompatibleChat);
        assert_eq!(info.name, "MiMo");
        assert_eq!(info.base_url, "https://mimo.example.com/v1");
        assert_eq!(info.default_model, "mimo-chat");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }
}
