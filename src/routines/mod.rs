//! Routines — event/webhook/cron triggered automations.
//!
//! Routines extend beyond simple cron jobs by supporting event triggers
//! (regex matching on incoming messages), webhook triggers (HTTP POST
//! path matching), and manual triggers.

pub mod engine;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A routine definition with trigger, action, and guardrails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether this routine is enabled.
    pub enabled: bool,
    /// What triggers the routine.
    pub trigger: Trigger,
    /// What action to take when triggered.
    pub action: RoutineAction,
    /// Guardrails to prevent abuse.
    pub guardrails: RoutineGuardrails,
}

/// What triggers a routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Trigger {
    /// Cron schedule (e.g. "0 9 * * *").
    #[serde(rename = "cron")]
    Cron { schedule: String },
    /// Event matching: regex against incoming messages on a channel.
    #[serde(rename = "event")]
    Event {
        /// Regex pattern to match against messages.
        pattern: String,
        /// Optional channel filter (if None, matches all channels).
        channel: Option<String>,
    },
    /// Webhook: matches an incoming HTTP POST by path.
    #[serde(rename = "webhook")]
    Webhook {
        /// URL path to match (e.g. "/hooks/deploy").
        path: String,
    },
    /// Manual: only triggered via CLI or API.
    #[serde(rename = "manual")]
    Manual,
}

/// What happens when a routine triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RoutineAction {
    /// Lightweight: single LLM call with a prompt (no tool loop).
    #[serde(rename = "lightweight")]
    Lightweight { prompt: String },
    /// Full job: delegates to the agent loop (with tool access).
    #[serde(rename = "full_job")]
    FullJob { prompt: String },
}

/// Guardrails to prevent routine abuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineGuardrails {
    /// Minimum time between executions (in seconds).
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    /// Maximum concurrent executions.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_cooldown() -> u64 {
    60
}
fn default_max_concurrent() -> usize {
    1
}

impl Default for RoutineGuardrails {
    fn default() -> Self {
        Self {
            cooldown_secs: default_cooldown(),
            max_concurrent: default_max_concurrent(),
        }
    }
}

/// Persistent store for routines (JSON file).
pub struct RoutineStore {
    /// Path to the JSON file.
    path: PathBuf,
    /// In-memory cache of routines.
    routines: Vec<Routine>,
    /// Last execution timestamps per routine ID.
    last_executed: HashMap<String, Instant>,
}

impl RoutineStore {
    /// Create a new store backed by the given file.
    pub fn new(path: PathBuf) -> Self {
        let routines = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        Self {
            path,
            routines,
            last_executed: HashMap::new(),
        }
    }

    /// List all routines.
    pub fn list(&self) -> &[Routine] {
        &self.routines
    }

    /// Get a routine by ID.
    pub fn get(&self, id: &str) -> Option<&Routine> {
        self.routines.iter().find(|r| r.id == id)
    }

    /// Add a routine.
    pub fn add(&mut self, routine: Routine) -> Result<(), String> {
        if self.routines.iter().any(|r| r.id == routine.id) {
            return Err(format!("Routine '{}' already exists", routine.id));
        }
        self.routines.push(routine);
        self.save()
    }

    /// Remove a routine by ID.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let len_before = self.routines.len();
        self.routines.retain(|r| r.id != id);
        if self.routines.len() == len_before {
            return Err(format!("Routine '{}' not found", id));
        }
        self.save()
    }

    /// Toggle a routine's enabled state.
    pub fn toggle(&mut self, id: &str) -> Result<bool, String> {
        let routine = self
            .routines
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("Routine '{}' not found", id))?;
        routine.enabled = !routine.enabled;
        let enabled = routine.enabled;
        self.save()?;
        Ok(enabled)
    }

    /// Check if a routine's cooldown has elapsed.
    pub fn check_cooldown(&self, id: &str) -> bool {
        let routine = match self.get(id) {
            Some(r) => r,
            None => return false,
        };

        match self.last_executed.get(id) {
            Some(last) => last.elapsed() >= Duration::from_secs(routine.guardrails.cooldown_secs),
            None => true, // Never executed, cooldown passed
        }
    }

    /// Record an execution timestamp.
    pub fn record_execution(&mut self, id: &str) {
        self.last_executed.insert(id.to_string(), Instant::now());
    }

    /// Count of routines.
    pub fn len(&self) -> usize {
        self.routines.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.routines.is_empty()
    }

    /// Save routines to disk.
    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
        let json = serde_json::to_string_pretty(&self.routines)
            .map_err(|e| format!("Failed to serialize routines: {}", e))?;
        std::fs::write(&self.path, json)
            .map_err(|e| format!("Failed to write routines file: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_routine(id: &str, trigger: Trigger, action: RoutineAction) -> Routine {
        Routine {
            id: id.to_string(),
            name: format!("Test {}", id),
            enabled: true,
            trigger,
            action,
            guardrails: RoutineGuardrails::default(),
        }
    }

    fn temp_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zeptoclaw_test_routines_{}_{}.json",
            suffix,
            std::process::id()
        ))
    }

    // --- Serde roundtrip tests ---

    #[test]
    fn test_trigger_cron_serde() {
        let trigger = Trigger::Cron {
            schedule: "0 9 * * *".to_string(),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        let parsed: Trigger = serde_json::from_str(&json).unwrap();
        match parsed {
            Trigger::Cron { schedule } => assert_eq!(schedule, "0 9 * * *"),
            _ => panic!("Expected Trigger::Cron"),
        }
    }

    #[test]
    fn test_trigger_event_serde() {
        let trigger = Trigger::Event {
            pattern: r"deploy\s+\w+".to_string(),
            channel: Some("telegram".to_string()),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        let parsed: Trigger = serde_json::from_str(&json).unwrap();
        match parsed {
            Trigger::Event { pattern, channel } => {
                assert_eq!(pattern, r"deploy\s+\w+");
                assert_eq!(channel, Some("telegram".to_string()));
            }
            _ => panic!("Expected Trigger::Event"),
        }
    }

    #[test]
    fn test_trigger_webhook_serde() {
        let trigger = Trigger::Webhook {
            path: "/hooks/deploy".to_string(),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        let parsed: Trigger = serde_json::from_str(&json).unwrap();
        match parsed {
            Trigger::Webhook { path } => assert_eq!(path, "/hooks/deploy"),
            _ => panic!("Expected Trigger::Webhook"),
        }
    }

    #[test]
    fn test_trigger_manual_serde() {
        let trigger = Trigger::Manual;
        let json = serde_json::to_string(&trigger).unwrap();
        let parsed: Trigger = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Trigger::Manual));
    }

    #[test]
    fn test_action_lightweight_serde() {
        let action = RoutineAction::Lightweight {
            prompt: "Summarize today's logs".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let parsed: RoutineAction = serde_json::from_str(&json).unwrap();
        match parsed {
            RoutineAction::Lightweight { prompt } => {
                assert_eq!(prompt, "Summarize today's logs");
            }
            _ => panic!("Expected RoutineAction::Lightweight"),
        }
    }

    #[test]
    fn test_action_full_job_serde() {
        let action = RoutineAction::FullJob {
            prompt: "Run the deployment pipeline".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let parsed: RoutineAction = serde_json::from_str(&json).unwrap();
        match parsed {
            RoutineAction::FullJob { prompt } => {
                assert_eq!(prompt, "Run the deployment pipeline");
            }
            _ => panic!("Expected RoutineAction::FullJob"),
        }
    }

    #[test]
    fn test_guardrails_defaults() {
        let guardrails = RoutineGuardrails::default();
        assert_eq!(guardrails.cooldown_secs, 60);
        assert_eq!(guardrails.max_concurrent, 1);
    }

    // --- Store tests ---

    #[test]
    fn test_store_add_and_list() {
        let path = temp_path("add_list");
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        assert!(store.is_empty());

        let routine = make_routine(
            "r1",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        store.add(routine).unwrap();

        assert_eq!(store.len(), 1);
        assert_eq!(store.list()[0].id, "r1");
        assert_eq!(store.get("r1").unwrap().name, "Test r1");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_remove() {
        let path = temp_path("remove");
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let routine = make_routine(
            "r1",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        store.add(routine).unwrap();
        assert_eq!(store.len(), 1);

        store.remove("r1").unwrap();
        assert!(store.is_empty());

        // Removing non-existent should error
        let err = store.remove("r1").unwrap_err();
        assert!(err.contains("not found"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_toggle() {
        let path = temp_path("toggle");
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let routine = make_routine(
            "r1",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        store.add(routine).unwrap();

        // Initially enabled
        assert!(store.get("r1").unwrap().enabled);

        // Toggle off
        let enabled = store.toggle("r1").unwrap();
        assert!(!enabled);
        assert!(!store.get("r1").unwrap().enabled);

        // Toggle on
        let enabled = store.toggle("r1").unwrap();
        assert!(enabled);
        assert!(store.get("r1").unwrap().enabled);

        // Toggle non-existent should error
        let err = store.toggle("nonexistent").unwrap_err();
        assert!(err.contains("not found"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_duplicate_id_error() {
        let path = temp_path("duplicate");
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());
        let routine = make_routine(
            "r1",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        store.add(routine.clone()).unwrap();

        let err = store.add(routine).unwrap_err();
        assert!(err.contains("already exists"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_persistence_roundtrip() {
        let path = temp_path("persistence");
        let _ = std::fs::remove_file(&path);

        // Create store, add routines, drop it
        {
            let mut store = RoutineStore::new(path.clone());
            store
                .add(make_routine(
                    "r1",
                    Trigger::Cron {
                        schedule: "0 9 * * *".to_string(),
                    },
                    RoutineAction::FullJob {
                        prompt: "daily report".to_string(),
                    },
                ))
                .unwrap();
            store
                .add(make_routine(
                    "r2",
                    Trigger::Webhook {
                        path: "/hooks/deploy".to_string(),
                    },
                    RoutineAction::Lightweight {
                        prompt: "notify deploy".to_string(),
                    },
                ))
                .unwrap();
        }

        // Load from same file
        let store = RoutineStore::new(path.clone());
        assert_eq!(store.len(), 2);
        assert_eq!(store.get("r1").unwrap().name, "Test r1");
        assert_eq!(store.get("r2").unwrap().name, "Test r2");

        match &store.get("r1").unwrap().trigger {
            Trigger::Cron { schedule } => assert_eq!(schedule, "0 9 * * *"),
            _ => panic!("Expected Trigger::Cron"),
        }
        match &store.get("r2").unwrap().action {
            RoutineAction::Lightweight { prompt } => assert_eq!(prompt, "notify deploy"),
            _ => panic!("Expected RoutineAction::Lightweight"),
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cooldown_enforcement() {
        let path = temp_path("cooldown");
        let _ = std::fs::remove_file(&path);

        let mut store = RoutineStore::new(path.clone());

        // Add routine with 0s cooldown for testability
        let mut routine = make_routine(
            "r1",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        routine.guardrails.cooldown_secs = 0;
        store.add(routine).unwrap();

        // Never executed — cooldown should pass
        assert!(store.check_cooldown("r1"));

        // Record execution
        store.record_execution("r1");

        // With 0s cooldown, should still pass immediately
        assert!(store.check_cooldown("r1"));

        // Non-existent routine should return false
        assert!(!store.check_cooldown("nonexistent"));

        // Now add a routine with a large cooldown
        let mut routine2 = make_routine(
            "r2",
            Trigger::Manual,
            RoutineAction::Lightweight {
                prompt: "hello".to_string(),
            },
        );
        routine2.guardrails.cooldown_secs = 3600; // 1 hour
        store.add(routine2).unwrap();

        // Never executed — should pass
        assert!(store.check_cooldown("r2"));

        // Record execution, then check — should NOT pass (1 hour hasn't elapsed)
        store.record_execution("r2");
        assert!(!store.check_cooldown("r2"));

        let _ = std::fs::remove_file(&path);
    }
}
