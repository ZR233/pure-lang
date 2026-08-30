//! External label normalization.

/// Failure returned while converting an external label to a canonical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeError {
    Empty,
    TooLong,
    InvalidCharacter { index: usize, byte: u8 },
}

/// Converts an external label into a canonical key.
pub fn normalize_key(_input: &str) -> Result<String, NormalizeError> {
    Err(NormalizeError::Empty)
}
