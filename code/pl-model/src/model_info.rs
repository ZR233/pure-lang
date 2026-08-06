use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::capabilities::ModelCapabilities;
use crate::parameter::ModelParameter;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,

    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,

    pub default_temperature: Option<f32>,
    pub max_output_tokens: Option<u64>,
    pub currency: Option<String>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub cache_read_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ModelParameter>,

    #[serde(default)]
    pub capabilities: ModelCapabilities,
    #[serde(default)]
    pub request_profile: ModelRequestProfile,

    #[serde(default)]
    pub truncation_policy: TruncationPolicy,

    #[serde(default)]
    pub base_instructions: String,

    #[serde(skip)]
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRequestProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_model: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub body: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub options: Map<String, Value>,
    #[serde(default, skip_serializing_if = "MaxTokensField::is_default")]
    pub max_tokens_field: MaxTokensField,
    #[serde(default, skip_serializing_if = "ResponsesMaxTokensField::is_default")]
    pub responses_max_tokens_field: ResponsesMaxTokensField,
}

impl ModelRequestProfile {
    /// 所有字段均未设置时返回 true（用于字段级合并判断「用户未提供」）。
    pub fn is_empty(&self) -> bool {
        self.api_model.is_none()
            && self.headers.is_empty()
            && self.body.is_empty()
            && self.options.is_empty()
            && self.max_tokens_field.is_default()
            && self.responses_max_tokens_field.is_default()
    }

    /// 将 JSON object 合并进请求 body；非 object 值会被忽略。
    ///
    /// 后合入的字段覆盖先前同名字段。该方法用于把配置来源中的额外请求体片段
    /// 投影到统一模型请求 profile，避免宿主重复维护 object 合并语义。
    pub fn extend_body_from_value(&mut self, value: &Value) {
        if let Some(object) = value.as_object() {
            self.body.extend(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

impl MaxTokensField {
    pub fn is_default(&self) -> bool {
        *self == Self::MaxTokens
    }
}

/// Controls how a Responses request serializes [`CompletionRequest::max_tokens`](crate::request::CompletionRequest::max_tokens).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesMaxTokensField {
    #[default]
    Omit,
    MaxOutputTokens,
    MaxTokens,
    MaxCompletionTokens,
}

impl ResponsesMaxTokensField {
    pub fn is_default(&self) -> bool {
        *self == Self::Omit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruncationPolicy {
    pub mode: TruncationMode,
    pub limit: u64,
}

impl Default for TruncationPolicy {
    fn default() -> Self {
        Self {
            mode: TruncationMode::Bytes,
            limit: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TruncationMode {
    Bytes,
    Tokens,
}

impl ModelInfo {
    pub fn resolved_context_window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    pub fn resolved_auto_compact_limit(&self) -> Option<u64> {
        let context = self.resolved_context_window()?;
        let default_limit = (context * 90) / 100;
        Some(
            self.auto_compact_token_limit
                .map_or(default_limit, |limit| limit.min(default_limit)),
        )
    }

    pub fn fallback(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            description: None,
            context_window: Some(128_000),
            max_context_window: Some(128_000),
            auto_compact_token_limit: None,
            default_temperature: Some(0.3),
            max_output_tokens: Some(4096),
            currency: None,
            input_price_per_mtok: None,
            output_price_per_mtok: None,
            cache_read_price_per_mtok: None,
            cache_write_price_per_mtok: None,
            parameters: Vec::new(),
            capabilities: ModelCapabilities::text_only(),
            request_profile: ModelRequestProfile::default(),
            truncation_policy: TruncationPolicy {
                mode: TruncationMode::Bytes,
                limit: 10_000,
            },
            base_instructions: String::new(),
            used_fallback: true,
        }
    }

    /// 返回名为 "effort" 的参数声明，若模型未声明则返回 None。
    pub fn effort_parameter(&self) -> Option<&ModelParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == "effort")
    }

    /// 返回 effort 参数的候选值字符串列表（GUI 下拉渲染用）。
    /// 若模型未声明 effort 参数，返回空 Vec。
    pub fn supported_efforts(&self) -> Vec<String> {
        self.effort_parameter()
            .map(|parameter| parameter.candidates.clone())
            .unwrap_or_default()
    }

    /// 返回 effort 的默认值（候选值首项），若模型未声明 effort 返回 None。
    pub fn default_effort(&self) -> Option<String> {
        self.effort_parameter()
            .and_then(|parameter| parameter.candidates.first().cloned())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    #[test]
    fn request_profile_extends_body_from_json_object_values() {
        let mut profile = ModelRequestProfile::default();

        profile.extend_body_from_value(&json!({
            "reasoning": { "effort": "high" },
            "temperature": 0.2
        }));
        profile.extend_body_from_value(&json!({
            "temperature": 0.4,
            "top_p": 0.9
        }));
        profile.extend_body_from_value(&json!("ignored"));

        assert_eq!(
            profile.body,
            json!({
                "reasoning": { "effort": "high" },
                "temperature": 0.4,
                "top_p": 0.9
            })
            .as_object()
            .expect("object")
            .clone()
        );
    }
}
