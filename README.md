# ccr

> A terminal-native AI coding assistant powered by Claude — built from scratch in Rust.

[![CI](https://github.com/JoaoHenriqueBarbosa/ccr/actions/workflows/ci.yml/badge.svg)](https://github.com/JoaoHenriqueBarbosa/ccr/actions/workflows/ci.yml)
[![Tests](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/JoaoHenriqueBarbosa/ccr/main/.github/badges/tests.json)](https://github.com/JoaoHenriqueBarbosa/ccr/actions/workflows/ci.yml)
[![Lines of Rust](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/JoaoHenriqueBarbosa/ccr/main/.github/badges/loc.json)](src)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![clippy: pedantic deny](https://img.shields.io/badge/clippy-pedantic%20deny-critical.svg)](Cargo.toml)

`ccr` is a coding agent that lives in your terminal. It is a from-scratch Rust
reimplementation of an agentic coding loop: it streams from the Anthropic API,
runs tools against your working tree, feeds the results back to the model, and
repeats until the task is done — all rendered in a `ratatui` TUI with its own
markdown renderer.

It is **not** a wrapper around another agent. The API client, the SSE parser,
the tool suite, the render pipeline, and the markdown engine are all implemented
here, in Rust, with a deliberately fanatical type discipline (see
[Code philosophy](#code-philosophy)).

---

## Highlights

- **Real agentic loop** — observe → act → observe. Streams the assistant turn,
  extracts tool calls, executes them, appends the results, and loops back into
  the model until there are no more tool uses.
- **Seven built-in tools** with typed dispatch (no string matching): `Bash`,
  `Read`, `Write`, `Edit`, `Glob`, `Grep`, `WebFetch`.
- **Hand-written SSE client** for the Anthropic Messages API, with byte-level
  UTF-8 safety (never splits a multi-byte codepoint across TCP chunks) and
  exponential backoff on 429/529/5xx.
- **Interleaved thinking** (`interleaved-thinking-2025-05-14`) surfaced live in
  the UI.
- **Own markdown renderer** (~900 lines) with `syntect` syntax highlighting,
  box-drawing tables that word-wrap and shrink columns to terminal width, task
  lists, nested blockquotes, and more.
- **Typestate render pipeline** — `PreparedBlocks → measure → clip → render`.
  The compiler forbids skipping a phase.
- **Two-thread architecture** — a UI thread that never blocks and a background
  Tokio task that does streaming + tool execution, communicating over a typed
  channel.

## Requirements

- **Rust** stable (MSRV **1.85**, edition 2024)
- **`rg` (ripgrep)** on `PATH` — used by `Glob` and `Grep`
- **`git`** on `PATH` — used for the branch indicator in the status bar
- An **Anthropic credential** (see [Authentication](#authentication))

## Install & run

```bash
git clone https://github.com/JoaoHenriqueBarbosa/ccr
cd ccr
cargo build --release
./target/release/ccr
```

## Authentication

`ccr` resolves a credential in this order:

1. `ANTHROPIC_API_KEY`
2. `CLAUDE_CODE_OAUTH_TOKEN`
3. `~/.claude/.credentials.json` (the OAuth token written by the official Claude
   Code login)

If none is found it exits with a message telling you to set `ANTHROPIC_API_KEY`
or log in with `claude`.

### Environment variables

| Variable                  | Default                                    | Purpose                        |
| ------------------------- | ------------------------------------------ | ------------------------------ |
| `ANTHROPIC_API_KEY`       | —                                          | API key auth                   |
| `CLAUDE_CODE_OAUTH_TOKEN` | —                                          | OAuth token auth               |
| `CLAUDE_MODEL`            | `claude-opus-4-6`                          | Model id                       |
| `CLAUDE_API_URL`          | `https://api.anthropic.com/v1/messages`    | API endpoint                   |

## The tool suite

Each tool has a typed input struct validated with `serde`, and a uniform
`execute_*(input, cwd) -> (ToolOutput, ToolResultStatus)` signature. Dispatch
goes through the `BuiltinTool` enum via an exhaustive `from_name` match.

| Tool         | What it does                                                                                                   |
| ------------ | ------------------------------------------------------------------------------------------------------------- |
| **Bash**     | Runs a command through the login shell, with timeouts, background detach, benign-exit handling, and output truncation/persistence. |
| **Read**     | Reads a file with line numbers; device-path and binary-extension blocklists, 10 MB cap, null-byte detection, and "did you mean?" suggestions. |
| **Write**    | Writes a file, preserving the existing encoding (UTF-8 / UTF-16LE-BOM) and line endings (LF/CRLF).             |
| **Edit**     | Exact-string replacement with uniqueness checks, curly-quote fallback, and typography re-application.          |
| **Glob**     | File matching via `rg --files`, with base-dir extraction and pagination.                                       |
| **Grep**     | Full ripgrep surface: content/files/count modes, context lines, multiline, type filters, pagination.          |
| **WebFetch** | Fetches a URL and converts HTML → markdown, with http→https upgrade and same-host redirect policy.             |

## Code philosophy

`ccr` follows an ascetic type discipline, codified in
[`CLAUDE.md`](CLAUDE.md) and enforced by the compiler and `clippy`:

- **No primitives in fields.** Every concept gets a newtype; booleans become
  descriptive enums. Illegal states are made unrepresentable.
- **Typed tool dispatch** — an exhaustive enum match, never string comparison.
- **Path-traversal guard** — `WorkingDir::validate_path` canonicalizes and
  confines every file access to the working directory.
- **Typestate everywhere** — the render pipeline, the stream accumulator, and
  the turn timer all use the type system to forbid invalid sequences.
- **Typed errors** via `thiserror` — never `anyhow`.
- **`clippy` `pedantic = deny`**, `#[must_use]` on value-returning methods,
  functions kept short.
- **Supply-chain hygiene** via `cargo-deny` ([`deny.toml`](deny.toml)).

## Development

```bash
cargo test                 # test suite
cargo clippy --all-targets # pedantic = deny — the bar is high
cargo deny check           # advisories, licenses, bans, sources
cargo fmt --check          # edition 2024, width 100
```

## Architecture

```
src/
├── main.rs         entry: resolve credential → tui::run
├── auth.rs         API key / OAuth resolution
├── api.rs          AnthropicClient, hand-written SSE, retry/backoff
├── tools/          the seven tools + shared helpers
├── tui/
│   ├── mod.rs      main UI loop, key handling
│   ├── backend.rs  the ReAct loop: stream → tools → loop
│   ├── render.rs   typestate render pipeline
│   └── markdown.rs markdown → ratatui renderer
└── types/          newtypes, API types, the WorkingDir guard, errors
```

## License

MIT © João Henrique Barbosa. See [LICENSE](LICENSE).
