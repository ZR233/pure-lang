# Canonical key product contract

External labels are normalized into stable canonical keys before validation. Canonical keys
are portable ASCII identifiers, have a 48-byte maximum, start with a lowercase letter, and
use only isolated internal hyphens as separators. Callers must be able to distinguish empty,
length, position, separator, and character failures without parsing error strings.

Normalization discards leading and trailing runs of ASCII whitespace, `_`, and `-`, while
collapsing each internal run of those bytes to one hyphen. Invalid bytes report their original
input byte index before normalized empty and length checks. Every successful normalized value
therefore satisfies the validator's structural separator boundary.
