use std::collections::BTreeSet;

use pl_protocol::{StudioEventEnvelope, StudioEventKind};
use tokio::sync::broadcast;

/// Studio 事件订阅范围。
///
/// UI 端通过范围控制资源消耗：Flutter 端打开会话时使用 `Session`，
/// 全局状态栏和设置页使用 `Global`；测试和诊断场景可使用 `All`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioEventScope {
    All,
    Session(String),
    Sessions(BTreeSet<String>),
    Global,
}

/// Studio 事件过滤器。
///
/// 过滤器只判断事件应进入哪个 UI stream，不改变事件持久化、sequence 或 payload。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioEventFilter {
    scope: StudioEventScope,
}

impl StudioEventFilter {
    pub fn all() -> Self {
        Self {
            scope: StudioEventScope::All,
        }
    }

    pub fn session(session_id: impl Into<String>) -> Self {
        Self {
            scope: StudioEventScope::Session(session_id.into()),
        }
    }

    pub fn sessions(session_ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            scope: StudioEventScope::Sessions(session_ids.into_iter().collect()),
        }
    }

    pub fn global() -> Self {
        Self {
            scope: StudioEventScope::Global,
        }
    }

    pub fn matches(&self, event: &StudioEventEnvelope) -> bool {
        match &self.scope {
            StudioEventScope::All => true,
            StudioEventScope::Session(session_id) => {
                event.session_id.as_deref() == Some(session_id.as_str())
            }
            StudioEventScope::Sessions(session_ids) => event
                .session_id
                .as_ref()
                .is_some_and(|session_id| session_ids.contains(session_id)),
            StudioEventScope::Global => is_global_event(event),
        }
    }
}

/// 带过滤条件的 Studio 事件接收器。
///
/// 调用方使用 `recv` 异步等待下一条命中过滤条件的事件；底层 broadcast
/// 的 lagged/closed 状态仍按原样返回，便于桥接层触发补拉。
pub struct StudioFilteredEventReceiver {
    rx: broadcast::Receiver<StudioEventEnvelope>,
    filter: StudioEventFilter,
}

impl StudioFilteredEventReceiver {
    pub(crate) fn new(
        rx: broadcast::Receiver<StudioEventEnvelope>,
        filter: StudioEventFilter,
    ) -> Self {
        Self { rx, filter }
    }

    pub async fn recv(
        &mut self,
    ) -> std::result::Result<StudioEventEnvelope, broadcast::error::RecvError> {
        loop {
            let event = self.rx.recv().await?;
            if self.filter.matches(&event) {
                return Ok(event);
            }
        }
    }
}

fn is_global_event(event: &StudioEventEnvelope) -> bool {
    if event.session_id.is_none() {
        return true;
    }
    matches!(
        &event.kind,
        StudioEventKind::SessionListChanged { .. }
            | StudioEventKind::McpHealthChanged { .. }
            | StudioEventKind::LspHealthChanged { .. }
    )
}

#[cfg(test)]
mod tests {
    use pl_protocol::{StudioEventEnvelope, StudioEventKind};

    use super::StudioEventFilter;

    #[test]
    fn session_filter_keeps_only_matching_session_events() {
        let filter = StudioEventFilter::session("session-a");

        assert!(filter.matches(&event(Some("session-a"))));
        assert!(!filter.matches(&event(Some("session-b"))));
        assert!(!filter.matches(&event(None)));
    }

    #[test]
    fn global_filter_keeps_global_events_out_of_session_streams() {
        let filter = StudioEventFilter::global();

        assert!(filter.matches(&event(None)));
        assert!(filter.matches(&session_list_event(Some("session-a"))));
        assert!(!filter.matches(&event(Some("session-a"))));
    }

    fn event(session_id: Option<&str>) -> StudioEventEnvelope {
        StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: session_id.map(str::to_string),
            turn_id: None,
            sequence: 0,
            created_at: 1,
            kind: StudioEventKind::Stale { lagged_events: 1 },
        }
    }

    fn session_list_event(session_id: Option<&str>) -> StudioEventEnvelope {
        StudioEventEnvelope {
            event_id: "event-2".to_string(),
            project_id: None,
            session_id: session_id.map(str::to_string),
            turn_id: None,
            sequence: 0,
            created_at: 1,
            kind: StudioEventKind::SessionListChanged {
                project_id: "project".to_string(),
                sessions: Vec::new(),
            },
        }
    }
}
