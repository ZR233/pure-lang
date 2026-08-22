use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkCompletionKind {
    Delivery,
    NoDelivery,
}

impl WorkCompletionKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::NoDelivery => "noDelivery",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryCompletion {
    head_commit: String,
    changed_files: Vec<String>,
}

impl DeliveryCompletion {
    fn new(head_commit: String, changed_files: Vec<String>) -> Option<Self> {
        (!head_commit.trim().is_empty()).then_some(Self {
            head_commit,
            changed_files,
        })
    }

    fn head_commit(&self) -> &str {
        &self.head_commit
    }

    fn changed_files(&self) -> &[String] {
        &self.changed_files
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NoDeliveryCompletion {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkCompletionContent {
    Delivery(DeliveryCompletion),
    NoDelivery(NoDeliveryCompletion),
}

impl WorkCompletionContent {
    pub(crate) fn delivery(head_commit: String, changed_files: Vec<String>) -> Option<Self> {
        DeliveryCompletion::new(head_commit, changed_files).map(Self::Delivery)
    }

    pub(crate) fn no_delivery() -> Self {
        Self::NoDelivery(NoDeliveryCompletion {})
    }

    pub(crate) const fn kind(&self) -> WorkCompletionKind {
        match self {
            Self::Delivery(_) => WorkCompletionKind::Delivery,
            Self::NoDelivery(_) => WorkCompletionKind::NoDelivery,
        }
    }

    pub(crate) fn head_commit(&self) -> Option<&str> {
        match self {
            Self::Delivery(value) => Some(value.head_commit()),
            Self::NoDelivery(_) => None,
        }
    }

    pub(crate) fn changed_files(&self) -> &[String] {
        match self {
            Self::Delivery(value) => value.changed_files(),
            Self::NoDelivery(_) => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_union_enforces_delivery_head_and_no_delivery_payload() {
        assert!(
            WorkCompletionContent::delivery(String::new(), vec!["src/lib.rs".to_string()])
                .is_none()
        );
        let no_delivery = WorkCompletionContent::no_delivery();
        assert_eq!(no_delivery.head_commit(), None);
        assert!(no_delivery.changed_files().is_empty());
        assert!(
            serde_json::from_value::<WorkCompletionContent>(serde_json::json!({
                "kind": "noDelivery",
                "data": {"headCommit": "bad"}
            }))
            .is_err()
        );
    }
}
