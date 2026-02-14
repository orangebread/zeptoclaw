//! Input content validator for ZeptoClaw.
//!
//! Validates raw input text before it enters the agent processing pipeline.
//! Checks for structural issues (length, encoding) and anomalous patterns
//! (excessive whitespace, repetition, control characters) that may indicate
//! malformed or adversarial input.
//!
//! Designed to be cheap to construct and reuse across many calls -- there
//! is no internal state that changes between invocations.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed input length in bytes (100 KB).
const MAX_INPUT_BYTES: usize = 102_400;

/// Whitespace ratio threshold. If more than this fraction of the input
/// consists of whitespace characters, a warning is emitted.
const WHITESPACE_RATIO_THRESHOLD: f64 = 0.90;

/// Maximum number of consecutive identical characters before a warning is
/// emitted.
const MAX_CONSECUTIVE_REPEATS: usize = 20;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The result of validating a piece of input content.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// `true` if there are no errors (warnings are allowed).
    pub valid: bool,
    /// Non-fatal issues that the caller may want to log.
    pub warnings: Vec<String>,
    /// Fatal issues that should prevent further processing.
    pub errors: Vec<String>,
}

impl ValidationResult {
    /// Create a passing result with no warnings or errors.
    fn ok() -> Self {
        Self {
            valid: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Add an error and mark the result as invalid.
    fn add_error(&mut self, msg: impl Into<String>) {
        self.valid = false;
        self.errors.push(msg.into());
    }

    /// Add a warning (does **not** change `valid`).
    fn add_warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

// ---------------------------------------------------------------------------
// ContentValidator
// ---------------------------------------------------------------------------

/// Validates input content for structural integrity and anomalous patterns.
///
/// Stateless -- construct once and call [`ContentValidator::validate`] as
/// many times as needed.
pub struct ContentValidator {
    max_bytes: usize,
}

impl ContentValidator {
    /// Create a new validator with default limits.
    pub fn new() -> Self {
        Self {
            max_bytes: MAX_INPUT_BYTES,
        }
    }

    /// Validate `input` and return a [`ValidationResult`].
    ///
    /// The result is `valid` if there are zero errors. Warnings are
    /// informational and do not affect validity.
    pub fn validate(&self, input: &str) -> ValidationResult {
        let mut result = ValidationResult::ok();

        self.check_length(input, &mut result);
        self.check_null_bytes(input, &mut result);
        self.check_whitespace_ratio(input, &mut result);
        self.check_repetition(input, &mut result);
        self.check_control_characters(input, &mut result);

        result
    }

    // -- Individual checks -------------------------------------------------

    /// Error if input exceeds the maximum byte length.
    fn check_length(&self, input: &str, result: &mut ValidationResult) {
        if input.len() > self.max_bytes {
            result.add_error(format!(
                "Input exceeds maximum length: {} bytes (limit: {} bytes)",
                input.len(),
                self.max_bytes,
            ));
        }
    }

    /// Error if input contains null bytes (`\0`).
    fn check_null_bytes(&self, input: &str, result: &mut ValidationResult) {
        if input.contains('\0') {
            result.add_error("Input contains null byte(s)");
        }
    }

    /// Warn if more than 90% of characters are whitespace.
    fn check_whitespace_ratio(&self, input: &str, result: &mut ValidationResult) {
        if input.is_empty() {
            return;
        }

        let total = input.chars().count();
        let whitespace = input.chars().filter(|c| c.is_whitespace()).count();
        let ratio = whitespace as f64 / total as f64;

        if ratio > WHITESPACE_RATIO_THRESHOLD {
            result.add_warning(format!(
                "Input is {:.0}% whitespace ({} of {} characters)",
                ratio * 100.0,
                whitespace,
                total,
            ));
        }
    }

    /// Warn if any single character repeats more than `MAX_CONSECUTIVE_REPEATS`
    /// times in a row.
    fn check_repetition(&self, input: &str, result: &mut ValidationResult) {
        let mut chars = input.chars();
        let Some(mut prev) = chars.next() else {
            return;
        };
        let mut run: usize = 1;

        for ch in chars {
            if ch == prev {
                run += 1;
                if run > MAX_CONSECUTIVE_REPEATS {
                    result.add_warning(format!(
                        "Character {:?} repeats {} consecutive times (threshold: {})",
                        prev, run, MAX_CONSECUTIVE_REPEATS,
                    ));
                    // One warning per character is enough -- skip the rest of
                    // this run.
                    break;
                }
            } else {
                prev = ch;
                run = 1;
            }
        }
    }

    /// Warn if input contains unusual ASCII control characters.
    ///
    /// We allow the common whitespace controls (`\t` = 9, `\n` = 10,
    /// `\r` = 13) but flag everything else in the ranges 0-8 and 14-31.
    fn check_control_characters(&self, input: &str, result: &mut ValidationResult) {
        let found: Vec<u8> = input
            .bytes()
            .filter(|&b| is_unusual_control(b))
            .collect::<std::collections::HashSet<u8>>()
            .into_iter()
            .collect();

        if !found.is_empty() {
            result.add_warning(format!(
                "Input contains unusual control character(s): {:?}",
                found,
            ));
        }
    }
}

impl Default for ContentValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` for ASCII control characters that are *not* the common
/// whitespace codes (tab, newline, carriage-return).
fn is_unusual_control(b: u8) -> bool {
    matches!(b, 0..=8 | 14..=31)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn validator() -> ContentValidator {
        ContentValidator::new()
    }

    // -- Length checks -----------------------------------------------------

    #[test]
    fn test_length_under_limit() {
        let input = "a".repeat(1_000);
        let r = validator().validate(&input);
        assert!(r.valid);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_length_exactly_at_limit() {
        let input = "x".repeat(MAX_INPUT_BYTES);
        let r = validator().validate(&input);
        // Exactly at limit should pass (not exceed).
        assert!(r.valid, "exactly at limit should be valid");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_length_over_limit() {
        let input = "y".repeat(MAX_INPUT_BYTES + 1);
        let r = validator().validate(&input);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("exceeds maximum")));
    }

    // -- Null bytes --------------------------------------------------------

    #[test]
    fn test_null_byte_detected() {
        let input = "hello\0world";
        let r = validator().validate(input);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("null byte")));
    }

    #[test]
    fn test_no_null_bytes() {
        let r = validator().validate("hello world");
        assert!(r.valid);
    }

    // -- Whitespace ratio --------------------------------------------------

    #[test]
    fn test_high_whitespace_ratio() {
        // 95 spaces + 5 letters = 95% whitespace > 90% threshold
        let input = format!("{}{}", " ".repeat(95), "abcde");
        let r = validator().validate(&input);
        assert!(r.valid, "whitespace is a warning, not an error");
        assert!(r.warnings.iter().any(|w| w.contains("whitespace")));
    }

    #[test]
    fn test_normal_whitespace_ratio() {
        let input = "The quick brown fox jumps over the lazy dog";
        let r = validator().validate(input);
        assert!(r.valid);
        assert!(
            !r.warnings.iter().any(|w| w.contains("whitespace")),
            "normal text should not trigger whitespace warning"
        );
    }

    // -- Repetition --------------------------------------------------------

    #[test]
    fn test_excessive_repetition() {
        let input = "a".repeat(25); // 25 > 20 threshold
        let r = validator().validate(&input);
        assert!(r.valid, "repetition is a warning, not an error");
        assert!(r.warnings.iter().any(|w| w.contains("repeats")));
    }

    #[test]
    fn test_acceptable_repetition() {
        let input = "a".repeat(20); // exactly 20, not exceeded
        let r = validator().validate(&input);
        assert!(!r.warnings.iter().any(|w| w.contains("repeats")));
    }

    // -- Control characters ------------------------------------------------

    #[test]
    fn test_unusual_control_char() {
        // ASCII 1 (SOH) is unusual
        let input = format!("hello{}world", char::from(1));
        let r = validator().validate(&input);
        assert!(r.valid, "control chars produce warnings, not errors");
        assert!(r.warnings.iter().any(|w| w.contains("control character")));
    }

    #[test]
    fn test_normal_control_chars_allowed() {
        // Tab, newline, carriage-return should NOT trigger a warning
        let input = "line1\n\tindented\r\nline2";
        let r = validator().validate(input);
        assert!(
            !r.warnings.iter().any(|w| w.contains("control character")),
            "common whitespace controls should not trigger warning"
        );
    }

    // -- Clean input -------------------------------------------------------

    #[test]
    fn test_clean_input_passes() {
        let r = validator().validate("Hello, how are you today?");
        assert!(r.valid);
        assert!(r.warnings.is_empty());
        assert!(r.errors.is_empty());
    }

    // -- Empty input -------------------------------------------------------

    #[test]
    fn test_empty_input_passes() {
        let r = validator().validate("");
        assert!(r.valid);
        assert!(r.warnings.is_empty());
        assert!(r.errors.is_empty());
    }

    // -- Multiple issues at once -------------------------------------------

    #[test]
    fn test_multiple_issues() {
        // Null byte (error) + high whitespace (warning) in one input
        let input = format!("{}\0{}", " ".repeat(95), "abcde");
        let r = validator().validate(&input);
        assert!(!r.valid, "null byte should make it invalid");
        assert!(
            !r.warnings.is_empty(),
            "should also have whitespace warning"
        );
        assert!(!r.errors.is_empty(), "should have null byte error");
    }
}
