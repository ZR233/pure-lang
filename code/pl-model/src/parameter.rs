//! 模型可调参数声明与 wire 透传机制。
//!
//! 参见 design/07-model.md 7.8 节。effort 等可调参数由模型声明：候选值域
//! 加每个候选值对应的 wire 写入规则。协议层据 wire 把用户选中的字符串值
//! 透传写入 API 请求体，不包含任何 provider 特定代码。

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

/// 模型声明的可调参数。
///
/// 描述「该模型有一个名为 [`name`](Self::name) 的可调参数，候选值域为
/// [`candidates`](Self::candidates)，每个候选值对应一组 wire 写入规则」。
/// effort 是首个应用（`name = "effort"`），设计上可容纳未来 thinking、
/// verbosity 等参数。
///
/// GUI 据此渲染候选值下拉；协议层据 [`wire`](Self::wire) 把选中值写入请求体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelParameter {
    /// 参数键，同一模型内唯一，如 "effort"。
    pub name: String,
    /// 面向用户的显示名，缺失时回退到 name。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 候选值域（GUI 渲染为下拉选项；首项为默认值）。
    pub candidates: Vec<String>,
    /// 每个候选值对应的 wire 写入规则，key 必须是 candidates 中的值。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub wire: BTreeMap<String, ParameterWire>,
}

impl ModelParameter {
    /// 返回选中候选值对应的 wire 规则；选中值未声明 wire 时返回 None。
    pub fn wire_for(&self, selected: &str) -> Option<&ParameterWire> {
        self.wire.get(selected)
    }

    /// 判断候选值是否在该参数声明的值域中。
    pub fn has_candidate(&self, candidate: &str) -> bool {
        self.candidates.iter().any(|item| item == candidate)
    }

    /// 返回参数的默认候选值。
    ///
    /// 若调用方传入的默认值存在且属于候选值域，优先返回该值；否则回退到
    /// candidates 首项。这样宿主产品可以保留配置文件里的显式默认值，同时仍由
    /// pl-model 统一维护回退规则。
    pub fn default_candidate(&self, configured_default: Option<&str>) -> Option<String> {
        configured_default
            .filter(|candidate| self.has_candidate(candidate))
            .map(ToString::to_string)
            .or_else(|| self.candidates.first().cloned())
    }

    /// 解析一次模型参数选择，统一处理显式选择、缺省默认值和禁用值。
    pub fn resolve_candidate(
        &self,
        request: ModelParameterCandidateRequest<'_>,
    ) -> Result<Option<String>, ModelParameterCandidateError> {
        if let Some(candidate) = request.requested {
            if candidate.trim().is_empty()
                || request
                    .disabled_values
                    .iter()
                    .any(|disabled| *disabled == candidate)
            {
                return Ok(None);
            }
            if self.has_candidate(candidate) {
                return Ok(Some(candidate.to_string()));
            }
            return Err(ModelParameterCandidateError::UnsupportedCandidate {
                parameter: self.name.clone(),
                candidate: candidate.to_string(),
            });
        }

        match request.missing {
            MissingCandidatePolicy::UseDefault => {
                Ok(self.default_candidate(request.default_candidate))
            }
            MissingCandidatePolicy::Omit => Ok(None),
        }
    }
}

/// 未显式选择参数值时的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingCandidatePolicy {
    /// 使用参数默认候选值。
    UseDefault,
    /// 不写入该参数。
    Omit,
}

/// 一次模型参数候选值解析请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelParameterCandidateRequest<'a> {
    /// 用户或上层策略显式指定的候选值。
    pub requested: Option<&'a str>,
    /// 配置中声明的默认候选值；若不存在或无效则回退到候选值首项。
    pub default_candidate: Option<&'a str>,
    /// 没有显式选择时采用的策略。
    pub missing: MissingCandidatePolicy,
    /// 表示禁用/清空该参数的特殊输入值。
    pub disabled_values: &'a [&'a str],
}

impl Default for ModelParameterCandidateRequest<'_> {
    fn default() -> Self {
        Self {
            requested: None,
            default_candidate: None,
            missing: MissingCandidatePolicy::Omit,
            disabled_values: &[],
        }
    }
}

/// 模型参数候选值解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelParameterCandidateError {
    /// 显式候选值不属于参数声明的值域。
    UnsupportedCandidate {
        parameter: String,
        candidate: String,
    },
}

impl fmt::Display for ModelParameterCandidateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCandidate {
                parameter,
                candidate,
            } => {
                write!(
                    formatter,
                    "candidate `{candidate}` is not supported by parameter `{parameter}`"
                )
            }
        }
    }
}

impl std::error::Error for ModelParameterCandidateError {}

/// 选中某候选值时对请求体的修改动作。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterWire {
    /// 把指定字段路径设置为值，dot 路径如 "reasoning.effort"。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<WireAssignment>,
    /// 从请求体移除指定字段路径（dot 路径）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
}

impl ParameterWire {
    /// 把本 wire 规则应用到请求体：先执行所有 set，再执行所有 remove。
    pub fn apply_to(&self, body: &mut Map<String, Value>) {
        for assignment in &self.set {
            set_nested(body, &assignment.path, assignment.value.clone());
        }
        for path in &self.remove {
            remove_nested(body, path);
        }
    }
}

/// 单个字段赋值：dot 路径 → 值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireAssignment {
    /// dot 分隔的嵌套路径，如 "reasoning.effort"、"thinking.type"。
    pub path: String,
    /// 写入该字段的值（字符串、布尔或数字），透传给 API。
    pub value: Value,
}

/// 将 JSON 请求片段拍平成 wire 赋值列表。
///
/// object 会递归展开为 dot 路径，null 会被忽略，其它 JSON 值按叶子节点原样
/// 保留。宿主产品可用它把配置中的 provider/model 请求片段转换为
/// [`ParameterWire::set`]，避免重复实现路径拍平语义。
pub fn wire_assignments_from_value(value: &Value) -> Vec<WireAssignment> {
    let mut assignments = Vec::new();
    collect_wire_assignments(None, value, &mut assignments);
    assignments
}

fn collect_wire_assignments(
    prefix: Option<&str>,
    value: &Value,
    assignments: &mut Vec<WireAssignment>,
) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let path = prefix
                    .map(|prefix| format!("{prefix}.{key}"))
                    .unwrap_or_else(|| key.clone());
                collect_wire_assignments(Some(&path), value, assignments);
            }
        }
        Value::Null => {}
        Value::Array(_) | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            if let Some(path) = prefix {
                assignments.push(WireAssignment {
                    path: path.to_string(),
                    value: value.clone(),
                });
            }
        }
    }
}

/// 按 dot 路径写入嵌套 JSON 对象。
///
/// 中间节点不存在则创建 Object；中间节点存在但非 Object 则整体替换为 Object。
/// 空路径段或空路径静默忽略。
fn set_nested(body: &mut Map<String, Value>, path: &str, value: Value) {
    let mut segments: Vec<&str> = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    let Some(leaf) = segments.pop() else {
        return;
    };
    let mut current = body;
    for segment in segments {
        let entry = current
            .entry(segment.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().expect("ensured object above");
    }
    current.insert(leaf.to_string(), value);
}

/// 按 dot 路径移除嵌套 JSON 对象的叶子字段。
///
/// 中间路径不存在或非 Object 时静默忽略。
fn remove_nested(body: &mut Map<String, Value>, path: &str) {
    let mut segments: Vec<&str> = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    let Some(leaf) = segments.pop() else {
        return;
    };
    let mut current = body;
    for segment in &segments {
        let Some(Value::Object(inner)) = current.get_mut(*segment) else {
            return;
        };
        current = inner;
    }
    current.remove(leaf);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn body_value(body: Map<String, Value>) -> Value {
        Value::Object(body)
    }

    #[test]
    fn set_nested_writes_top_level_field() {
        let mut body = Map::new();
        set_nested(&mut body, "reasoning_effort", json!("high"));

        assert_eq!(body_value(body), json!({"reasoning_effort": "high"}));
    }

    #[test]
    fn set_nested_creates_intermediate_objects() {
        let mut body = Map::new();
        set_nested(&mut body, "reasoning.effort", json!("high"));

        assert_eq!(body_value(body), json!({"reasoning": {"effort": "high"}}));
    }

    #[test]
    fn set_nested_writes_three_level_path() {
        let mut body = Map::new();
        set_nested(&mut body, "a.b.c", json!("v"));

        assert_eq!(body_value(body), json!({"a": {"b": {"c": "v"}}}));
    }

    #[test]
    fn set_nested_overwrites_non_object_intermediate() {
        let mut body = Map::new();
        body.insert("thinking".to_string(), json!("placeholder"));

        set_nested(&mut body, "thinking.type", json!("enabled"));

        assert_eq!(body_value(body), json!({"thinking": {"type": "enabled"}}));
    }

    #[test]
    fn set_nested_writes_boolean_value() {
        // Zhipu clear_thinking = false 是布尔值，必须原样透传
        let mut body = Map::new();
        set_nested(&mut body, "thinking.clear_thinking", json!(false));

        assert_eq!(
            body_value(body),
            json!({"thinking": {"clear_thinking": false}})
        );
    }

    #[test]
    fn set_nested_ignores_empty_segments() {
        let mut body = Map::new();
        set_nested(&mut body, ".thinking.type.", json!("enabled"));
        set_nested(&mut body, "", json!("ignored"));

        assert_eq!(body_value(body), json!({"thinking": {"type": "enabled"}}));
    }

    #[test]
    fn remove_nested_deletes_leaf_field() {
        let mut body: Map<String, Value> =
            serde_json::from_str(r#"{"reasoning_effort":"high","thinking":{"type":"enabled"}}"#)
                .unwrap();

        remove_nested(&mut body, "reasoning_effort");
        assert!(!body.contains_key("reasoning_effort"));

        remove_nested(&mut body, "thinking.type");
        assert_eq!(body_value(body), json!({"thinking": {}}));
    }

    #[test]
    fn remove_nested_silent_on_missing_path() {
        let mut body = Map::new();
        remove_nested(&mut body, "nonexistent.path");
        remove_nested(&mut body, "thinking.type");
        // 不 panic 即通过
    }

    #[test]
    fn parameter_wire_apply_handles_glm52_none() {
        // GLM-5.2 选 none：set thinking.type=disabled，remove reasoning_effort
        let wire = ParameterWire {
            set: vec![WireAssignment {
                path: "thinking.type".to_string(),
                value: json!("disabled"),
            }],
            remove: vec!["reasoning_effort".to_string()],
        };
        let mut body: Map<String, Value> =
            serde_json::from_str(r#"{"reasoning_effort":"high","thinking":{"type":"enabled"}}"#)
                .unwrap();

        wire.apply_to(&mut body);

        assert_eq!(body_value(body), json!({"thinking": {"type": "disabled"}}));
    }

    #[test]
    fn parameter_wire_apply_set_runs_before_remove() {
        // set 同一字段再 remove：最终字段被移除（remove 后执行）
        let wire = ParameterWire {
            set: vec![WireAssignment {
                path: "reasoning_effort".to_string(),
                value: json!("high"),
            }],
            remove: vec!["reasoning_effort".to_string()],
        };
        let mut body = Map::new();

        wire.apply_to(&mut body);

        assert_eq!(body, Map::new());
    }

    #[test]
    fn wire_assignments_from_value_flattens_scalar_leaves() {
        let request = json!({
            "reasoning_effort": "high",
            "thinking": {
                "type": "enabled",
                "clear_thinking": false,
                "ignored": null
            },
            "modalities": ["text"]
        });

        assert_eq!(
            wire_assignments_from_value(&request),
            vec![
                WireAssignment {
                    path: "modalities".to_string(),
                    value: json!(["text"]),
                },
                WireAssignment {
                    path: "reasoning_effort".to_string(),
                    value: json!("high"),
                },
                WireAssignment {
                    path: "thinking.clear_thinking".to_string(),
                    value: json!(false),
                },
                WireAssignment {
                    path: "thinking.type".to_string(),
                    value: json!("enabled"),
                },
            ]
        );
    }

    #[test]
    fn wire_for_returns_wire_for_known_candidate() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "max".to_string()],
            wire: [(
                "high".to_string(),
                ParameterWire {
                    set: vec![WireAssignment {
                        path: "reasoning.effort".to_string(),
                        value: json!("high"),
                    }],
                    remove: Vec::new(),
                },
            )]
            .into_iter()
            .collect(),
        };

        let wire = param.wire_for("high").unwrap();
        assert_eq!(wire.set.len(), 1);
        assert_eq!(wire.set[0].path, "reasoning.effort");
        assert_eq!(wire.set[0].value, json!("high"));

        assert!(param.wire_for("unknown").is_none());
    }

    fn candidate_request<'a>(
        requested: Option<&'a str>,
        default_candidate: Option<&'a str>,
        missing: MissingCandidatePolicy,
    ) -> ModelParameterCandidateRequest<'a> {
        ModelParameterCandidateRequest {
            requested,
            default_candidate,
            missing,
            disabled_values: &["none"],
        }
    }

    #[test]
    fn resolve_candidate_accepts_requested_candidate() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "max".to_string()],
            wire: BTreeMap::new(),
        };

        assert_eq!(
            param.resolve_candidate(candidate_request(
                Some("max"),
                Some("high"),
                MissingCandidatePolicy::UseDefault
            )),
            Ok(Some("max".to_string()))
        );
    }

    #[test]
    fn resolve_candidate_uses_valid_default_candidate() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "medium".to_string()],
            wire: BTreeMap::new(),
        };

        assert_eq!(
            param.resolve_candidate(candidate_request(
                None,
                Some("medium"),
                MissingCandidatePolicy::UseDefault
            )),
            Ok(Some("medium".to_string()))
        );
    }

    #[test]
    fn resolve_candidate_falls_back_to_first_candidate_when_default_is_unknown() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "medium".to_string()],
            wire: BTreeMap::new(),
        };

        assert_eq!(
            param.resolve_candidate(candidate_request(
                None,
                Some("low"),
                MissingCandidatePolicy::UseDefault
            )),
            Ok(Some("high".to_string()))
        );
    }

    #[test]
    fn resolve_candidate_returns_none_for_disabled_or_blank_values() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "none".to_string()],
            wire: BTreeMap::new(),
        };

        assert_eq!(
            param.resolve_candidate(candidate_request(
                Some("none"),
                Some("high"),
                MissingCandidatePolicy::UseDefault
            )),
            Ok(None)
        );
        assert_eq!(
            param.resolve_candidate(candidate_request(
                Some("  "),
                Some("high"),
                MissingCandidatePolicy::UseDefault
            )),
            Ok(None)
        );
    }

    #[test]
    fn resolve_candidate_rejects_unknown_requested_candidate() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["high".to_string(), "max".to_string()],
            wire: BTreeMap::new(),
        };

        assert_eq!(
            param.resolve_candidate(candidate_request(
                Some("low"),
                Some("high"),
                MissingCandidatePolicy::UseDefault
            )),
            Err(ModelParameterCandidateError::UnsupportedCandidate {
                parameter: "effort".to_string(),
                candidate: "low".to_string(),
            })
        );
    }

    #[test]
    fn parameter_roundtrips_through_serde() {
        let param = ModelParameter {
            name: "effort".to_string(),
            label: Some("推理强度".to_string()),
            candidates: vec!["high".to_string(), "none".to_string()],
            wire: [
                (
                    "high".to_string(),
                    ParameterWire {
                        set: vec![
                            WireAssignment {
                                path: "thinking.type".to_string(),
                                value: json!("enabled"),
                            },
                            WireAssignment {
                                path: "thinking.clear_thinking".to_string(),
                                value: json!(false),
                            },
                        ],
                        remove: Vec::new(),
                    },
                ),
                (
                    "none".to_string(),
                    ParameterWire {
                        set: vec![WireAssignment {
                            path: "thinking.type".to_string(),
                            value: json!("disabled"),
                        }],
                        remove: vec!["reasoning_effort".to_string()],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };

        let json_value = serde_json::to_value(&param).unwrap();
        assert!(json_value.get("wire").is_some());
        let back: ModelParameter = serde_json::from_value(json_value).unwrap();
        assert_eq!(back, param);
    }
}
