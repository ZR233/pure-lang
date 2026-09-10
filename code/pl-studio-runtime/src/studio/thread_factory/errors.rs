//! Resource errors retain their underlying source across the product boundary.
pub(super) fn resource_error(
    operation: &'static str,
    source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> crate::thread_assembler::ThreadAssemblyError {
    crate::thread_assembler::ThreadAssemblyError::Resource {
        operation,
        source: source.into(),
    }
}
