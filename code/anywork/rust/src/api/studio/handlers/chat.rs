use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::thread_stream::bridge_chat_thread_item;
use crate::api::studio::types::{
    BridgeChatDirection, BridgeChatFocus, BridgeChatItem, BridgeChatSnapshot, BridgeChatUpdate,
    BridgeError, BridgeViewChange,
};
use pl_studio_runtime::{
    ChatDirection, ChatFocus, ChatItem, ChatSnapshot, ChatUpdate, ChatUpdates, ChatView, ViewChange,
};

/// An independently positioned, bounded window. One `next` call is outstanding at a
/// time, so the FFI does not queue unprocessed token updates in Dart.
pub struct BridgeChatView {
    view: ChatView,
    initial: BridgeChatSnapshot,
    updates: Mutex<ChatUpdates>,
    cancel: CancellationToken,
}

pub async fn open_chat_view(
    thread_id: String,
    focus: BridgeChatFocus,
) -> Result<BridgeChatView, BridgeError> {
    let bridge = active_bridge().await?;
    let view = bridge
        .studio
        .open_chat(&thread_id, core_focus(focus))
        .await?;
    let (initial, updates) = view.subscribe();
    Ok(BridgeChatView {
        view,
        initial: bridge_snapshot(initial)?,
        updates: Mutex::new(updates),
        cancel: bridge.shutdown.child_token(),
    })
}

impl BridgeChatView {
    /// The subscription baseline is captured before returning the handle.
    pub fn initial(&self) -> BridgeChatSnapshot {
        self.initial.clone()
    }

    pub fn snapshot(&self) -> Result<BridgeChatSnapshot, BridgeError> {
        bridge_snapshot(self.view.snapshot())
    }

    /// Waits for one consolidated batch, then returns control to the consumer.
    pub async fn next(&self) -> Result<Option<BridgeChatUpdate>, BridgeError> {
        let mut updates = self.updates.lock().await;
        let update = tokio::select! {
            () = self.cancel.cancelled() => return Ok(None),
            update = updates.next() => update,
        };
        update.map(bridge_update).transpose()
    }

    pub async fn load(
        &self,
        direction: BridgeChatDirection,
    ) -> Result<BridgeChatSnapshot, BridgeError> {
        let direction = match direction {
            BridgeChatDirection::Older => ChatDirection::Older,
            BridgeChatDirection::Newer => ChatDirection::Newer,
        };
        bridge_snapshot(
            self.view
                .load(direction)
                .await
                .map_err(anyhow::Error::new)?,
        )
    }

    pub async fn focus(&self, focus: BridgeChatFocus) -> Result<BridgeChatSnapshot, BridgeError> {
        self.view
            .focus(core_focus(focus))
            .await
            .map_err(anyhow::Error::new)?;
        bridge_snapshot(self.view.snapshot())
    }

    pub async fn read_item(&self, item_id: String) -> Result<Option<BridgeChatItem>, BridgeError> {
        self.view
            .read_item(&item_id)
            .await
            .map_err(anyhow::Error::new)?
            .map(bridge_item)
            .transpose()
    }

    pub fn close(&self) {
        self.cancel.cancel();
    }
}

impl Drop for BridgeChatView {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn core_focus(focus: BridgeChatFocus) -> ChatFocus {
    match focus {
        BridgeChatFocus::Latest => ChatFocus::Latest,
        BridgeChatFocus::Around { item_id } => ChatFocus::Around(item_id),
    }
}

fn bridge_focus(focus: ChatFocus) -> BridgeChatFocus {
    match focus {
        ChatFocus::Latest => BridgeChatFocus::Latest,
        ChatFocus::Around(item_id) => BridgeChatFocus::Around { item_id },
    }
}

fn bridge_item(value: ChatItem) -> Result<BridgeChatItem, BridgeError> {
    let item: pl_protocol::ThreadItem = serde_json::from_str(&value.body)?;
    if item.id != value.item_id
        || item.ordinal != value.order
        || item.revision != value.revision
        || item.turn_id != value.turn_id
    {
        return Err(
            anyhow::anyhow!("chat item identity differs from its presentation body").into(),
        );
    }
    Ok(BridgeChatItem {
        item: bridge_chat_thread_item(item)?,
        saved: value.saved,
        omitted_bytes: value.omitted_bytes,
    })
}

fn bridge_snapshot(snapshot: ChatSnapshot) -> Result<BridgeChatSnapshot, BridgeError> {
    Ok(BridgeChatSnapshot {
        focus: bridge_focus(snapshot.focus),
        version: snapshot.version,
        items: snapshot
            .items
            .into_iter()
            .map(bridge_item)
            .collect::<Result<_, _>>()?,
        has_older: snapshot.has_older,
        has_newer: snapshot.has_newer,
    })
}

fn bridge_update(update: ChatUpdate) -> Result<BridgeChatUpdate, BridgeError> {
    Ok(match update {
        ChatUpdate::Reset(snapshot) => BridgeChatUpdate::Reset {
            snapshot: bridge_snapshot(snapshot)?,
        },
        ChatUpdate::Patch {
            from,
            to,
            changes,
            has_newer,
        } => BridgeChatUpdate::Patch {
            from,
            to,
            changes: changes
                .into_iter()
                .map(|change| {
                    Ok(match change {
                        ViewChange::Splice {
                            index,
                            remove,
                            items,
                        } => BridgeViewChange::Splice {
                            index: u64::try_from(index).map_err(anyhow::Error::new)?,
                            remove: u64::try_from(remove).map_err(anyhow::Error::new)?,
                            items: items
                                .into_iter()
                                .map(bridge_item)
                                .collect::<Result<_, _>>()?,
                        },
                        ViewChange::AppendText {
                            item_id,
                            part_id,
                            expected_revision,
                            revision,
                            text,
                        } => BridgeViewChange::AppendText {
                            item_id,
                            part_id,
                            expected_revision,
                            revision,
                            text,
                        },
                    })
                })
                .collect::<Result<_, BridgeError>>()?,
            has_newer,
        },
    })
}
