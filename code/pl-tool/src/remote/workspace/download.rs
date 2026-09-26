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
    #[error("remote capture is {total} bytes, past the {limit}-byte transfer ceiling")]
    TooLarge { total: u64, limit: u64 },
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
        self.copy_capture(path, writer, None).await
    }

    /// Copies a completed remote capture only while its real length stays within `limit`.
    ///
    /// The bound is enforced from the first range response's authoritative `total_len`, before any
    /// byte is staged, so a runaway remote capture is refused with a typed reason instead of being
    /// transferred and rejected only after the whole copy. The caller owns the staging writer and
    /// publishes it only after this operation succeeds.
    ///
    /// # Errors
    /// Returns [`RemoteDownloadError::TooLarge`] for a capture past `limit`, plus the same path,
    /// framing, transport and staging failures as [`Self::copy_file_to`].
    pub async fn copy_capture_bounded<W: AsyncWrite + Unpin + Send>(
        &self,
        path: &str,
        writer: &mut W,
        limit: u64,
    ) -> Result<u64, RemoteDownloadError> {
        self.copy_capture(path, writer, Some(limit)).await
    }

    async fn copy_capture<W: AsyncWrite + Unpin + Send>(
        &self,
        path: &str,
        writer: &mut W,
        limit: Option<u64>,
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
            // The bound is checked against the authoritative total before the first byte is staged,
            // so a capture past the ceiling is refused without a full transfer.
            if let Some(limit) = limit
                && total_len > limit
            {
                return Err(RemoteDownloadError::TooLarge {
                    total: total_len,
                    limit,
                });
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
