pub mod text;

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
