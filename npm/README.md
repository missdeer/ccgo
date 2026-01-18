# ccgo npm shim

This npm package provides a convenient way to install and run `ccgo` (ClaudeCode-Codex-Gemini-OpenCode MCP Server) via npm/npx.

## How It Works

This is a **shim package** that automatically downloads the appropriate pre-built binary for your platform from GitHub Releases when first run. The binary is cached locally for subsequent invocations.

## Quick Start

```bash
# Run directly with npx (no installation needed)
npx ccgo serve

# Or install globally
npm install -g ccgo
ccgo serve
```

## What is CCGO?

CCGO is an MCP (Model Context Protocol) server that enables Claude Code to orchestrate multiple AI coding assistants (Codex, Gemini, OpenCode) through a unified interface. Features include:

- **MCP Protocol Support** - Runs as an MCP server over stdio
- **Multi-Agent Management** - Manage Codex, Gemini, and OpenCode from a single interface
- **PTY-based Execution** - Each agent runs in its own pseudo-terminal
- **Web UI Console** - Real-time terminal output via WebSocket
- **Cross-Platform** - Works on Windows, Linux, and macOS

## Supported Platforms

| Platform | Architecture | Status |
|----------|--------------|--------|
| Windows  | x64, ARM64   | Supported |
| macOS    | x64, ARM64   | Supported (universal binary) |
| Linux    | x64, ARM64   | Supported |

## Cache Location

Downloaded binaries are cached in platform-specific directories:

| Platform | Cache Path |
|----------|------------|
| Windows  | `%LOCALAPPDATA%\ccgo\<version>\` |
| macOS    | `~/Library/Caches/ccgo/<version>/` |
| Linux    | `$XDG_CACHE_HOME/ccgo/<version>/` or `~/.cache/ccgo/<version>/` |

The cache is versioned, so upgrading the npm package will download a new binary matching that version.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `GITHUB_TOKEN` | GitHub personal access token to avoid rate limits when downloading |

## Requirements

- Node.js 14.14.0 or later
- `tar` command (Linux/macOS) or PowerShell 5.0+ (Windows) for extraction

## Troubleshooting

### Download Fails

If automatic download fails, you can:

1. **Manual download**: Download the appropriate binary from [GitHub Releases](https://github.com/missdeer/ccgo/releases) and place it in the cache directory.

2. **Install via Cargo**: If you have Rust installed:
   ```bash
   cargo install ccgo
   ```

3. **Set GITHUB_TOKEN**: If you're hitting GitHub API rate limits:
   ```bash
   export GITHUB_TOKEN=your_github_token
   npx ccgo serve
   ```

### Binary Not Found After Extraction

Ensure your system has the required extraction tools:
- **Windows**: PowerShell 5.0+ with `Expand-Archive` cmdlet
- **Linux/macOS**: `tar` command

## How the Shim Works

1. On first run, checks if the binary exists in the cache directory
2. If not found, queries GitHub API for the release matching the npm package version
3. Downloads the platform-appropriate archive (`.zip` for Windows, `.tar.gz` for others)
4. Extracts the binary to the cache directory
5. Executes the binary with all provided arguments

## License

This package is part of [ccgo](https://github.com/missdeer/ccgo) and is dual-licensed under GPL-3.0 (non-commercial) and a commercial license.

For commercial use, please contact missdeer@gmail.com for licensing options.
