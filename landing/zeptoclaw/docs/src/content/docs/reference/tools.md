---
title: Tools Reference
description: Complete reference for all built-in agent tools
tableOfContents:
  minHeadingLevel: 2
  maxHeadingLevel: 3
---

ZeptoClaw ships with 17 built-in tools. Each tool is available to the agent by default unless restricted by the approval gate or a template's tool whitelist.

## shell

Execute shell commands with optional container isolation.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | Yes | The shell command to execute |

**Security:** Commands are checked against a regex blocklist (dangerous patterns like `rm -rf /`, `curl | sh`, etc.) and can be isolated in Docker or Apple Container.

## read_file

Read file contents from the workspace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | Yes | Relative path within workspace |

## write_file

Write or create files in the workspace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | Yes | Relative path within workspace |
| `content` | string | Yes | File contents to write |

## list_files

List directory contents.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | No | Directory path (default: workspace root) |

## edit_file

Search-and-replace edits on existing files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | Yes | Relative path within workspace |
| `old_text` | string | Yes | Text to find |
| `new_text` | string | Yes | Replacement text |

## web_search

Search the web using the Brave Search API.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Search query |

**Security:** SSRF protection blocks requests to private IP ranges, IPv6 loopback, and non-HTTP schemes. DNS pinning prevents rebinding attacks.

## web_fetch

Fetch and parse a web page.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | Yes | URL to fetch |

Returns cleaned text content (HTML stripped). Response body limited to prevent token waste.

## memory

Search workspace memory (markdown files).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Search query |

Searches markdown files in the workspace, scoring by keyword relevance with chunked results.

## longterm_memory

Persistent key-value store with categories and tags.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `action` | string | Yes | One of: set, get, search, delete, list, categories |
| `key` | string | Varies | Memory key |
| `value` | string | Varies | Value to store |
| `category` | string | No | Category for organization |
| `tags` | array | No | Tags for filtering |

Stored at `~/.zeptoclaw/memory/longterm.json`. Persists across sessions with access tracking.

## message

Send proactive messages to channels.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `content` | string | Yes | Message text |
| `channel` | string | No | Target channel (telegram, slack, discord, webhook) |
| `chat_id` | string | No | Target chat ID |

Falls back to the current context's channel and chat_id if not specified.

## cron

Schedule recurring tasks.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `action` | string | Yes | One of: add, list, remove |
| `name` | string | Varies | Job name |
| `schedule` | string | Varies | Cron expression |
| `message` | string | Varies | Message to process |

## spawn

Delegate a background task.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `message` | string | Yes | Task description |
| `label` | string | No | Task label |

## delegate

Create a sub-agent (agent swarm).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `role` | string | Yes | Sub-agent role/system prompt |
| `message` | string | Yes | Message to send |
| `tools` | array | No | Tool whitelist for sub-agent |

The delegate tool creates a temporary agent loop with a role-specific system prompt. Recursion is blocked to prevent infinite delegation chains.

## whatsapp

Send WhatsApp messages via Cloud API.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `to` | string | Yes | Recipient phone number |
| `message` | string | Yes | Message text |

## gsheets

Read and write Google Sheets.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `action` | string | Yes | One of: read, write, append |
| `spreadsheet_id` | string | Yes | Google Sheet ID |
| `range` | string | Yes | Cell range (e.g., "A1:B10") |
| `values` | array | Varies | Data to write |

## r8r

Content rating and analysis tool.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `content` | string | Yes | Content to analyze |
