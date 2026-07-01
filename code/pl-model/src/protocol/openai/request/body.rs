use pl_protocol::Result;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::model_info::ModelInfo;
use crate::request::{ReasoningConfig, ToolFormat};

use super::protocol_error;
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolFormatBody {
    Text,
    Grammar { syntax: String, definition: String },
}

impl ToolFormatBody {
    pub(super) fn from_format(format: &ToolFormat) -> Self {
        match format {
            ToolFormat::Text => Self::Text,
            ToolFormat::Grammar { syntax, definition } => Self::Grammar {
                syntax: syntax.clone(),
                definition: definition.clone(),
            },
        }
    }
}

/// 把强类型请求序列化为 JSON 对象 Map，供 base body 与 parameter wire 注入。
pub(super) fn to_object_map<T: Serialize>(value: &T) -> Result<Map<String, Value>> {
    let serialized = serde_json::to_value(value)?;
    match serialized {
        Value::Object(map) => Ok(map),
        _ => Err(protocol_error(
            "typed request body must serialize to a JSON object",
        )),
    }
}

/// 注入 base body 后应用 parameter wire，完成请求体动态字段组装。
pub(super) fn finalize_body(
    body: &mut Map<String, Value>,
    model: &ModelInfo,
    reasoning: &Option<ReasoningConfig>,
) {
    merge_base_body(body, &model.request_profile.body);
    apply_parameters(body, model, reasoning);
}

/// 深合并 base body 到请求体：对象字段递归合并，其余字段覆盖。
fn merge_base_body(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(target_inner)), Value::Object(source_inner)) => {
                merge_base_body(target_inner, source_inner);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

/// 遍历模型声明的可调参数，对用户选中的值应用 wire 写入请求体。
fn apply_parameters(
    body: &mut Map<String, Value>,
    model: &ModelInfo,
    reasoning: &Option<ReasoningConfig>,
) {
    for parameter in &model.parameters {
        let selected = if parameter.name == "effort" {
            reasoning
                .as_ref()
                .and_then(|config| config.effort.as_deref())
        } else {
            None
        };
        if let Some(value) = selected
            && let Some(wire) = parameter.wire_for(value)
        {
            wire.apply_to(body);
        }
    }
}
