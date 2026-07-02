#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelContinuationState {
    previous_response_id: Option<String>,
    prompt_cache_key: Option<String>,
    acknowledged_message_count: usize,
    disabled: bool,
}

impl ModelContinuationState {
    pub fn set_prompt_cache_key(&mut self, key: String) {
        self.prompt_cache_key = Some(key);
    }

    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.prompt_cache_key.as_deref()
    }

    pub fn previous_response_id(&self) -> Option<&str> {
        self.previous_response_id.as_deref()
    }

    pub fn acknowledged_message_count(&self) -> usize {
        self.acknowledged_message_count
    }

    pub fn continuation_start_index(&self) -> Option<usize> {
        if self.disabled {
            return None;
        }
        self.previous_response_id
            .as_ref()
            .map(|_| self.acknowledged_message_count)
    }

    pub fn acknowledged_tail<'a, T>(&self, items: &'a [T], use_continuation: bool) -> &'a [T] {
        if !use_continuation {
            return items;
        }
        self.continuation_start_index()
            .and_then(|start| items.get(start..))
            .unwrap_or(items)
    }

    pub fn disabled(&self) -> bool {
        self.disabled
    }

    pub fn acknowledge_response(
        &mut self,
        acknowledged_message_count: usize,
        response_id: Option<String>,
        current_message_count: usize,
    ) {
        if let Some(response_id) = response_id.filter(|id| !id.trim().is_empty()) {
            self.previous_response_id = Some(response_id);
            self.acknowledged_message_count = acknowledged_message_count.min(current_message_count);
            self.disabled = false;
        } else {
            self.reset();
        }
    }

    pub fn acknowledge_message_count(
        &mut self,
        acknowledged_message_count: usize,
        current_message_count: usize,
    ) {
        self.acknowledged_message_count = acknowledged_message_count.min(current_message_count);
    }

    pub fn mark_unsupported(&mut self) {
        self.previous_response_id = None;
        self.acknowledged_message_count = 0;
        self.disabled = true;
    }

    pub fn reset(&mut self) {
        let prompt_cache_key = self.prompt_cache_key.clone();
        *self = Self {
            prompt_cache_key,
            ..Self::default()
        };
    }

    pub fn reset_if_acknowledged_messages_were_removed(&mut self, current_message_count: usize) {
        if self.acknowledged_message_count > current_message_count {
            self.reset();
        }
    }
}

pub fn is_continuation_unsupported_error(error: &pl_protocol::PureError) -> bool {
    let message = error.to_string();
    message.contains("previous_response_id")
        && (message.contains("not supported") || message.contains("only supported"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_response_prompt_cache_and_truncation() {
        let mut state = ModelContinuationState::default();
        state.set_prompt_cache_key("cache-1".to_string());
        state.acknowledge_response(2, Some("resp-1".to_string()), 3);
        state.acknowledge_message_count(3, 3);

        assert_eq!(state.prompt_cache_key(), Some("cache-1"));
        assert_eq!(state.previous_response_id(), Some("resp-1"));
        assert_eq!(state.acknowledged_message_count(), 3);
        assert_eq!(state.continuation_start_index(), Some(3));

        state.reset_if_acknowledged_messages_were_removed(1);

        assert_eq!(state.prompt_cache_key(), Some("cache-1"));
        assert_eq!(state.previous_response_id(), None);
        assert_eq!(state.acknowledged_message_count(), 0);
        assert_eq!(state.continuation_start_index(), None);
    }

    #[test]
    fn unsupported_continuation_disables_incremental_start() {
        let mut state = ModelContinuationState::default();
        state.set_prompt_cache_key("cache-1".to_string());
        state.acknowledge_response(1, Some("resp-1".to_string()), 1);

        state.mark_unsupported();

        assert!(state.disabled());
        assert_eq!(state.prompt_cache_key(), Some("cache-1"));
        assert_eq!(state.previous_response_id(), None);
        assert_eq!(state.continuation_start_index(), None);
    }

    #[test]
    fn acknowledged_tail_uses_previous_response_prefix_only_when_enabled() {
        let items = ["system", "user-1", "assistant-1", "user-2"];
        let mut state = ModelContinuationState::default();
        state.acknowledge_response(2, Some("resp-1".to_string()), items.len());

        assert_eq!(state.acknowledged_tail(&items, true), &items[2..]);
        assert_eq!(state.acknowledged_tail(&items, false), &items);

        state.mark_unsupported();

        assert_eq!(state.acknowledged_tail(&items, true), &items);
    }

    #[test]
    fn recognizes_previous_response_id_unsupported_errors() {
        let error = pl_protocol::PureError::LlmError(
            "previous_response_id is only supported on Responses WebSocket v2".to_string(),
        );

        assert!(is_continuation_unsupported_error(&error));
        assert!(!is_continuation_unsupported_error(
            &pl_protocol::PureError::LlmError("rate limit".to_string())
        ));
    }
}
