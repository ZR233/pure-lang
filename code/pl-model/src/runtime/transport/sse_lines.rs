//! Feed complete lines to the SSE parser, which otherwise rescans an incomplete
//! line from its beginning on every network chunk (quadratic for large deltas).
use futures::{Stream, StreamExt, TryStreamExt, stream};

pub(super) fn complete_lines<B>(
    stream: impl Stream<Item = Result<B, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send
where
    B: AsRef<[u8]> + Send + 'static,
{
    let reader = LineReader {
        stream: stream.boxed(),
        chunk: None,
        offset: 0,
        line: Vec::new(),
        skip_lf: false,
    };
    stream::try_unfold(reader, |mut reader| async move {
        let line = reader.next_line().await?;
        Ok(line.map(|line| (line, reader)))
    })
}

struct LineReader<S, B> {
    stream: S,
    chunk: Option<B>,
    offset: usize,
    line: Vec<u8>,
    // CR already ended the last line. A following LF is part of that same
    // terminator, even when it arrives in a different HTTP chunk.
    skip_lf: bool,
}

impl<S, B> LineReader<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    async fn next_line(&mut self) -> Result<Option<Vec<u8>>, reqwest::Error> {
        loop {
            if let Some(chunk) = &self.chunk {
                let bytes = &chunk.as_ref()[self.offset..];
                if !bytes.is_empty() {
                    if self.skip_lf {
                        self.skip_lf = false;
                        if bytes[0] == b'\n' {
                            self.offset += 1;
                            continue;
                        }
                    }
                    if let Some(end) = bytes.iter().position(|b| matches!(b, b'\r' | b'\n')) {
                        self.line.extend_from_slice(&bytes[..end]);
                        self.line.push(b'\n');
                        self.skip_lf = bytes[end] == b'\r';
                        self.offset += end + 1;
                        return Ok(Some(std::mem::take(&mut self.line)));
                    }
                    self.line.extend_from_slice(bytes);
                }
                self.chunk = None;
                self.offset = 0;
            }
            self.chunk = self.stream.try_next().await?;
            if self.chunk.is_none() {
                // Preserve EOF semantics: do not invent a newline or dispatch an
                // unterminated event. The existing SSE parser owns that decision.
                return Ok((!self.line.is_empty()).then(|| std::mem::take(&mut self.line)));
            }
        }
    }
}
