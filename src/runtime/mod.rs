//! Container runtime module for ZeptoClaw
//!
//! This module provides container isolation for shell command execution.
//! It supports multiple runtimes:
//! - Native: Direct execution (no isolation, uses application-level security)
//! - Docker: Docker container isolation (Linux, macOS, Windows)
//! - Apple Container: Apple's native container technology (macOS only)

pub mod types;

pub use types::{CommandOutput, ContainerConfig, ContainerRuntime, RuntimeError, RuntimeResult};
