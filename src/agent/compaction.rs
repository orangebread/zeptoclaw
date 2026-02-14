//! Context compaction strategies for conversation history.
//!
//! Provides two strategies for reducing conversation history size:
//!
//! - **Truncate**: Drop old messages, keeping only the N most recent.
//!   Always preserves the first system message if present.
//! - **Summarize**: Replace old messages with a single summary message,
//!   keeping the N most recent messages intact.
//!
//! These are pure functions that operate on `Vec<Message>`. The caller
//! is responsible for obtaining any LLM-generated summaries before
//! calling `summarize_messages`.

use crate::session::{Message, Role};

/// Truncate messages to keep only the N most recent.
///
/// Always preserves the first system message if present. When the first
/// message has `role == System`, the result contains that system message
/// plus the `keep_recent` most recent non-system-prefix messages.
///
/// # Arguments
/// * `messages` - The full conversation history
/// * `keep_recent` - How many recent messages to keep
///
/// # Returns
/// A truncated message list of at most `keep_recent` messages (plus the
/// leading system message, if preserved).
///
/// # Examples
/// ```
/// use zeptoclaw::session::Message;
/// use zeptoclaw::agent::compaction::truncate_messages;
///
/// let msgs = vec![
///     Message::system("You are helpful."),
///     Message::user("Hi"),
///     Message::assistant("Hello!"),
///     Message::user("How are you?"),
///     Message::assistant("Great!"),
/// ];
/// let result = truncate_messages(msgs, 2);
/// assert_eq!(result.len(), 3); // system + 2 recent
/// ```
pub fn truncate_messages(messages: Vec<Message>, keep_recent: usize) -> Vec<Message> {
    if messages.len() <= keep_recent {
        return messages;
    }

    if keep_recent == 0 {
        // Preserve system message even when keep_recent is 0
        if let Some(first) = messages.first() {
            if first.role == Role::System {
                return vec![messages.into_iter().next().unwrap()];
            }
        }
        return Vec::new();
    }

    let has_system_prefix = messages
        .first()
        .map(|m| m.role == Role::System)
        .unwrap_or(false);

    if has_system_prefix {
        let total = messages.len();
        // System message + the last `keep_recent` messages from the rest
        let skip = (total - 1).saturating_sub(keep_recent);
        let mut result = Vec::with_capacity(1 + keep_recent);
        let mut iter = messages.into_iter();
        result.push(iter.next().unwrap()); // system message
                                           // Skip old non-system messages
        for msg in iter.skip(skip) {
            result.push(msg);
        }
        result
    } else {
        // No system prefix — just keep the tail
        let skip = messages.len() - keep_recent;
        messages.into_iter().skip(skip).collect()
    }
}

/// Summarize old messages into a single summary message, keeping the most
/// recent messages intact.
///
/// Splits the conversation into "old" (to be summarized) and "recent" (to
/// keep). The old messages are replaced with a single system message
/// containing the summary text. If the first message is a system message,
/// it is preserved before the summary.
///
/// # Arguments
/// * `messages` - The full conversation history
/// * `keep_recent` - How many recent messages to keep verbatim
/// * `summary_text` - An LLM-generated summary of the old messages
///
/// # Returns
/// A compacted message list: `[system_msg?, summary_msg, ...recent_msgs]`
///
/// # Examples
/// ```
/// use zeptoclaw::session::Message;
/// use zeptoclaw::agent::compaction::summarize_messages;
///
/// let msgs = vec![
///     Message::system("You are helpful."),
///     Message::user("Tell me about Rust"),
///     Message::assistant("Rust is a systems language..."),
///     Message::user("What about async?"),
///     Message::assistant("Async in Rust uses tokio..."),
/// ];
/// let result = summarize_messages(msgs, 2, "User asked about Rust and async.");
/// assert_eq!(result.len(), 4); // system + summary + 2 recent
/// ```
pub fn summarize_messages(
    messages: Vec<Message>,
    keep_recent: usize,
    summary_text: &str,
) -> Vec<Message> {
    if messages.is_empty() {
        return vec![Message::system(&format!(
            "[Conversation Summary]\n{}",
            summary_text
        ))];
    }

    if messages.len() <= keep_recent {
        // Nothing to summarize — everything is "recent"
        return messages;
    }

    let has_system_prefix = messages
        .first()
        .map(|m| m.role == Role::System)
        .unwrap_or(false);

    let summary_msg = Message::system(&format!("[Conversation Summary]\n{}", summary_text));

    if has_system_prefix {
        let total = messages.len();
        // recent = last `keep_recent` messages (excluding system prefix)
        let skip = (total - 1).saturating_sub(keep_recent);
        let mut result = Vec::with_capacity(2 + keep_recent);
        let mut iter = messages.into_iter();
        result.push(iter.next().unwrap()); // original system message
        result.push(summary_msg);
        for msg in iter.skip(skip) {
            result.push(msg);
        }
        result
    } else {
        let total = messages.len();
        let skip = total - keep_recent;
        let mut result = Vec::with_capacity(1 + keep_recent);
        result.push(summary_msg);
        for msg in messages.into_iter().skip(skip) {
            result.push(msg);
        }
        result
    }
}

/// Build a prompt asking an LLM to summarize a set of messages.
///
/// Formats the messages into a human-readable transcript and appends
/// instructions for producing a concise summary.
///
/// # Arguments
/// * `messages` - The messages to summarize
///
/// # Returns
/// A prompt string suitable for sending to an LLM.
///
/// # Examples
/// ```
/// use zeptoclaw::session::Message;
/// use zeptoclaw::agent::compaction::build_summary_prompt;
///
/// let msgs = vec![
///     Message::user("Hello"),
///     Message::assistant("Hi there!"),
/// ];
/// let prompt = build_summary_prompt(&msgs);
/// assert!(prompt.contains("user: Hello"));
/// assert!(prompt.contains("assistant: Hi there!"));
/// ```
pub fn build_summary_prompt(messages: &[Message]) -> String {
    let mut transcript = String::new();
    for msg in messages {
        transcript.push_str(&format!("{}: {}\n", msg.role, msg.content));
    }

    format!(
        "Summarize the following conversation focusing on key decisions, \
         information exchanged, and actions taken. Be concise.\n\n{}",
        transcript
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── truncate_messages ──────────────────────────────────────────────

    #[test]
    fn test_truncate_keeps_n_recent() {
        let msgs = vec![
            Message::user("one"),
            Message::user("two"),
            Message::user("three"),
            Message::user("four"),
        ];
        let result = truncate_messages(msgs, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "three");
        assert_eq!(result[1].content, "four");
    }

    #[test]
    fn test_truncate_preserves_system_message() {
        let msgs = vec![
            Message::system("system prompt"),
            Message::user("one"),
            Message::user("two"),
            Message::user("three"),
        ];
        let result = truncate_messages(msgs, 2);
        assert_eq!(result.len(), 3); // system + 2 recent
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "system prompt");
        assert_eq!(result[1].content, "two");
        assert_eq!(result[2].content, "three");
    }

    #[test]
    fn test_truncate_empty_messages() {
        let result = truncate_messages(Vec::new(), 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_truncate_keep_greater_than_len() {
        let msgs = vec![Message::user("one"), Message::user("two")];
        let result = truncate_messages(msgs, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "one");
        assert_eq!(result[1].content, "two");
    }

    #[test]
    fn test_truncate_keep_equal_to_len() {
        let msgs = vec![
            Message::user("one"),
            Message::user("two"),
            Message::user("three"),
        ];
        let result = truncate_messages(msgs, 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_truncate_keep_zero() {
        let msgs = vec![Message::user("one"), Message::user("two")];
        let result = truncate_messages(msgs, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_truncate_keep_zero_with_system() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("one"),
            Message::user("two"),
        ];
        let result = truncate_messages(msgs, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "sys");
    }

    #[test]
    fn test_truncate_single_message() {
        let msgs = vec![Message::user("only")];
        let result = truncate_messages(msgs, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "only");
    }

    // ── summarize_messages ─────────────────────────────────────────────

    #[test]
    fn test_summarize_with_system_message() {
        let msgs = vec![
            Message::system("You are helpful."),
            Message::user("Tell me about Rust"),
            Message::assistant("Rust is great."),
            Message::user("And async?"),
            Message::assistant("Use tokio."),
        ];
        let result = summarize_messages(msgs, 2, "Discussed Rust basics.");
        // system + summary + 2 recent
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "You are helpful.");
        assert_eq!(result[1].role, Role::System);
        assert!(result[1].content.contains("[Conversation Summary]"));
        assert!(result[1].content.contains("Discussed Rust basics."));
        assert_eq!(result[2].content, "And async?");
        assert_eq!(result[3].content, "Use tokio.");
    }

    #[test]
    fn test_summarize_without_system_message() {
        let msgs = vec![
            Message::user("Hello"),
            Message::assistant("Hi!"),
            Message::user("Bye"),
            Message::assistant("Goodbye!"),
        ];
        let result = summarize_messages(msgs, 2, "User greeted.");
        // summary + 2 recent
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, Role::System);
        assert!(result[0].content.contains("[Conversation Summary]"));
        assert!(result[0].content.contains("User greeted."));
        assert_eq!(result[1].content, "Bye");
        assert_eq!(result[2].content, "Goodbye!");
    }

    #[test]
    fn test_summarize_empty_messages() {
        let result = summarize_messages(Vec::new(), 2, "Nothing happened.");
        assert_eq!(result.len(), 1);
        assert!(result[0].content.contains("[Conversation Summary]"));
        assert!(result[0].content.contains("Nothing happened."));
    }

    #[test]
    fn test_summarize_keep_greater_than_len() {
        let msgs = vec![Message::user("one"), Message::user("two")];
        let result = summarize_messages(msgs, 10, "summary");
        // Nothing to summarize — all messages are "recent"
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "one");
        assert_eq!(result[1].content, "two");
    }

    // ── build_summary_prompt ───────────────────────────────────────────

    #[test]
    fn test_build_summary_prompt_includes_content() {
        let msgs = vec![
            Message::user("What is Rust?"),
            Message::assistant("A systems programming language."),
        ];
        let prompt = build_summary_prompt(&msgs);
        assert!(prompt.contains("What is Rust?"));
        assert!(prompt.contains("A systems programming language."));
    }

    #[test]
    fn test_build_summary_prompt_includes_role_labels() {
        let msgs = vec![
            Message::user("Hi"),
            Message::assistant("Hello"),
            Message::system("Be concise"),
        ];
        let prompt = build_summary_prompt(&msgs);
        assert!(prompt.contains("user: Hi"));
        assert!(prompt.contains("assistant: Hello"));
        assert!(prompt.contains("system: Be concise"));
    }

    #[test]
    fn test_build_summary_prompt_includes_instruction() {
        let msgs = vec![Message::user("test")];
        let prompt = build_summary_prompt(&msgs);
        assert!(prompt.contains("Summarize the following conversation"));
        assert!(prompt.contains("key decisions"));
        assert!(prompt.contains("Be concise"));
    }

    #[test]
    fn test_build_summary_prompt_empty_messages() {
        let prompt = build_summary_prompt(&[]);
        assert!(prompt.contains("Summarize the following conversation"));
        // No message content, but prompt itself is still valid
        assert!(!prompt.contains("user:"));
    }
}
