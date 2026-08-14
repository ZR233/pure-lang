use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::ChildStderr;

const STDERR_TAIL_BYTES: usize = 16 * 1024;

pub(super) struct StderrCapture {
    tail: Arc<Mutex<VecDeque<u8>>>,
    secrets: Vec<String>,
}

impl StderrCapture {
    pub(super) fn spawn(stderr: ChildStderr, environment: &BTreeMap<String, String>) -> Self {
        let capture = Self {
            tail: Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES))),
            secrets: sensitive_values(environment),
        };
        let tail = capture.tail.clone();
        tokio::spawn(async move { drain_stderr(stderr, tail).await });
        capture
    }

    pub(super) fn render(&self) -> Option<String> {
        let tail = self.tail.lock().expect("MCP stderr capture lock");
        render_tail(&tail, &self.secrets)
    }
}

async fn drain_stderr(mut stderr: ChildStderr, tail: Arc<Mutex<VecDeque<u8>>>) {
    let mut buffer = [0_u8; 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => append_tail(
                &mut tail.lock().expect("MCP stderr capture lock"),
                &buffer[..read],
            ),
        }
    }
}

fn append_tail(tail: &mut VecDeque<u8>, bytes: &[u8]) {
    tail.extend(bytes);
    while tail.len() > STDERR_TAIL_BYTES {
        tail.pop_front();
    }
}

fn sensitive_values(environment: &BTreeMap<String, String>) -> Vec<String> {
    environment
        .iter()
        .filter(|(name, value)| !value.is_empty() && is_sensitive_name(name))
        .map(|(_, value)| value.clone())
        .collect()
}

fn is_sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|marker| name.contains(marker))
}

fn render_tail(tail: &VecDeque<u8>, secrets: &[String]) -> Option<String> {
    let bytes = tail.iter().copied().collect::<Vec<_>>();
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    for secret in secrets {
        text = text.replace(secret, "[REDACTED]");
    }
    let text = text
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect::<String>();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_tail_is_bounded_and_redacts_credentials() {
        let mut tail = VecDeque::new();
        append_tail(&mut tail, &vec![b'x'; STDERR_TAIL_BYTES]);
        append_tail(&mut tail, b" token=secret-value\n");

        let rendered = render_tail(&tail, &["secret-value".to_string()]).unwrap();

        assert!(tail.len() <= STDERR_TAIL_BYTES);
        assert!(rendered.contains("token=[REDACTED]"));
        assert!(!rendered.contains("secret-value"));
    }

    #[test]
    fn only_secret_like_environment_names_are_redacted() {
        let environment = BTreeMap::from([
            ("Z_AI_API_KEY".to_string(), "secret".to_string()),
            ("Z_AI_MODE".to_string(), "ZHIPU".to_string()),
        ]);

        assert_eq!(sensitive_values(&environment), vec!["secret"]);
    }
}
