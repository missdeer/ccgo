# CLAUDE 

## Workflow

- **MUST** follow multi-agent-workflow
- carefully read and understand rules defined in `.claude/rules` directory and follow them strictly

---

## File editing on Windows (CRITICAL FIX)

**ALWAYS use RELATIVE paths** for Read and Edit tools:

✅ CORRECT:
- Read("src/components/Button.tsx")
- Edit("src/components/Button.tsx", ...)
- Read("config/settings.json")
- Edit("config/settings.json", ...)

❌ INCORRECT:
- Read("C:/Users/.../src/components/Button.tsx")
- Edit("C:/Users/.../src/components/Button.tsx", ...)

**Rules:**
1. Use paths relative to your working directory
2. Use the SAME exact path in Read and Edit
3. Avoid absolute paths with forward slashes

**If error persists:** Re-read with the SAME relative path.

---

## Project Overview

CCGO (ClaudeCode-Codex-Gemini-OpenCode) is an MCP (Model Context Protocol) server that enables Claude Code to orchestrate multiple AI coding assistants (Codex, Gemini, OpenCode) through a unified interface. It runs as an MCP server over stdio while providing a web UI for real-time terminal monitoring.

## Build and Development Commands

```bash
# Build
cargo build                    # Debug build
cargo build --release          # Release build (optimized, stripped)

# Run
cargo run -- serve             # Run as MCP server (default)
cargo run -- web               # Run web server only
cargo run -- config            # Show configuration

# Testing
cargo test --lib --verbose           # Unit tests
cargo test --test '*' --verbose      # Integration tests
cargo test --doc --verbose           # Doc tests
cargo test --all-features --verbose  # All tests with all features

# Linting and Formatting
cargo fmt --all -- --check                              # Check formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lint
cargo doc --no-deps --all-features                      # Build docs (RUSTDOCFLAGS=-D warnings)

# Static check
cargo check --all-targets --all-features
```

Minimum supported Rust version: 1.90

## Architecture

```
src/
├── main.rs          # CLI entry point (clap), builds Config, launches servers
├── lib.rs           # Public module exports
├── config/          # Configuration structs (ServerConfig, AgentConfig, TimeoutConfig, WebConfig)
├── agent/           # Agent trait and implementations (Codex, Gemini, OpenCode)
│   └── mod.rs       # Agent trait: startup commands, ready patterns, sentinel injection
├── session/         # Session management layer
│   └── mod.rs       # AgentSession (per-agent state machine), SessionManager (registry)
├── pty/             # PTY management (portable-pty)
│   └── mod.rs       # PtyHandle (spawn, read/write, buffer), PtyManager (handle registry)
├── mcp/             # MCP protocol implementation
│   ├── mod.rs       # McpServer: stdio JSON-RPC (auto-detects JSONL vs LSP-style)
│   ├── protocol.rs  # JSON-RPC types, MCP initialize/tools/call handlers
│   └── tools.rs     # Tool definitions (ask_agents)
├── state/           # Agent state machine (Stopped→Starting→Idle⇄Busy→Dead)
├── log_provider/    # Log file watchers for detecting agent replies
│   └── mod.rs       # LogProvider trait, file watching with debouncing
└── web/             # Web UI and API (axum)
    ├── mod.rs       # WebServer, routes (/api/*, /ws/*)
    ├── handlers.rs  # REST API handlers
    ├── websocket.rs # WebSocket terminal I/O
    ├── static_files.rs  # Embedded static files (rust-embed)
    └── auth.rs      # Optional auth token middleware
```

### Key Data Flow

1. **MCP Request** → `McpServer::handle_tools_call` → `execute_tool("ask_agents", ...)` → `SessionManager::get(agent)` → `AgentSession::ask()`
2. **AgentSession::ask** → auto-starts agent if stopped → queues request → sends to PTY with sentinel → waits for LogProvider to detect reply
3. **Reply Detection** → LogProvider watches agent log files → parses assistant responses → delivers via oneshot channel

### Agent State Machine

`Stopped` → (StartAgent) → `Starting` → (ReadyDetected) → `Idle` ⇄ (AskAgent/ReplyReceived) → `Busy`
Any running state → (StopAgent/ProcessDied) → `Dead`

### PTY Layer

Uses `portable-pty` for cross-platform PTY support (ConPTY on Windows, native TTY on Unix). Each agent gets its own PTY with:
- Ring buffer for terminal output history
- Broadcast channel for WebSocket streaming
- Graceful shutdown with process cleanup

## MCP Tool

Single tool exposed: `ask_agents`
- `requests`: Array of 1-4 agent requests, each containing:
  - `agent`: "codex" | "gemini" | "opencode" | "claudecode"
  - `message`: Prompt to send
- `timeout`: Optional seconds (default: 600, max: 1800)

Returns JSON: `{"results": [{"agent": "...", "success": true/false, "response": "...", "error": "..."}]}`

Agents auto-start on first message. Multiple agents execute in parallel.
