//! Routine engine — matches events, webhooks, and cron schedules.

use regex::Regex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Routine, RoutineStore, Trigger};

/// Compiled regex cache for event triggers.
struct CompiledPattern {
    routine_id: String,
    regex: Regex,
    channel_filter: Option<String>,
}

/// Engine that evaluates routine triggers.
pub struct RoutineEngine {
    /// Compiled regex patterns for event triggers.
    event_patterns: Vec<CompiledPattern>,
    /// Webhook path → routine ID mapping.
    webhook_paths: HashMap<String, String>,
    /// Concurrent execution counter per routine.
    active_counts: HashMap<String, AtomicU64>,
}

/// Result of checking triggers against an event.
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    pub routine_id: String,
    pub trigger_type: String,
}

impl RoutineEngine {
    /// Build an engine from the current routines in the store.
    pub fn from_store(store: &RoutineStore) -> Self {
        let mut event_patterns = Vec::new();
        let mut webhook_paths = HashMap::new();
        let mut active_counts = HashMap::new();

        for routine in store.list() {
            if !routine.enabled {
                continue;
            }

            active_counts.insert(routine.id.clone(), AtomicU64::new(0));

            match &routine.trigger {
                Trigger::Event { pattern, channel } => {
                    if let Ok(regex) = Regex::new(pattern) {
                        event_patterns.push(CompiledPattern {
                            routine_id: routine.id.clone(),
                            regex,
                            channel_filter: channel.clone(),
                        });
                    }
                }
                Trigger::Webhook { path } => {
                    webhook_paths.insert(path.clone(), routine.id.clone());
                }
                _ => {} // Cron and Manual handled elsewhere
            }
        }

        Self {
            event_patterns,
            webhook_paths,
            active_counts,
        }
    }

    /// Check incoming messages against event triggers.
    ///
    /// Returns matching routine IDs.
    pub fn check_event_triggers(&self, channel: &str, message: &str) -> Vec<TriggerMatch> {
        self.event_patterns
            .iter()
            .filter(|p| {
                // Check channel filter
                if let Some(ref filter) = p.channel_filter {
                    if filter != channel {
                        return false;
                    }
                }
                // Check regex match
                p.regex.is_match(message)
            })
            .map(|p| TriggerMatch {
                routine_id: p.routine_id.clone(),
                trigger_type: "event".to_string(),
            })
            .collect()
    }

    /// Check if an incoming webhook path matches a routine.
    pub fn check_webhook_trigger(&self, path: &str) -> Option<TriggerMatch> {
        self.webhook_paths.get(path).map(|id| TriggerMatch {
            routine_id: id.clone(),
            trigger_type: "webhook".to_string(),
        })
    }

    /// Check which cron-triggered routines are due.
    ///
    /// Returns routine IDs that have cron triggers (actual schedule evaluation
    /// is delegated to the caller using the existing CronSchedule).
    pub fn get_cron_routines<'a>(&self, store: &'a RoutineStore) -> Vec<&'a Routine> {
        store
            .list()
            .iter()
            .filter(|r| r.enabled && matches!(r.trigger, Trigger::Cron { .. }))
            .collect()
    }

    /// Check if a routine can execute (not exceeding max_concurrent).
    pub fn can_execute(&self, routine: &Routine) -> bool {
        match self.active_counts.get(&routine.id) {
            Some(count) => count.load(Ordering::Relaxed) < routine.guardrails.max_concurrent as u64,
            None => true,
        }
    }

    /// Increment the active execution count for a routine.
    pub fn start_execution(&self, routine_id: &str) {
        if let Some(count) = self.active_counts.get(routine_id) {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Decrement the active execution count for a routine.
    pub fn finish_execution(&self, routine_id: &str) {
        if let Some(count) = self.active_counts.get(routine_id) {
            // Saturating subtract to avoid underflow
            let current = count.load(Ordering::Relaxed);
            if current > 0 {
                count.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Get the number of compiled event patterns.
    pub fn event_pattern_count(&self) -> usize {
        self.event_patterns.len()
    }

    /// Get the number of registered webhook paths.
    pub fn webhook_path_count(&self) -> usize {
        self.webhook_paths.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routines::{RoutineAction, RoutineGuardrails};

    fn make_routine(id: &str, trigger: Trigger, enabled: bool) -> Routine {
        Routine {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            trigger,
            action: RoutineAction::Lightweight {
                prompt: "test".to_string(),
            },
            guardrails: RoutineGuardrails::default(),
        }
    }

    fn temp_store_path(suffix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("zeptoclaw_engine_test_{}_{}.json", suffix, line!()))
    }

    #[test]
    fn test_engine_from_empty_store() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_empty_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let store = RoutineStore::new(path.clone());
        let engine = RoutineEngine::from_store(&store);

        assert_eq!(engine.event_pattern_count(), 0);
        assert_eq!(engine.webhook_path_count(), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_event_trigger_match() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_event_match_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "deploy-notifier",
                Trigger::Event {
                    pattern: r"deploy\s+\w+".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);
        let matches = engine.check_event_triggers("telegram", "deploy production");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].routine_id, "deploy-notifier");
        assert_eq!(matches[0].trigger_type, "event");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_event_trigger_no_match() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_event_nomatch_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "deploy-notifier",
                Trigger::Event {
                    pattern: r"deploy\s+\w+".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);
        let matches = engine.check_event_triggers("telegram", "hello world");

        assert!(matches.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_event_trigger_channel_filter() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_chan_filter_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "slack-deploy",
                Trigger::Event {
                    pattern: r"deploy\s+\w+".to_string(),
                    channel: Some("slack".to_string()),
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);

        // Message from wrong channel should not match
        let matches = engine.check_event_triggers("telegram", "deploy production");
        assert!(matches.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_event_trigger_channel_filter_pass() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_chan_pass_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "slack-deploy",
                Trigger::Event {
                    pattern: r"deploy\s+\w+".to_string(),
                    channel: Some("slack".to_string()),
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);

        // Message from correct channel should match
        let matches = engine.check_event_triggers("slack", "deploy staging");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].routine_id, "slack-deploy");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webhook_trigger_match() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_webhook_match_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "gh-webhook",
                Trigger::Webhook {
                    path: "/hooks/github".to_string(),
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);
        let result = engine.check_webhook_trigger("/hooks/github");

        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.routine_id, "gh-webhook");
        assert_eq!(m.trigger_type, "webhook");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webhook_trigger_no_match() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_webhook_nomatch_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "gh-webhook",
                Trigger::Webhook {
                    path: "/hooks/github".to_string(),
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);
        let result = engine.check_webhook_trigger("/hooks/unknown");

        assert!(result.is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cron_routines() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_cron_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "daily-report",
                Trigger::Cron {
                    schedule: "0 9 * * *".to_string(),
                },
                true,
            ))
            .unwrap();
        store
            .add(make_routine(
                "event-handler",
                Trigger::Event {
                    pattern: "test".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();
        store
            .add(make_routine("manual-task", Trigger::Manual, true))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);
        let cron_routines = engine.get_cron_routines(&store);

        assert_eq!(cron_routines.len(), 1);
        assert_eq!(cron_routines[0].id, "daily-report");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_disabled_routines_ignored() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_disabled_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        store
            .add(make_routine(
                "active-event",
                Trigger::Event {
                    pattern: "hello".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();
        store
            .add(make_routine(
                "disabled-event",
                Trigger::Event {
                    pattern: "hello".to_string(),
                    channel: None,
                },
                false,
            ))
            .unwrap();
        store
            .add(make_routine(
                "active-webhook",
                Trigger::Webhook {
                    path: "/hooks/a".to_string(),
                },
                true,
            ))
            .unwrap();
        store
            .add(make_routine(
                "disabled-webhook",
                Trigger::Webhook {
                    path: "/hooks/b".to_string(),
                },
                false,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);

        // Only enabled routines should be compiled
        assert_eq!(engine.event_pattern_count(), 1);
        assert_eq!(engine.webhook_path_count(), 1);

        // Disabled event should not match
        let matches = engine.check_event_triggers("any", "hello world");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].routine_id, "active-event");

        // Disabled webhook should not match
        assert!(engine.check_webhook_trigger("/hooks/b").is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_concurrent_check_allows() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_conc_allow_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let mut routine = make_routine("r1", Trigger::Manual, true);
        routine.guardrails.max_concurrent = 2;
        store.add(routine).unwrap();

        let engine = RoutineEngine::from_store(&store);
        let routine = store.get("r1").unwrap();

        // No executions yet — should allow
        assert!(engine.can_execute(routine));

        // One execution — still below limit of 2
        engine.start_execution("r1");
        assert!(engine.can_execute(routine));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_concurrent_check_blocks() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_conc_block_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let mut routine = make_routine("r1", Trigger::Manual, true);
        routine.guardrails.max_concurrent = 1;
        store.add(routine).unwrap();

        let engine = RoutineEngine::from_store(&store);
        let routine = store.get("r1").unwrap();

        // Start one execution — should hit the limit of 1
        engine.start_execution("r1");
        assert!(!engine.can_execute(routine));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_start_finish_execution() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_start_finish_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let mut routine = make_routine("r1", Trigger::Manual, true);
        routine.guardrails.max_concurrent = 1;
        store.add(routine).unwrap();

        let engine = RoutineEngine::from_store(&store);
        let routine = store.get("r1").unwrap();

        // Start: should block
        engine.start_execution("r1");
        assert!(!engine.can_execute(routine));

        // Finish: should allow again
        engine.finish_execution("r1");
        assert!(engine.can_execute(routine));

        // Double finish should not underflow (stays at 0)
        engine.finish_execution("r1");
        assert!(engine.can_execute(routine));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_invalid_regex_skipped() {
        let path = std::env::temp_dir().join(format!(
            "zeptoclaw_engine_test_invalid_regex_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        // Invalid regex (unclosed group)
        store
            .add(make_routine(
                "bad-regex",
                Trigger::Event {
                    pattern: r"(unclosed".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();
        // Valid regex
        store
            .add(make_routine(
                "good-regex",
                Trigger::Event {
                    pattern: r"hello\s+world".to_string(),
                    channel: None,
                },
                true,
            ))
            .unwrap();

        let engine = RoutineEngine::from_store(&store);

        // Only the valid regex should be compiled
        assert_eq!(engine.event_pattern_count(), 1);

        // The valid pattern should still match
        let matches = engine.check_event_triggers("any", "hello world");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].routine_id, "good-regex");

        let _ = std::fs::remove_file(&path);
    }
}
