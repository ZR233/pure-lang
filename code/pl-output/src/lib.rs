use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// 单个输出流的截断结果。
///
/// 原始输出超过截断限制时，中间部分用省略指示替换。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TruncatedOutput {
    /// 截断后的内容。
    pub content: String,
    /// 是否发生了截断。
    pub was_truncated: bool,
    /// 原始内容的字节长度。
    pub original_length: usize,
}

impl TruncatedOutput {
    pub fn empty() -> Self {
        Self {
            content: String::new(),
            was_truncated: false,
            original_length: 0,
        }
    }
}

/// stdout/stderr 各自的截断结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputTruncation {
    pub stdout: TruncatedOutput,
    pub stderr: TruncatedOutput,
}

impl OutputTruncation {
    pub fn empty() -> Self {
        Self {
            stdout: TruncatedOutput::empty(),
            stderr: TruncatedOutput::empty(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundedOutput {
    pub text: String,
    pub truncated: bool,
    pub bytes_omitted: usize,
    pub next_offset: Option<usize>,
}

pub fn bounded_text(value: &str, max_bytes: usize, offset: usize) -> BoundedOutput {
    if value.len() <= max_bytes {
        return BoundedOutput {
            text: value.to_string(),
            truncated: false,
            bytes_omitted: 0,
            next_offset: None,
        };
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    BoundedOutput {
        text: value[..end].to_string(),
        truncated: true,
        bytes_omitted: value.len().saturating_sub(end),
        next_offset: Some(offset.saturating_add(end)),
    }
}

#[derive(Debug)]
pub struct HeadTailBuffer {
    cap: usize,
    head: Vec<u8>,
    tail: VecDequeBytes,
    total: usize,
}

impl HeadTailBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            head: Vec::new(),
            tail: VecDequeBytes::new(cap / 2),
            total: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        if self.cap == 0 {
            return;
        }
        let head_cap = self.head_cap();
        if self.head.len() < head_cap {
            let take = (head_cap - self.head.len()).min(bytes.len());
            self.head.extend_from_slice(&bytes[..take]);
            if take < bytes.len() {
                self.tail.push(&bytes[take..]);
            }
        } else {
            self.tail.push(bytes);
        }
    }

    pub fn truncated(&self) -> bool {
        self.total > self.cap
    }

    pub fn into_bytes(self) -> Vec<u8> {
        if !self.truncated() {
            let mut out = self.head;
            out.extend_from_slice(&self.tail.into_vec());
            return out;
        }
        let omitted = self
            .total
            .saturating_sub(self.head.len())
            .saturating_sub(self.tail.len());
        let marker = format!("\n... omitted {omitted} bytes ...\n");
        let mut out = self.head;
        out.extend_from_slice(marker.as_bytes());
        out.extend_from_slice(&self.tail.into_vec());
        out
    }

    fn head_cap(&self) -> usize {
        self.cap.saturating_sub(self.cap / 2)
    }
}

#[derive(Debug)]
struct VecDequeBytes {
    cap: usize,
    bytes: VecDeque<u8>,
}

impl VecDequeBytes {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            bytes: VecDeque::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.cap == 0 {
            return;
        }
        if bytes.len() >= self.cap {
            self.bytes.clear();
            self.bytes.extend(
                bytes[bytes.len().saturating_sub(self.cap)..]
                    .iter()
                    .copied(),
            );
            return;
        }
        self.bytes.extend(bytes.iter().copied());
        if self.bytes.len() > self.cap {
            let excess = self.bytes.len() - self.cap;
            self.bytes.drain(..excess);
        }
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn into_vec(self) -> Vec<u8> {
        self.bytes.into_iter().collect()
    }
}

/// 可配置的输出截断策略。
///
/// 保留前 `head_limit` 和后 `tail_limit` 字节，中间被省略的部分
/// 用指示器标注省略的字节数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncationStrategy {
    pub head_limit: usize,
    pub tail_limit: usize,
}

impl Default for TruncationStrategy {
    fn default() -> Self {
        Self {
            head_limit: 1000,
            tail_limit: 1000,
        }
    }
}

impl TruncationStrategy {
    pub fn new(head_limit: usize, tail_limit: usize) -> Self {
        Self {
            head_limit,
            tail_limit,
        }
    }

    /// 按策略截断字符串。
    ///
    /// 输入长度 ≤ head + tail 时原样返回；否则拼接 head + 省略指示 + tail。
    pub fn truncate(&self, input: &str) -> TruncatedOutput {
        let original_length = input.len();

        if original_length <= self.head_limit + self.tail_limit {
            return TruncatedOutput {
                content: input.to_string(),
                was_truncated: false,
                original_length,
            };
        }

        let head_end = input.floor_char_boundary(self.head_limit);
        let tail_start = input.floor_char_boundary(original_length - self.tail_limit);

        let omitted = tail_start - head_end;
        let content = format!(
            "{}\n\n... [{omitted} characters omitted] ...\n\n{}",
            &input[..head_end],
            &input[tail_start..],
        );

        TruncatedOutput {
            content,
            was_truncated: true,
            original_length,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn short_string_not_truncated() {
        let strategy = TruncationStrategy::new(10, 10);
        let result = strategy.truncate("hello");

        assert_eq!(
            result,
            TruncatedOutput {
                content: "hello".to_string(),
                was_truncated: false,
                original_length: 5,
            }
        );
    }

    #[test]
    fn long_string_truncated_with_indicator() {
        let strategy = TruncationStrategy::new(5, 5);
        let input = "aaaaaBBBBBBBBBBccccceeeee";
        let result = strategy.truncate(input);

        assert!(result.was_truncated);
        assert!(result.content.starts_with("aaaaa"));
        assert!(result.content.ends_with("eeeee"));
        assert!(result.content.contains("characters omitted"));
        assert_eq!(result.original_length, 25);
    }

    #[test]
    fn exact_boundary_not_truncated() {
        let strategy = TruncationStrategy::new(5, 5);
        let result = strategy.truncate("1234567890");

        assert_eq!(
            result,
            TruncatedOutput {
                content: "1234567890".to_string(),
                was_truncated: false,
                original_length: 10,
            }
        );
    }

    #[test]
    fn empty_string_not_truncated() {
        let strategy = TruncationStrategy::default();
        let result = strategy.truncate("");

        assert_eq!(result, TruncatedOutput::empty());
    }

    #[test]
    fn default_strategy_uses_1000_chars() {
        let strategy = TruncationStrategy::default();
        assert_eq!(
            strategy,
            TruncationStrategy {
                head_limit: 1000,
                tail_limit: 1000,
            }
        );
    }

    #[test]
    fn multibyte_text_is_split_on_char_boundaries() {
        let strategy = TruncationStrategy::new(4, 4);
        let result = strategy.truncate("你好世界abcdef再见");

        assert!(result.was_truncated);
        assert!(result.content.starts_with("你"));
        assert!(result.content.ends_with("见"));
    }

    #[test]
    fn bounded_text_reports_next_offset_on_char_boundary() {
        let result = bounded_text("你abc", 4, 7);

        assert_eq!(
            result,
            BoundedOutput {
                text: "你a".to_string(),
                truncated: true,
                bytes_omitted: 2,
                next_offset: Some(11),
            }
        );
    }

    #[test]
    fn head_tail_buffer_keeps_bounded_head_and_tail() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push(b"abcdef");
        buffer.push(b"ghijklmnop");

        assert!(buffer.truncated());
        let text = String::from_utf8(buffer.into_bytes()).expect("utf8");
        assert!(text.starts_with("abcde"));
        assert!(text.contains("omitted 6 bytes"));
        assert!(text.ends_with("lmnop"));
    }

    #[test]
    fn head_tail_buffer_keeps_full_when_under_cap() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push(b"abc");
        buffer.push(b"def");

        assert!(!buffer.truncated());
        assert_eq!(buffer.into_bytes(), b"abcdef");
    }
}
