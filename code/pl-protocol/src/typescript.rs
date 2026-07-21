/// 返回由 PL 维护的 canonical session event TypeScript 声明。
///
/// 产品只能机械复制该内容，不应在自己的前端工程中维护第二份事件类型。
pub fn session_events_typescript() -> &'static str {
    include_str!("typescript/session-events.generated.ts")
}

#[cfg(test)]
mod tests {
    use super::session_events_typescript;

    #[test]
    fn exported_types_are_marked_read_only() {
        let types = session_events_typescript();
        assert!(types.starts_with("// @generated from pl-protocol."));
        assert!(types.contains("export type SessionStreamFrame"));
        assert!(types.contains("export interface SessionViewSnapshot"));
    }
}
