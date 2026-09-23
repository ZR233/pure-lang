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
/// Diagnostic chain text of one source, exactly as serialization records it.
pub(crate) fn chain_text(source: &(dyn Error + 'static)) -> Vec<String> {
    capture(source)
}

/// Rebuilds the recorded chain a persisted diagnostic decodes to; an empty chain has no source.
///
/// The persisted form keeps only the chain text, so a rebuilt chain serializes and reports exactly
/// like the original without loading provider or plugin code.
pub(crate) fn chain_source(chain: Vec<String>) -> Option<Box<dyn Error + Send + Sync>> {
    let mut source = None;
    for message in chain.into_iter().rev() {
        source = Some(Box::new(RecordedSource { message, source }));
    }
    source.map(|source| source as Source)
}

fn restore<E: serde::de::Error>(chain: Vec<String>) -> Result<Source, E> {
    chain_source(chain).ok_or_else(|| E::custom("recorded error chain is empty"))
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
