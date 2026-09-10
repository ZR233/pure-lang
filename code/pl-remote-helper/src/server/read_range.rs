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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_ranges_preserve_bytes_and_report_exact_eof() {
        let directory = tempfile::tempdir().unwrap();
        let content = b"a\x00\xffbcd";
        tokio::fs::write(directory.path().join("capture"), content)
            .await
            .unwrap();
        let mut workspaces = WorkspaceRegistry::default();
        let (workspace_id, _) = workspaces.open_resolved(directory.path().canonicalize().unwrap());
        let state = Arc::new(Mutex::new(ServerState {
            workspaces,
            shell: RemoteShellDescriptor {
                dialect: RemoteShellDialect::Bash,
                path: "/bin/bash".into(),
            },
        }));
        let request = |offset, max_bytes| RemoteReadRangeRequest {
            workspace_id: workspace_id.clone(),
            path: "capture".into(),
            offset,
            max_bytes,
        };
        let mut collected = Vec::new();
        for offset in [0, 4, 6] {
            let (response, bytes) = read(&state, request(offset, 4)).await.unwrap();
            assert_eq!(
                response,
                RemoteResponse::ByteRange {
                    offset,
                    total_len: 6
                }
            );
            collected.extend(bytes);
        }
        assert_eq!(collected, content);
        assert!(read(&state, request(7, 4)).await.is_err());
        assert!(read(&state, request(0, 0)).await.is_err());
        assert!(
            read(&state, request(0, REMOTE_MAX_BODY_BYTES + 1))
                .await
                .is_err()
        );
    }
}
