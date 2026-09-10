/// shell 命令 timeout 包装策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCommandTimeout {
    Disabled,
    Seconds(u64),
}

impl ShellCommandTimeout {
    pub fn from_optional_seconds(timeout_secs: Option<u64>) -> Self {
        match timeout_secs {
            Some(seconds) => Self::Seconds(seconds),
            None => Self::Disabled,
        }
    }
}

/// 为需要在 `/bin/sh -lc` 下执行的命令添加可选 timeout 包装。
pub fn shell_command_with_timeout(command: &str, timeout: ShellCommandTimeout) -> String {
    match timeout {
        ShellCommandTimeout::Seconds(seconds) if seconds > 0 => {
            format!(
                "timeout --preserve-status {seconds}s /bin/sh -lc {}",
                shell_quote_word(command)
            )
        }
        ShellCommandTimeout::Disabled | ShellCommandTimeout::Seconds(_) => command.to_string(),
    }
}

/// 对单个 shell word 做 POSIX 风格转义。
pub fn shell_quote_word(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
