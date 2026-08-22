use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspAvailable {
    checked_at: i64,
    diagnostic_count: u64,
    activity: LspAvailableActivity,
}

impl LspAvailable {
    pub fn new(checked_at: i64, diagnostic_count: u64, activity: LspAvailableActivity) -> Self {
        Self {
            checked_at,
            diagnostic_count,
            activity,
        }
    }

    pub fn checked_at(&self) -> i64 {
        self.checked_at
    }

    pub fn diagnostic_count(&self) -> u64 {
        self.diagnostic_count
    }

    pub fn activity(&self) -> &LspAvailableActivity {
        &self.activity
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum LspAvailableActivity {
    Idle(LspIdle),
    Busy(LspBusy),
    Indexing(LspIndexing),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LspIdle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspBusy {
    title: Option<String>,
    message: Option<String>,
    percentage: Option<u32>,
}

impl LspBusy {
    pub fn new(title: Option<String>, message: Option<String>, percentage: Option<u32>) -> Self {
        Self {
            title,
            message,
            percentage,
        }
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn percentage(&self) -> Option<u32> {
        self.percentage
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspIndexing {
    title: Option<String>,
    message: Option<String>,
    percentage: Option<u32>,
}

impl LspIndexing {
    pub fn new(title: Option<String>, message: Option<String>, percentage: Option<u32>) -> Self {
        Self {
            title,
            message,
            percentage,
        }
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn percentage(&self) -> Option<u32> {
        self.percentage
    }
}
