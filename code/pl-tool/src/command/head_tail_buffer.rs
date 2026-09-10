use std::collections::VecDeque;

/// 保留输出前缀和后缀的有界缓冲。
#[derive(Debug)]
pub(super) struct HeadTailBuffer {
    max_bytes: usize,
    head_budget: usize,
    tail_budget: usize,
    head: VecDeque<Vec<u8>>,
    tail: VecDeque<Vec<u8>>,
    head_bytes: usize,
    tail_bytes: usize,
    omitted_bytes: usize,
}

impl HeadTailBuffer {
    pub(super) fn new(max_bytes: usize) -> Self {
        let head_budget = max_bytes / 2;
        let tail_budget = max_bytes.saturating_sub(head_budget);
        Self {
            max_bytes,
            head_budget,
            tail_budget,
            head: VecDeque::new(),
            tail: VecDeque::new(),
            head_bytes: 0,
            tail_bytes: 0,
            omitted_bytes: 0,
        }
    }

    pub(super) fn push_chunk(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if self.max_bytes == 0 {
            self.omitted_bytes = self.omitted_bytes.saturating_add(chunk.len());
            return;
        }
        if self.head_bytes < self.head_budget {
            let remaining_head = self.head_budget.saturating_sub(self.head_bytes);
            if chunk.len() <= remaining_head {
                self.head_bytes = self.head_bytes.saturating_add(chunk.len());
                self.head.push_back(chunk.to_vec());
                return;
            }
            let (head_part, tail_part) = chunk.split_at(remaining_head);
            if !head_part.is_empty() {
                self.head_bytes = self.head_bytes.saturating_add(head_part.len());
                self.head.push_back(head_part.to_vec());
            }
            self.push_to_tail(tail_part.to_vec());
            return;
        }
        self.push_to_tail(chunk.to_vec());
    }

    pub(super) fn display_text(&self) -> String {
        let mut bytes = Vec::with_capacity(self.head_bytes.saturating_add(self.tail_bytes));
        for chunk in &self.head {
            bytes.extend_from_slice(chunk);
        }
        if self.omitted_bytes > 0 {
            let omitted = self.omitted_bytes;
            bytes
                .extend_from_slice(format!("\n\n... [{omitted} bytes omitted] ...\n\n").as_bytes());
        }
        for chunk in &self.tail {
            bytes.extend_from_slice(chunk);
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    pub(super) fn take_display_text(&mut self) -> String {
        let replacement = Self::new(self.max_bytes);
        std::mem::replace(self, replacement).display_text()
    }

    pub(super) fn total_bytes(&self) -> usize {
        self.head_bytes
            .saturating_add(self.tail_bytes)
            .saturating_add(self.omitted_bytes)
    }

    fn push_to_tail(&mut self, chunk: Vec<u8>) {
        if self.tail_budget == 0 {
            self.omitted_bytes = self.omitted_bytes.saturating_add(chunk.len());
            return;
        }
        if chunk.len() >= self.tail_budget {
            let start = chunk.len().saturating_sub(self.tail_budget);
            let kept = chunk[start..].to_vec();
            let dropped = chunk.len().saturating_sub(kept.len());
            self.omitted_bytes = self
                .omitted_bytes
                .saturating_add(self.tail_bytes)
                .saturating_add(dropped);
            self.tail.clear();
            self.tail_bytes = kept.len();
            self.tail.push_back(kept);
            return;
        }
        self.tail_bytes = self.tail_bytes.saturating_add(chunk.len());
        self.tail.push_back(chunk);
        self.trim_tail_to_budget();
    }

    fn trim_tail_to_budget(&mut self) {
        let mut excess = self.tail_bytes.saturating_sub(self.tail_budget);
        while excess > 0 {
            match self.tail.front_mut() {
                Some(front) if excess >= front.len() => {
                    excess -= front.len();
                    self.tail_bytes = self.tail_bytes.saturating_sub(front.len());
                    self.omitted_bytes = self.omitted_bytes.saturating_add(front.len());
                    self.tail.pop_front();
                }
                Some(front) => {
                    front.drain(..excess);
                    self.tail_bytes = self.tail_bytes.saturating_sub(excess);
                    self.omitted_bytes = self.omitted_bytes.saturating_add(excess);
                    break;
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn preserves_short_output() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push_chunk(b"hello");

        assert_eq!(buffer.display_text(), "hello");
    }

    #[test]
    fn omits_middle_when_capacity_is_exceeded() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push_chunk(b"aaaaabbbbbccccc");

        let text = buffer.display_text();
        assert!(text.starts_with("aaaaa"));
        assert!(text.ends_with("ccccc"));
        assert!(text.contains("bytes omitted"));
    }

    #[test]
    fn taking_display_text_drains_only_the_current_increment() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push_chunk(b"first");

        assert_eq!(buffer.take_display_text(), "first");
        assert_eq!(buffer.take_display_text(), "");

        buffer.push_chunk(b"second");
        assert_eq!(buffer.take_display_text(), "second");
    }
}
