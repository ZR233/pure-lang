//! Product activation follows saved ancestry and shares the single Thread assembler.
use super::super::StudioRuntime;
use crate::thread_assembler::ThreadActivation;
use anyhow::{Context, Result};

impl StudioRuntime {
    pub(in crate::studio) async fn ensure_thread_owner(
        &self,
        id: &str,
    ) -> Result<pl_core::thread::ThreadHandle> {
        let _activation_pin = self.pin_thread(id);
        if let Some(thread) = self.threads.thread(id) {
            self.residency.touch(id).await;
            self.enforce_residency_limit().await;
            return Ok(thread);
        }
        let mut ancestry = Vec::new();
        let mut directory = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut cursor = id.to_owned();
        loop {
            anyhow::ensure!(seen.insert(cursor.clone()), "cyclic Thread ancestry");
            if self.threads.thread(&cursor).is_some() {
                break;
            }
            let record = self.read_owned_thread(&cursor).await?;
            anyhow::ensure!(
                record.visibility == crate::studio::ThreadVisibility::Active,
                "archived Thread cannot be activated for execution"
            );
            directory.push(pl_protocol::Thread::from(record.clone()));
            ancestry.push(ThreadActivation {
                id: record.id,
                parent_id: record.parent_thread_id.clone(),
            });
            let Some(parent) = record.parent_thread_id else {
                break;
            };
            cursor = parent;
        }
        // Observers and tool refresh can run as soon as activation publishes an owner.
        // Warm the validated ancestry first, preserving any newer in-memory entries.
        self.agent_facility
            .product_events
            .warm_thread_index(directory);
        for identity in ancestry.into_iter().rev() {
            let activated_id = identity.id.clone();
            self.threads
                .activate(identity, self.thread_factory.clone())
                .await?;
            self.residency.touch(&activated_id).await;
        }
        self.residency.touch(id).await;
        self.enforce_residency_limit().await;
        self.threads
            .thread(id)
            .context("Thread activation did not publish its owner")
    }
}
