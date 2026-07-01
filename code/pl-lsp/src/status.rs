use std::collections::{HashMap, HashSet};

use lsp_types::{ProgressParams, ProgressParamsValue, ProgressToken, WorkDoneProgress};

use crate::types::LspActivityKind;

/// LSP 客户端运行时状态快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LspClientRuntimeStatus {
    pub activity_kind: LspActivityKind,
    pub activity_title: Option<String>,
    pub activity_message: Option<String>,
    pub activity_percentage: Option<u32>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

impl Default for LspClientRuntimeStatus {
    fn default() -> Self {
        Self {
            activity_kind: LspActivityKind::Idle,
            activity_title: None,
            activity_message: None,
            activity_percentage: None,
            last_error: None,
            last_error_at: None,
        }
    }
}

/// LSP 进度条目。
#[derive(Debug, Clone)]
pub(crate) struct LspProgressEntry {
    pub activity_kind: LspActivityKind,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
    pub sequence: u64,
}

/// LSP 客户端内部状态和进度跟踪。
#[derive(Debug, Default)]
pub(crate) struct LspClientStatus {
    registered_progress_tokens: HashSet<ProgressToken>,
    progress: HashMap<ProgressToken, LspProgressEntry>,
    next_progress_sequence: u64,
    last_error: Option<String>,
    last_error_at: Option<i64>,
}

impl LspClientStatus {
    pub fn register_progress_token(&mut self, token: ProgressToken) -> bool {
        self.registered_progress_tokens.insert(token)
    }

    pub fn apply_progress(&mut self, params: ProgressParams) -> bool {
        if !self.registered_progress_tokens.contains(&params.token) {
            return false;
        }
        match params.value {
            ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(progress)) => {
                self.next_progress_sequence += 1;
                let activity_kind =
                    activity_kind_for_progress(&progress.title, progress.message.as_deref());
                self.progress.insert(
                    params.token,
                    LspProgressEntry {
                        activity_kind,
                        title: progress.title,
                        message: progress.message,
                        percentage: progress.percentage,
                        sequence: self.next_progress_sequence,
                    },
                );
            }
            ProgressParamsValue::WorkDone(WorkDoneProgress::Report(progress)) => {
                if let Some(entry) = self.progress.get_mut(&params.token) {
                    self.next_progress_sequence += 1;
                    entry.sequence = self.next_progress_sequence;
                    entry.message = progress.message;
                    entry.percentage = progress.percentage;
                }
            }
            ProgressParamsValue::WorkDone(WorkDoneProgress::End(_)) => {
                self.progress.remove(&params.token);
            }
        }
        true
    }

    pub fn clear_progress(&mut self) -> bool {
        let changed = !self.progress.is_empty() || !self.registered_progress_tokens.is_empty();
        self.registered_progress_tokens.clear();
        self.progress.clear();
        changed
    }

    pub fn record_error(&mut self, message: String) -> bool {
        self.last_error = Some(message);
        self.last_error_at = Some(unix_seconds());
        true
    }

    pub fn runtime_status(&self) -> LspClientRuntimeStatus {
        let Some(entry) = self.progress.values().max_by_key(|entry| entry.sequence) else {
            return LspClientRuntimeStatus {
                last_error: self.last_error.clone(),
                last_error_at: self.last_error_at,
                ..LspClientRuntimeStatus::default()
            };
        };
        LspClientRuntimeStatus {
            activity_kind: entry.activity_kind,
            activity_title: Some(entry.title.clone()),
            activity_message: entry.message.clone(),
            activity_percentage: entry.percentage,
            last_error: self.last_error.clone(),
            last_error_at: self.last_error_at,
        }
    }
}

/// 根据进度标题和消息判断活动类型。
fn activity_kind_for_progress(title: &str, message: Option<&str>) -> LspActivityKind {
    let msg = message.unwrap_or_default();
    let text = format!("{title} {msg}").to_ascii_lowercase();
    if text.contains("index")
        || text.contains("fetching")
        || text.contains("crategraph")
        || text.contains("crate graph")
        || text.contains("roots scanned")
        || text.contains("cargo metadata")
        || text.contains("compile-time-deps")
        || text.contains("discovering sysroot")
        || text.contains("querying project metadata")
    {
        LspActivityKind::Indexing
    } else {
        LspActivityKind::Busy
    }
}

/// 获取当前 Unix 时间戳（秒）。
fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use lsp_types::{ProgressParams, ProgressParamsValue, ProgressToken, WorkDoneProgress};
    use pretty_assertions::assert_eq;

    use super::LspClientStatus;
    use crate::LspActivityKind;

    #[test]
    fn client_status_tracks_registered_indexing_progress() {
        let mut status = LspClientStatus::default();
        let token = ProgressToken::String("rust-analyzer".to_string());

        assert!(status.register_progress_token(token.clone()));
        assert!(!status.register_progress_token(token.clone()));

        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                lsp_types::WorkDoneProgressBegin {
                    title: "Roots Scanned".to_string(),
                    message: Some("0/408".to_string()),
                    percentage: Some(0),
                    cancellable: Some(false),
                },
            )),
        }));
        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                lsp_types::WorkDoneProgressReport {
                    message: Some("166/408".to_string()),
                    percentage: Some(40),
                    cancellable: Some(false),
                },
            )),
        }));

        let runtime = status.runtime_status();

        assert_eq!(runtime.activity_kind, LspActivityKind::Indexing);
        assert_eq!(runtime.activity_title, Some("Roots Scanned".to_string()));
        assert_eq!(runtime.activity_message, Some("166/408".to_string()));
        assert_eq!(runtime.activity_percentage, Some(40));
    }

    #[test]
    fn client_status_tracks_completed_indexing_progress() {
        let mut status = LspClientStatus::default();
        let token = ProgressToken::String("rust-analyzer".to_string());

        assert!(status.register_progress_token(token.clone()));

        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                lsp_types::WorkDoneProgressBegin {
                    title: "Indexing".to_string(),
                    message: None,
                    percentage: None,
                    cancellable: None,
                },
            )),
        }));
        assert_eq!(
            status.runtime_status().activity_kind,
            LspActivityKind::Indexing
        );

        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                lsp_types::WorkDoneProgressEnd::default(),
            )),
        }));
        assert_eq!(status.runtime_status().activity_kind, LspActivityKind::Idle);
    }

    #[test]
    fn client_status_ignores_unregistered_progress() {
        let mut status = LspClientStatus::default();

        let changed = status.apply_progress(ProgressParams {
            token: ProgressToken::String("unknown".to_string()),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                lsp_types::WorkDoneProgressBegin {
                    title: "Indexing".to_string(),
                    message: None,
                    percentage: None,
                    cancellable: None,
                },
            )),
        });

        assert!(!changed);
        assert_eq!(status.runtime_status().activity_kind, LspActivityKind::Idle);
    }

    #[test]
    fn client_status_records_last_error() {
        let mut status = LspClientStatus::default();

        assert!(status.record_error("LSP server error -32603: url is not a file".to_string()));

        let runtime = status.runtime_status();
        assert_eq!(
            runtime.last_error,
            Some("LSP server error -32603: url is not a file".to_string())
        );
        assert!(runtime.last_error_at.is_some());
    }
}
