/// 返回由 PL 维护的 canonical session event TypeScript 声明。
///
/// 产品只能机械复制该内容，不应在自己的前端工程中维护第二份事件类型。
pub fn session_events_typescript() -> &'static str {
    include_str!("typescript/session-events.ts")
}

#[cfg(test)]
mod tests {
    use super::session_events_typescript;

    #[test]
    fn exported_types_are_canonical_and_track_v3_wire() {
        let types = session_events_typescript();
        assert!(types.starts_with("// Canonical TypeScript declarations for the pl-protocol"));
        assert!(types.contains("export type SessionStreamFrame"));
        assert!(types.contains("export interface SessionViewSnapshot"));
        assert!(types.contains("state: SessionTurnState"));
        assert!(types.contains("\"reasoning.content\""));
    }
}
