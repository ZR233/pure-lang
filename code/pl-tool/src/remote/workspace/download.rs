//! Bounded remote capture transfer into a caller-owned local writer.
use pl_protocol::remote::{RemoteReadRangeRequest, RemoteRequest, RemoteResponse};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::RemoteWorkspaceFileBackend;
use crate::remote::RemoteClientError;

const CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RemoteDownloadError {
    #[error("invalid remote capture path")]
    Path(#[source] pl_protocol::PureError),
    #[error("remote capture read failed at byte {offset}")]
    Read {
        offset: u64,
        #[source]
        source: RemoteClientError,
    },
    #[error("remote capture range is inconsistent at byte {offset}")]
    Inconsistent { offset: u64 },
    #[error("unexpected remote capture response: {0}")]
    Response(String),
    #[error("local capture staging failed at byte {offset}")]
    Write {
        offset: u64,
        #[source]
        source: std::io::Error,
    },
}

impl RemoteWorkspaceFileBackend {
    /// Copies a completed remote capture without imposing a single-frame total-size limit.
    /// The caller owns the staging writer and publishes it only after this operation succeeds.
    ///
    /// # Errors
    /// Rejects invalid paths, changed lengths, incomplete ranges, transport and staging failures.
    pub async fn copy_file_to<W: AsyncWrite + Unpin + Send>(
        &self,
        path: &str,
        writer: &mut W,
    ) -> Result<u64, RemoteDownloadError> {
        let path = self
            .path_request(path.to_owned(), None)
            .map_err(RemoteDownloadError::Path)?;
        let mut offset = 0_u64;
        let mut expected_total = None;
        loop {
            let reply = self
                .client
                .request(
                    RemoteRequest::ReadRange(RemoteReadRangeRequest {
                        workspace_id: path.workspace_id.clone(),
                        path: path.path.clone(),
                        offset,
                        max_bytes: CHUNK_BYTES,
                    }),
                    &[],
                )
                .await
                .map_err(|source| RemoteDownloadError::Read { offset, source })?;
            let RemoteResponse::ByteRange {
                offset: actual_offset,
                total_len,
            } = reply.response
            else {
                return Err(RemoteDownloadError::Response(format!(
                    "{:?}",
                    reply.response
                )));
            };
            if actual_offset != offset
                || offset > total_len
                || expected_total.is_some_and(|expected| expected != total_len)
                || reply.body.len() as u64 != (total_len - offset).min(CHUNK_BYTES as u64)
            {
                return Err(RemoteDownloadError::Inconsistent { offset });
            }
            writer
                .write_all(&reply.body)
                .await
                .map_err(|source| RemoteDownloadError::Write { offset, source })?;
            offset += reply.body.len() as u64;
            expected_total = Some(total_len);
            if offset == total_len {
                return Ok(total_len);
            }
        }
    }
}
