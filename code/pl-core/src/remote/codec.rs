use std::io;

use pl_protocol::remote::{
    REMOTE_MAX_BODY_BYTES, REMOTE_MAX_HEADER_BYTES, RemoteFrameHeader, RemoteMessage,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug)]
pub(super) struct Frame {
    pub(super) request_id: Option<u64>,
    pub(super) message: RemoteMessage,
    pub(super) body: Vec<u8>,
}

pub(super) async fn read_frame<R>(reader: &mut R) -> io::Result<Option<Frame>>
where
    R: AsyncRead + Unpin,
{
    let header_len = match reader.read_u32().await {
        Ok(value) => value as usize,
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    };
    if header_len == 0 || header_len > REMOTE_MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("remote frame header length {header_len} is invalid"),
        ));
    }
    let mut header_bytes = vec![0; header_len];
    reader.read_exact(&mut header_bytes).await?;
    let header: RemoteFrameHeader = serde_json::from_slice(&header_bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to decode remote frame header: {error}"),
        )
    })?;
    if header.body_len > REMOTE_MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("remote frame body length {} is too large", header.body_len),
        ));
    }
    let mut body = vec![0; header.body_len];
    reader.read_exact(&mut body).await?;
    Ok(Some(Frame {
        request_id: header.request_id,
        message: header.message,
        body,
    }))
}

pub(super) async fn write_frame<W>(
    writer: &mut W,
    request_id: Option<u64>,
    message: RemoteMessage,
    body: &[u8],
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if body.len() > REMOTE_MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("remote frame body length {} is too large", body.len()),
        ));
    }
    let header = RemoteFrameHeader {
        request_id,
        message,
        body_len: body.len(),
    };
    let bytes = serde_json::to_vec(&header).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to encode remote frame header: {error}"),
        )
    })?;
    if bytes.len() > REMOTE_MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("remote frame header length {} is too large", bytes.len()),
        ));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.write_all(body).await?;
    writer.flush().await
}
