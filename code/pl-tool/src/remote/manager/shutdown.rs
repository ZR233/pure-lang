//! Manager-wide connection admission and ordered transport shutdown.

use std::sync::Arc;

use tokio_util::task::task_tracker::TaskTrackerToken;

use super::{RemoteClientError, SshConnectionState, SshManager};

impl SshManager {
    pub(super) async fn connection_lock(&self, server_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.connection_locks
            .lock()
            .await
            .entry(server_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    // Caller owns server serialization, or has sealed and drained all operations.
    pub(super) async fn close_connection(&self, server_id: &str) -> Result<(), RemoteClientError> {
        let connection = self.connections.lock().await.get(server_id).cloned();
        if let Some(connection) = connection {
            connection.client.close().await?;
            connection
                .process
                .lock()
                .await
                .wait()
                .await
                .map_err(|error| {
                    RemoteClientError::Protocol(format!(
                        "failed to reap SSH for {server_id}: {error}"
                    ))
                })?;
            let removed = self.connections.lock().await.remove(server_id);
            drop(removed);
        }
        self.workspaces
            .lock()
            .await
            .retain(|(id, _), _| id != server_id);
        self.set_state(server_id, SshConnectionState::Disconnected)
            .await;
        Ok(())
    }
    pub(super) async fn admit_connection(&self) -> Result<TaskTrackerToken, RemoteClientError> {
        let accepting = self.admission.lock().await;
        if !*accepting {
            return Err(RemoteClientError::ManagerClosing);
        }
        Ok(self.operations.token())
    }

    /// Permanently seals connection admission and drains owned SSH connections.
    ///
    /// Repeated calls reuse retained connections after a failure or canceled wait.
    /// Dependent remote tools and services must be stopped before calling this.
    ///
    /// # Errors
    /// Returns a transport or process-wait failure without removing that connection.
    pub async fn shutdown(&self) -> Result<(), RemoteClientError> {
        {
            let mut accepting = self.admission.lock().await;
            *accepting = false;
            self.operations.close();
        }
        self.closing.cancel();
        self.desired_connections.write().await.clear();
        self.operations.wait().await;

        let connections = self
            .connections
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut first_error = None;
        for server_id in connections {
            let result = self.close_connection(&server_id).await;
            match result {
                Ok(()) => {}
                Err(error) => {
                    self.set_state(
                        &server_id,
                        SshConnectionState::Failed {
                            code: "sshShutdownFailed".to_string(),
                            message: error.to_string(),
                        },
                    )
                    .await;
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
