# Contributing to ccr

Thanks for your interest in contributing! This guide explains how to get involved.

## Environment Setup

```bash
git clone https://github.com/JoaoHenriqueBarbosa/ccr
cd ccr
cargo build
```

You will also need, on your `PATH`:

- **`rg` (ripgrep)** — used by the `Glob` and `Grep` tools
- **`git`** — used for the branch indicator

## Project Structure

```
src/
  main.rs      entry point
  auth.rs      credential resolution
  api.rs       Anthropic client, hand-written SSE, retry/backoff
  tools/       the seven tools + shared helpers
  tui/         main loop, backend ReAct loop, render pipeline, markdown renderer
  types/       newtypes, API types, WorkingDir guard, errors
```

## The Bar

`ccr` follows the ascetic type discipline in [`CLAUDE.md`](CLAUDE.md). Before a PR
is mergeable it must clear:

```bash
cargo test                 # the full suite must pass
cargo clippy --all-targets # clippy pedantic = deny — zero warnings
cargo fmt --check          # edition 2024, width 100
cargo deny check           # advisories, licenses, bans, sources
```

Concretely, that means: **no primitives in struct fields** (use newtypes),
booleans expressed as descriptive enums, typed error variants (never `anyhow`),
`#[must_use]` on value-returning methods, and short functions.

## Contribution Flow

1. **Fork** the repository
2. **Create a branch**: `git checkout -b feat/my-feature`
3. **Make your changes** (respecting the discipline above)
4. **Run the checks** listed in [The Bar](#the-bar)
5. **Commit** using [conventional commits](https://www.conventionalcommits.org/)
6. **Push** and open a **Pull Request**

## Conventional Commits

| Type       | Description                          |
| ---------- | ------------------------------------ |
| `feat`     | A new feature                        |
| `fix`      | A bug fix                            |
| `docs`     | Documentation only                   |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `test`     | Adding or correcting tests           |
| `perf`     | A performance improvement            |
| `ci`       | CI configuration                     |
| `chore`    | Tooling / housekeeping               |

## Reporting Bugs & Requesting Features

Use the issue templates under **Issues → New issue**.
