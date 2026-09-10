//! Error-source wire projection. Restoring diagnostics never loads provider or plugin code.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{error::Error, fmt};

type Source = Box<dyn Error + Send + Sync>;

#[derive(Debug)]
struct RecordedSource {
    message: String,
    source: Option<Box<RecordedSource>>,
}
impl fmt::Display for RecordedSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl Error for RecordedSource {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as &dyn Error)
    }
}

fn capture(source: &(dyn Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = Some(source);
    let mut visited: Vec<&(dyn Error + 'static)> = Vec::new();
    while let Some(error) = current {
        if visited
            .iter()
            .any(|previous| std::ptr::eq(*previous, error))
        {
            chain.push("[cyclic error source omitted]".into());
            break;
        }
        if visited.len() == 64 {
            chain.push("[error source chain exceeds 64 entries]".into());
            break;
        }
        visited.push(error);
        chain.push(error.to_string());
        current = error.source();
    }
    chain
}
fn restore<E: serde::de::Error>(chain: Vec<String>) -> Result<Source, E> {
    let mut source = None;
    for message in chain.into_iter().rev() {
        source = Some(Box::new(RecordedSource { message, source }));
    }
    source
        .map(|source| source as Source)
        .ok_or_else(|| E::custom("recorded error chain is empty"))
}

pub(crate) mod required {
    use super::*;
    pub(crate) fn serialize<S: Serializer>(
        source: &Source,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        capture(source.as_ref()).serialize(serializer)
    }
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Source, D::Error> {
        restore(Vec::<String>::deserialize(deserializer)?)
    }
}

pub(crate) mod optional {
    use super::*;
    pub(crate) fn serialize<S: Serializer>(
        source: &Option<Source>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        source
            .as_ref()
            .map(|source| capture(source.as_ref()))
            .serialize(serializer)
    }
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Source>, D::Error> {
        Option::<Vec<String>>::deserialize(deserializer)?
            .map(restore)
            .transpose()
    }
}

/// Panic information retained at an implementation boundary instead of terminating its owner.
#[derive(Debug, thiserror::Error)]
#[error("{operation} panicked: {message}")]
pub(crate) struct BoundaryPanic {
    operation: &'static str,
    message: String,
}

pub(crate) async fn catch_boundary<T>(
    operation: &'static str,
    future: impl std::future::Future<Output = T> + Send,
) -> Result<T, BoundaryPanic> {
    use futures::FutureExt;
    std::panic::AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .map_err(|payload| {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|message| (*message).to_owned())
                })
                .unwrap_or_else(|| "non-text panic payload".into());
            BoundaryPanic { operation, message }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Cyclic;
    impl fmt::Display for Cyclic {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("cycle")
        }
    }
    impl Error for Cyclic {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self)
        }
    }
    #[test]
    fn cyclic_error_source_does_not_block_journal_encoding() {
        assert_eq!(
            capture(&Cyclic),
            vec!["cycle", "[cyclic error source omitted]"]
        );
    }

    #[test]
    fn diagnostic_chain_round_trips_without_instantiating_the_original_error_type() {
        let error = crate::model::ModelError {
            details: Some(Box::new(
                crate::context::OpaquePayload::new("unknown.failure", 73, "original\r\n诊断\0")
                    .unwrap(),
            )),
            kind: crate::model::ModelFailureKind::Unavailable,
            usage: crate::model::ModelUsage {
                input_tokens: Some(9),
                ..Default::default()
            },
            source: Some(Box::new(RecordedSource {
                message: "provider context".into(),
                source: Some(Box::new(RecordedSource {
                    message: "transport failure\0".into(),
                    source: None,
                })),
            })),
        };
        let encoded = serde_json::to_string(&error).unwrap();
        let restored: crate::model::ModelError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.details, error.details);
        assert_eq!(
            capture(error.source.as_deref().unwrap()),
            capture(restored.source.as_deref().unwrap())
        );
        assert_eq!(restored.usage, error.usage);
        assert_eq!(restored.kind, error.kind);
    }
}
