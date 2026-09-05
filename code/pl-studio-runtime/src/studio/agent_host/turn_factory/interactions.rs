//! 把交互事件发射为 durable thread facts 的 emitter。

use std::sync::Arc;

use futures::FutureExt;

pub(super) fn interaction_emitter(
    runtime: pl_core::AgentRuntimeHandle,
    thread_id: String,
    agent_path: String,
) -> crate::studio::InteractionEmitter {
    Arc::new(move |interaction| {
        let runtime = runtime.clone();
        let thread_id = thread_id.clone();
        let agent_path = agent_path.clone();
        async move {
            let emitted_at = interaction.updated_at;
            runtime
                .record_thread_facts(
                    pl_core::ThreadId::new(agent_path.clone())?,
                    pl_core::ThreadId::new(thread_id)?,
                    vec![pl_core::ThreadNotificationFact::durable(
                        emitted_at,
                        pl_protocol::ThreadNotification::InteractionChanged {
                            interaction: Box::new(interaction),
                        },
                    )],
                )
                .await?;
            Ok(())
        }
        .boxed()
    })
}
