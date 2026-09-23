//! Published workspace cleanup remains owned until the frozen close policy succeeds.
use super::{StudioThreadAssembler, ThreadAssemblyError};
use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
use std::sync::Arc;

impl StudioThreadAssembler {
    pub(super) fn seal_agent_close(
        &self,
        targets: &[(usize, String)],
        disposition: AgentWorkspaceDisposition,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        for (_, id) in targets {
            if let Some(entry) = state.entries.get(id)
                && entry
                    .workspace_disposition
                    .is_some_and(|current| current != disposition)
            {
                return Err(ThreadAssemblyError::CloseDisposition(id.clone()));
            }
        }
        for (_, id) in targets {
            if let Some(entry) = state.entries.get_mut(id) {
                entry.workspace_disposition = Some(disposition);
                entry.ready = false;
            }
        }
        Ok(())
    }

    pub(super) async fn close_published_resources(
        &self,
        id: &str,
        incarnation: &Arc<()>,
    ) -> Result<(), ThreadAssemblyError> {
        let cleanup = {
            let state = self.0.state();
            if state.unpublished_children.contains_key(id) {
                return Ok(());
            }
            state
                .entries
                .get(id)
                .filter(|entry| {
                    entry.parent_id.is_some() && Arc::ptr_eq(&entry.incarnation, incarnation)
                })
                .map(|entry| entry.cleanup.clone())
        };
        let Some(cleanup) = cleanup else {
            return Ok(());
        };
        let _permit = cleanup.acquire().await;
        let resource = {
            let state = self.0.state();
            state
                .entries
                .get(id)
                .filter(|entry| {
                    !entry.published_resources_closed
                        && Arc::ptr_eq(&entry.incarnation, incarnation)
                })
                .and_then(|entry| {
                    state
                        .child_factory
                        .clone()
                        .map(|factory| (factory, entry.workspace_disposition.unwrap_or_default()))
                })
        };
        if let Some((factory, disposition)) = resource {
            factory.close_published(id, disposition).await?;
            if let Some(entry) = self.0.state().entries.get_mut(id)
                && Arc::ptr_eq(&entry.incarnation, incarnation)
            {
                entry.published_resources_closed = true;
            }
        }
        Ok(())
    }
}
