//! Integration tests for PicoClaw
//!
//! These tests verify that the various components work together correctly,
//! testing the full message flow, tool execution, session persistence, and
//! configuration handling.

use picoclaw::{
    bus::{InboundMessage, MessageBus, OutboundMessage},
    config::Config,
    session::{Message, SessionManager},
    tools::{EchoTool, ToolContext, ToolRegistry},
};
use std::sync::Arc;

// ============================================================================
// Message Bus Integration Tests
// ============================================================================

#[tokio::test]
async fn test_message_flow() {
    let bus = Arc::new(MessageBus::new());
    let msg = InboundMessage::new("test", "user1", "chat1", "Hello");
    bus.publish_inbound(msg).await.unwrap();
    let received = bus.consume_inbound().await.unwrap();
    assert_eq!(received.content, "Hello");
    assert_eq!(received.channel, "test");
}

#[tokio::test]
async fn test_message_bus_roundtrip() {
    let bus = MessageBus::new();

    // Simulate a message from a channel
    let inbound = InboundMessage::new("telegram", "user123", "chat456", "Hello bot!");
    bus.publish_inbound(inbound).await.unwrap();

    // Agent receives the message
    let received = bus.consume_inbound().await.unwrap();
    assert_eq!(received.content, "Hello bot!");
    assert_eq!(received.session_key, "telegram:chat456");

    // Agent sends a response
    let response = OutboundMessage::reply_to(&received, "Hello human!");
    bus.publish_outbound(response).await.unwrap();

    // Channel receives the response
    let outgoing = bus.consume_outbound().await.unwrap();
    assert_eq!(outgoing.content, "Hello human!");
    assert_eq!(outgoing.channel, "telegram");
    assert_eq!(outgoing.chat_id, "chat456");
}

#[tokio::test]
async fn test_concurrent_message_producers() {
    let bus = Arc::new(MessageBus::new());
    let mut handles = vec![];

    // Spawn multiple producers (simulating multiple channels)
    for channel in ["telegram", "discord", "slack"] {
        let bus_clone = Arc::clone(&bus);
        let channel = channel.to_string();
        let handle = tokio::spawn(async move {
            for i in 0..5 {
                let msg = InboundMessage::new(&channel, "user", "chat", &format!("{}:{}", channel, i));
                bus_clone.publish_inbound(msg).await.unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all producers to finish
    for handle in handles {
        handle.await.unwrap();
    }

    // Consume all messages
    let mut count = 0;
    while count < 15 {
        if bus.consume_inbound().await.is_some() {
            count += 1;
        }
    }
    assert_eq!(count, 15);
}

// ============================================================================
// Tool Execution Integration Tests
// ============================================================================

#[tokio::test]
async fn test_tool_execution() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let result = registry.execute("echo", serde_json::json!({"message": "test"})).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "test");
}

#[tokio::test]
async fn test_tool_registry_multiple_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));

    // Verify tool is registered
    assert!(registry.has("echo"));
    assert_eq!(registry.len(), 1);

    // Execute the tool
    let result = registry.execute("echo", serde_json::json!({"message": "Hello, World!"})).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello, World!");

    // Verify non-existent tool returns error
    let missing = registry.execute("nonexistent", serde_json::json!({})).await;
    assert!(missing.is_err());
}

#[tokio::test]
async fn test_tool_execution_with_context() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));

    let ctx = ToolContext::new()
        .with_channel("telegram", "12345")
        .with_workspace("/tmp/test_workspace");

    let result = registry
        .execute_with_context("echo", serde_json::json!({"message": "Context test"}), &ctx)
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Context test");
}

#[tokio::test]
async fn test_tool_definitions_for_llm() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));

    let definitions = registry.definitions();
    assert_eq!(definitions.len(), 1);

    let echo_def = &definitions[0];
    assert_eq!(echo_def.name, "echo");
    assert!(!echo_def.description.is_empty());
    assert!(echo_def.parameters.is_object());
    assert!(echo_def.parameters["properties"]["message"].is_object());
}

// ============================================================================
// Session Persistence Integration Tests
// ============================================================================

#[tokio::test]
async fn test_session_persistence() {
    let manager = SessionManager::new_memory();
    let mut session = manager.get_or_create("test-key").await.unwrap();
    session.add_message(Message::user("Hello"));
    manager.save(&session).await.unwrap();
    let loaded = manager.get_or_create("test-key").await.unwrap();
    assert_eq!(loaded.messages.len(), 1);
}

#[tokio::test]
async fn test_session_full_conversation() {
    let manager = SessionManager::new_memory();

    // Simulate a full conversation
    let mut session = manager.get_or_create("telegram:chat123").await.unwrap();

    // Add system prompt
    session.add_message(Message::system("You are a helpful assistant."));

    // User asks a question
    session.add_message(Message::user("What is Rust?"));

    // Assistant responds
    session.add_message(Message::assistant("Rust is a systems programming language focused on safety and performance."));

    // User follows up
    session.add_message(Message::user("What are its main features?"));

    // Save session
    manager.save(&session).await.unwrap();

    // Load and verify
    let loaded = manager.get_or_create("telegram:chat123").await.unwrap();
    assert_eq!(loaded.messages.len(), 4);
    assert_eq!(loaded.messages[0].role, picoclaw::session::Role::System);
    assert_eq!(loaded.messages[1].role, picoclaw::session::Role::User);
    assert_eq!(loaded.messages[2].role, picoclaw::session::Role::Assistant);
    assert_eq!(loaded.messages[3].role, picoclaw::session::Role::User);
}

#[tokio::test]
async fn test_session_with_tool_calls() {
    let manager = SessionManager::new_memory();
    let mut session = manager.get_or_create("tool-session").await.unwrap();

    // User message
    session.add_message(Message::user("Echo this: Hello World"));

    // Assistant with tool call
    let tool_call = picoclaw::session::ToolCall::new("call_1", "echo", r#"{"message": "Hello World"}"#);
    session.add_message(Message::assistant_with_tools("Let me echo that.", vec![tool_call]));

    // Tool result
    session.add_message(Message::tool_result("call_1", "Hello World"));

    // Final assistant response
    session.add_message(Message::assistant("I echoed your message: Hello World"));

    manager.save(&session).await.unwrap();

    // Verify
    let loaded = manager.get_or_create("tool-session").await.unwrap();
    assert_eq!(loaded.messages.len(), 4);
    assert!(loaded.messages[1].has_tool_calls());
    assert!(loaded.messages[2].is_tool_result());
}

#[tokio::test]
async fn test_session_isolation() {
    let manager = SessionManager::new_memory();

    // Create two separate sessions
    let mut session1 = manager.get_or_create("user1:chat1").await.unwrap();
    let mut session2 = manager.get_or_create("user2:chat2").await.unwrap();

    session1.add_message(Message::user("Message from user 1"));
    session2.add_message(Message::user("Message from user 2"));

    manager.save(&session1).await.unwrap();
    manager.save(&session2).await.unwrap();

    // Verify isolation
    let loaded1 = manager.get_or_create("user1:chat1").await.unwrap();
    let loaded2 = manager.get_or_create("user2:chat2").await.unwrap();

    assert_eq!(loaded1.messages.len(), 1);
    assert_eq!(loaded1.messages[0].content, "Message from user 1");

    assert_eq!(loaded2.messages.len(), 1);
    assert_eq!(loaded2.messages[0].content, "Message from user 2");
}

#[tokio::test]
async fn test_session_manager_concurrent_access() {
    let manager = Arc::new(SessionManager::new_memory());
    let mut handles = vec![];

    for i in 0..5 {
        let manager_clone = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            let mut session = manager_clone.get_or_create(&format!("concurrent-{}", i)).await.unwrap();
            session.add_message(Message::user(&format!("Message {}", i)));
            manager_clone.save(&session).await.unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all sessions were created
    let keys = manager.list().await.unwrap();
    assert_eq!(keys.len(), 5);
}

// ============================================================================
// Configuration Integration Tests
// ============================================================================

#[test]
fn test_config_serialization() {
    let config = Config::default();
    let json = serde_json::to_string(&config).unwrap();
    let parsed: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(config.agents.defaults.model, parsed.agents.defaults.model);
}

#[test]
fn test_config_partial_override() {
    // Test that partial JSON properly inherits defaults
    let json = r#"{"agents": {"defaults": {"model": "gpt-4"}}}"#;
    let config: Config = serde_json::from_str(json).unwrap();

    assert_eq!(config.agents.defaults.model, "gpt-4");
    // Other defaults should still be set
    assert_eq!(config.agents.defaults.max_tokens, 8096);
    assert_eq!(config.agents.defaults.temperature, 0.7);
    assert_eq!(config.gateway.port, 8080);
}

#[test]
fn test_config_all_fields() {
    let json = r#"{
        "agents": {
            "defaults": {
                "model": "claude-sonnet-4-5-20250929",
                "max_tokens": 4096,
                "temperature": 0.5,
                "max_tool_iterations": 10,
                "workspace": "/custom/workspace"
            }
        },
        "gateway": {
            "host": "127.0.0.1",
            "port": 9000
        }
    }"#;

    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(config.agents.defaults.model, "claude-sonnet-4-5-20250929");
    assert_eq!(config.agents.defaults.max_tokens, 4096);
    assert_eq!(config.agents.defaults.temperature, 0.5);
    assert_eq!(config.agents.defaults.max_tool_iterations, 10);
    assert_eq!(config.agents.defaults.workspace, "/custom/workspace");
    assert_eq!(config.gateway.host, "127.0.0.1");
    assert_eq!(config.gateway.port, 9000);
}

#[test]
fn test_config_provider_settings() {
    let json = r#"{
        "providers": {
            "anthropic": {"api_key": "sk-ant-test"},
            "openai": {"api_key": "sk-test", "api_base": "https://api.openai.com/v1"}
        }
    }"#;

    let config: Config = serde_json::from_str(json).unwrap();

    let anthropic = config.providers.anthropic.as_ref().unwrap();
    assert_eq!(anthropic.api_key, Some("sk-ant-test".to_string()));

    let openai = config.providers.openai.as_ref().unwrap();
    assert_eq!(openai.api_key, Some("sk-test".to_string()));
    assert_eq!(openai.api_base, Some("https://api.openai.com/v1".to_string()));
}

#[test]
fn test_config_channel_settings() {
    let json = r#"{
        "channels": {
            "telegram": {
                "enabled": true,
                "token": "123456:ABC-DEF",
                "allow_from": ["user1", "user2"]
            }
        }
    }"#;

    let config: Config = serde_json::from_str(json).unwrap();

    let telegram = config.channels.telegram.as_ref().unwrap();
    assert!(telegram.enabled);
    assert_eq!(telegram.token, "123456:ABC-DEF");
    assert_eq!(telegram.allow_from, vec!["user1", "user2"]);
}

// ============================================================================
// End-to-End Integration Tests
// ============================================================================

#[tokio::test]
async fn test_message_to_session_flow() {
    // Simulates the flow: Channel -> MessageBus -> Session
    let bus = MessageBus::new();
    let session_manager = SessionManager::new_memory();

    // Channel publishes a message
    let inbound = InboundMessage::new("telegram", "user123", "chat456", "Hello!");
    bus.publish_inbound(inbound).await.unwrap();

    // Agent receives from bus
    let received = bus.consume_inbound().await.unwrap();

    // Agent adds to session
    let mut session = session_manager.get_or_create(&received.session_key).await.unwrap();
    session.add_message(Message::user(&received.content));

    // Simulate AI response
    let response_text = "Hello! How can I help you?";
    session.add_message(Message::assistant(response_text));
    session_manager.save(&session).await.unwrap();

    // Agent publishes response to bus
    let outbound = OutboundMessage::new(&received.channel, &received.chat_id, response_text);
    bus.publish_outbound(outbound).await.unwrap();

    // Verify full flow
    let saved_session = session_manager.get_or_create("telegram:chat456").await.unwrap();
    assert_eq!(saved_session.messages.len(), 2);

    let outgoing = bus.consume_outbound().await.unwrap();
    assert_eq!(outgoing.content, "Hello! How can I help you?");
}

#[tokio::test]
async fn test_tool_call_flow() {
    // Simulates: User message -> Tool call -> Tool result -> Response
    let session_manager = SessionManager::new_memory();
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(Box::new(EchoTool));

    let mut session = session_manager.get_or_create("test-flow").await.unwrap();

    // User request
    session.add_message(Message::user("Echo this: Test message"));

    // Simulate LLM deciding to call a tool
    let tool_call = picoclaw::session::ToolCall::new("call_001", "echo", r#"{"message": "Test message"}"#);
    session.add_message(Message::assistant_with_tools("I'll echo that for you.", vec![tool_call.clone()]));

    // Execute the tool
    let args: serde_json::Value = tool_call.parse_arguments().unwrap();
    let tool_result = tool_registry.execute("echo", args).await.unwrap();
    assert_eq!(tool_result, "Test message");

    // Add tool result to session
    session.add_message(Message::tool_result(&tool_call.id, &tool_result));

    // Final response
    session.add_message(Message::assistant("I echoed your message: Test message"));

    session_manager.save(&session).await.unwrap();

    // Verify complete conversation
    let loaded = session_manager.get_or_create("test-flow").await.unwrap();
    assert_eq!(loaded.messages.len(), 4);
    assert!(loaded.messages[1].has_tool_calls());
    assert_eq!(loaded.messages[2].content, "Test message");
    assert!(loaded.messages[3].content.contains("echoed"));
}

#[tokio::test]
async fn test_multi_channel_sessions() {
    // Test that sessions from different channels are isolated
    let session_manager = SessionManager::new_memory();

    let channels = ["telegram", "discord", "slack"];
    for channel in channels {
        let session_key = format!("{}:user1:chat1", channel);
        let mut session = session_manager.get_or_create(&session_key).await.unwrap();
        session.add_message(Message::user(&format!("Hello from {}", channel)));
        session_manager.save(&session).await.unwrap();
    }

    // Verify each channel has its own session
    for channel in channels {
        let session_key = format!("{}:user1:chat1", channel);
        let session = session_manager.get_or_create(&session_key).await.unwrap();
        assert_eq!(session.messages.len(), 1);
        assert!(session.messages[0].content.contains(channel));
    }

    // Verify total session count
    let keys = session_manager.list().await.unwrap();
    assert_eq!(keys.len(), 3);
}
