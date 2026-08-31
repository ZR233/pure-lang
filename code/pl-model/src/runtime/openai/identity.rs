//! OpenAI Responses 工具身份的唯一归一化规则。

/// 把 envelope 提取出的 Responses item/call identity 归一化为必填值。
///
/// late `call_id` 到达前以 `item_id` 确定性填充；两者都缺失时保留空值，
/// 由 canonical accumulator 以协议错误拒绝。
pub(super) fn responses_tool_identity(
    item_id: Option<&str>,
    call_id: Option<&str>,
    kind: &str,
) -> (String, String) {
    let item_id = item_id
        .filter(|item_id| !item_id.is_empty())
        .or_else(|| call_id.filter(|call_id| !call_id.is_empty()))
        .unwrap_or_default()
        .to_string();
    let call_id = call_id
        .filter(|call_id| !call_id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            tracing::trace!(
                item_id = %item_id,
                kind,
                "responses tool item missing call_id; assigning item id"
            );
            item_id.clone()
        });
    (item_id, call_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_and_call_aliases_have_one_deterministic_rule() {
        assert_eq!(
            responses_tool_identity(Some("item-1"), Some("call-1"), "function_call"),
            ("item-1".to_string(), "call-1".to_string())
        );
        assert_eq!(
            responses_tool_identity(None, Some("call-1"), "function_call"),
            ("call-1".to_string(), "call-1".to_string())
        );
        assert_eq!(
            responses_tool_identity(Some("item-1"), None, "function_call"),
            ("item-1".to_string(), "item-1".to_string())
        );
    }
}
