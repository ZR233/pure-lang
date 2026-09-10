//! Injected resource reads with content verification, independent of filesystems and providers.
use super::{ResourceError, ResourceReference};
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Reads retained binary material. The implementation owns storage and physical location policy.
pub trait ResourceReader: Send + Sync + fmt::Debug + 'static {
    /// Reads a complete resource under cancellation; the framework verifies returned bytes.
    fn read(
        &self,
        reference: ResourceReference,
        cancellation: CancellationToken,
    ) -> impl Future<Output = Result<Arc<[u8]>, ResourceReadError>> + Send;
}

/// Resource availability and integrity errors cannot be replaced with empty/default content.
#[derive(Debug, thiserror::Error)]
pub enum ResourceReadError {
    #[error("resource read cancelled")]
    Cancelled,
    #[error("resource content is invalid: {0}")]
    Integrity(#[from] ResourceError),
    #[error("resource is unavailable: {source}")]
    Unavailable {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

trait ErasedReader: Send + Sync + fmt::Debug {
    fn read(
        &self,
        reference: ResourceReference,
        cancellation: CancellationToken,
    ) -> BoxFuture<'_, Result<Arc<[u8]>, ResourceReadError>>;
}
impl<T: ResourceReader> ErasedReader for T {
    fn read(
        &self,
        reference: ResourceReference,
        cancellation: CancellationToken,
    ) -> BoxFuture<'_, Result<Arc<[u8]>, ResourceReadError>> {
        Box::pin(ResourceReader::read(self, reference, cancellation))
    }
}

/// Shareable resource service lease. A prepared model request captures this exact implementation.
#[derive(Debug, Clone)]
pub struct ResourceAccess(Arc<dyn ErasedReader>);
impl ResourceAccess {
    /// Erases an explicitly configured storage reader.
    pub fn new(reader: impl ResourceReader) -> Self {
        Self(Arc::new(reader))
    }

    pub(crate) fn same_service(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Reads and verifies binary material before the model adapter can encode it.
    ///
    /// # Errors
    /// Rejects cancelled reads, unavailable material, and a changed length or digest.
    pub async fn read(
        &self,
        reference: &ResourceReference,
        cancellation: CancellationToken,
    ) -> Result<Arc<[u8]>, ResourceReadError> {
        reference.validate()?;
        if cancellation.is_cancelled() {
            return Err(ResourceReadError::Cancelled);
        }
        let content = self.0.read(reference.clone(), cancellation.clone()).await?;
        if cancellation.is_cancelled() {
            return Err(ResourceReadError::Cancelled);
        }
        reference.verify(&content)?;
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[derive(Debug)]
    struct Bytes(Arc<[u8]>);
    impl ResourceReader for Bytes {
        async fn read(
            &self,
            _: ResourceReference,
            _: CancellationToken,
        ) -> Result<Arc<[u8]>, ResourceReadError> {
            Ok(self.0.clone())
        }
    }
    #[tokio::test]
    async fn injected_reads_preserve_binary_material_and_reject_replaced_content() {
        let bytes: Arc<[u8]> = Arc::from([0, 255, 1, 128]);
        let reference = ResourceReference::new(
            "stable-identity".into(),
            format!("sha256:{:x}", Sha256::digest(&bytes)),
            bytes.len() as u64,
            "application/octet-stream".into(),
        )
        .unwrap();
        let access = ResourceAccess::new(Bytes(bytes.clone()));
        assert_eq!(
            access
                .read(&reference, CancellationToken::new())
                .await
                .unwrap(),
            bytes
        );
        let replaced = ResourceAccess::new(Bytes(Arc::from([0, 255, 1, 129])));
        assert!(matches!(
            replaced.read(&reference, CancellationToken::new()).await,
            Err(ResourceReadError::Integrity(_))
        ));
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            access.read(&reference, cancelled).await,
            Err(ResourceReadError::Cancelled)
        ));
    }
}
