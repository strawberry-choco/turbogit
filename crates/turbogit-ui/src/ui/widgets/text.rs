//! General-purpose text utilities used by commit presentations.

/// Default number of Unicode characters retained from a commit reference.
pub const SHORT_COMMIT_REF_CHARS: usize = 7;

/// Format a commit reference for compact display without splitting a Unicode
/// scalar value. References at or below the default length pass through intact.
pub fn short_commit_ref(reference: &str) -> String {
    reference.chars().take(SHORT_COMMIT_REF_CHARS).collect()
}
