//! Bounded capture reads; no remote path is interpreted by the local host filesystem.
use super::*;
use pl_protocol::remote::{REMOTE_MAX_BODY_BYTES, RemoteReadRangeRequest};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(super) async fn read(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteReadRangeRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    if request.max_bytes == 0 || request.max_bytes > REMOTE_MAX_BODY_BYTES {
        return Err(remote_error(
            RemoteErrorCode::InvalidRequest,
            "invalid range byte limit",
        ));
    }
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_existing(&request.workspace_id, &request.path)
        .await?;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| io_error("failed to open range source", error))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|error| io_error("failed to inspect range source", error))?;
    if !metadata.is_file() || request.offset > metadata.len() {
        return Err(remote_error(
            RemoteErrorCode::InvalidRequest,
            "invalid file range",
        ));
    }
    file.seek(std::io::SeekFrom::Start(request.offset))
        .await
        .map_err(|error| io_error("failed to seek range source", error))?;
    let length = (metadata.len() - request.offset).min(request.max_bytes as u64) as usize;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .await
        .map_err(|error| io_error("range source changed while reading", error))?;
    Ok((
        RemoteResponse::ByteRange {
            offset: request.offset,
            total_len: metadata.len(),
        },
        bytes,
    ))
}
