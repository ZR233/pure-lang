//! Canonical key validation.

/// Failure returned when a canonical key violates the product contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    Empty,
    TooLong,
    InvalidStart { byte: u8 },
    InvalidCharacter { index: usize, byte: u8 },
    InvalidSeparator { index: usize },
}

/// Validates an already-normalized canonical key.
pub fn validate_key(input: &str) -> Result<(), ValidationError> {
    if input.is_empty() {
        Err(ValidationError::Empty)
    } else {
        Ok(())
    }
}
