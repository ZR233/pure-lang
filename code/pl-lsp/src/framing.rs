use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use crate::types::{LspResult, LspRuntimeError};

pub(crate) async fn read_message<R>(reader: &mut R) -> LspResult<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            let value = value.trim().parse::<usize>().map_err(|error| {
                LspRuntimeError::InvalidQuery(format!("invalid Content-Length: {error}"))
            })?;
            content_length = Some(value);
        }
    }

    let length = content_length
        .ok_or_else(|| LspRuntimeError::InvalidQuery("missing Content-Length".to_string()))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

pub(crate) fn encode_message(value: &serde_json::Value) -> LspResult<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut output = Vec::with_capacity(header.len() + body.len());
    output.extend_from_slice(header.as_bytes());
    output.extend_from_slice(&body);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tokio::io::BufReader;

    use super::*;

    #[tokio::test]
    async fn reads_content_length_framed_message() {
        let payload = br#"{"jsonrpc":"2.0","id":1,"result":true}"#;
        let input = format!("Content-Length: {}\r\n\r\n", payload.len());
        let bytes = [input.as_bytes(), payload].concat();
        let mut reader = BufReader::new(bytes.as_slice());

        let message = read_message(&mut reader).await.unwrap().unwrap();

        assert_eq!(message, payload);
    }

    #[test]
    fn encodes_content_length_framed_message() {
        let value = serde_json::json!({"jsonrpc":"2.0","method":"initialized","params":{}});

        let encoded = encode_message(&value).unwrap();
        let text = String::from_utf8(encoded).unwrap();

        assert!(text.starts_with("Content-Length: "));
        assert!(text.contains("\r\n\r\n"));
        assert!(text.ends_with(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#));
    }
}
