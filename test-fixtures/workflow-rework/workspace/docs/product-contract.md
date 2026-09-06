# External key normalization

`normalize_key(&str) -> Result<String, NormalizeError>` lowercases ASCII letters,
preserves ASCII digits, trims leading/trailing separator runs and collapses internal
runs to one hyphen. Separators are `_`, `-`, space and ASCII bytes 0x09 through 0x0d
(including vertical tab).

Reject all other bytes using their original UTF-8 byte offset and byte value.
Scan invalid input before reporting Empty or TooLong. Return Empty if normalization
leaves no characters, and TooLong if the normalized output exceeds 48 bytes.
Do not change the public function or error variants and do not add dependencies.
