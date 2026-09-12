# agentws — Handoff / Status

**Date:** 2026-09-12
**State:** P0-P3 complete. **45 tests green; strict Clippy green.**

> Read this first, then `PLAN.md`. This is the current implementation handoff.

---

## TL;DR

- agentws owns per-story workspaces, not agent processes. Source the shell
  integration, activate a story, then run Pi/Claude/Codex/OpenCode natively.
- Workspace state now lives in SQLite/WAL (`workspace.db`) with serialized
  writers and an append-only repo-request event log.
- P3 is complete: tmux approvals, Pi-native tools, MCP integration installers,
  cross-repo dotenv wiring, verified Fish activation, and an opt-in macOS hard
  sandbox.
- The verified P3 release binary is installed at `~/.cargo/bin/agentws`.
- The working tree contains all P3 work and is intentionally uncommitted. Do not
  commit until the human gives the go-ahead.

---

## 1. Architecture

Each story is `~/.agentws/<story>/` with selected repositories mounted as Git
worktrees beneath that root. The sourced `agentws activate` function changes the
current shell's cwd and sets `AGENTWS_WORKSPACE`; it never launches or owns an
agent. Native resume/session behavior therefore remains with each harness.

Workspace resolution is:

1. explicit `--story`
2. `$AGENTWS_WORKSPACE`
3. cwd beneath `~/.agentws/<story>`
4. `~/.agentws/.current`
5. the only existing workspace
6. otherwise, a helpful error

---

## 2. P3 capabilities

### SQLite manifest and request history

- `workspace.db` has `workspace`, `repos`, `requests`, and append-only
  `request_events` tables.
- WAL, a five-second busy timeout, and `BEGIN IMMEDIATE` protect concurrent
  CLI/watcher/MCP writes from lost updates.
- `agentws history [--story ...]` prints request creation and status transitions.
- Existing `workspace.json` files migrate on first access and are preserved as a
  backup. Legacy unknown fields still deserialize.
- An E2E starts eight simultaneous writers and verifies every request and event.

### Inline approval

- `agentws approvals` watches and resolves requests in the current terminal.
- `agentws approvals --tmux` creates a dedicated ten-line pane, returns focus to
  the current pane, and focuses/rings the watcher when a request arrives.
- The isolated tmux E2E creates a request, approves it through the pane, and
  verifies the SQLite status transition.

### Agent integration

`agentws integrate <agent...> [--story ...] [--force]` supports `pi`, `claude`,
`codex`, `opencode`, and `all`:

- Pi: writes `.pi/extensions/agentws.ts` with native
  `list_available_repos`, `request_repo`, and `check_request` tools plus a prompt
  for path-based file operations outside the workspace.
- Claude: merges `mcpServers.agentws` into project `.mcp.json`.
- OpenCode: merges `mcp.agentws` into project `opencode.json`.
- Codex: calls `codex mcp add agentws -- <absolute-agentws> mcp`; this is a
  user-level change and only happens when Codex is explicitly selected.

Project JSON merges preserve unrelated settings. Conflicting managed entries
fail safely unless `--force` is supplied. The generated Pi extension was loaded
successfully by installed Pi 0.84.4. No real agent configuration was modified
during development; tests and load checks used temporary paths.

### Cross-repo dotenv wiring

Config can declare per-repo `exposes`, `consumes`, and placeholder `defaults`.
For example, `api.PORT` can feed `web.API_URL` as `{api.port}`. agentws maintains
a story-specific marked block without replacing the rest of the dotenv file.

Wiring runs after `new`, `add`, `remove`, `approve`, and `restore`, and can be
rerun with `agentws rewire`. Paths must be relative, remain inside the worktree,
and not traverse symlinks; keys and generated values are validated.

### Fish activation

Fish 4.9.3 was installed through Homebrew on this host. The real-shell E2E
sources `agentws init-shell fish`, verifies cwd/env mutation, deactivation
restoration, and one-shot command restoration.

### Optional hard sandbox

`agentws sandbox [--allow-network] [--allow-read PATH] [--allow-write PATH] --`
`<command...>` uses macOS Seatbelt (`/usr/bin/sandbox-exec`). It permits the
workspace and original-repo Git metadata, creates a private temporary directory,
and denies network access by default. An E2E proves workspace and temp I/O work
while outside reads and writes fail.

This is macOS-only and opt-in. `sandbox-exec` is deprecated by Apple, so the
normal design remains soft workspace scoping plus native agent permission
prompts. The Pi extension's path guard is also a soft prompt; use `sandbox` when
strict enforcement is required.

---

## 3. Key command surface

```text
agentws new/list/use/status/open/code/delete
agentws add/remove/rewire/archive/restore
agentws request/pending/history/approve/deny/approvals
agentws mcp/mcp-config/integrate/sandbox
agentws config/discover/completions/init-shell
```

`mcp-config` is retained as a manual-snippet alternative to `integrate`.

---

## 4. Configuration additions

```toml
[env.api]
file = ".env.local"
exposes = { port = "PORT" }

[env.web]
file = ".env.local"
consumes = { API_URL = "http://localhost:{api.port}" }
defaults = { "api.port" = "5000" }
```

The dotenv reader intentionally handles ordinary `KEY=value`, quoted values,
and optional `export`; it is not a shell evaluator and does not expand command
substitutions or nested environment variables.

---

## 5. Verification

Current results:

```text
cargo test --all-targets                   45 passed, 0 failed
cargo clippy --all-targets -- -D warnings  clean
```

Breakdown: 37 library tests and 8 integration tests across story resolution,
bulk delete safety, tmux approval, SQLite concurrency, Fish activation, and
macOS Seatbelt. Pi extension loading is included in the library test suite.

The Seatbelt integration test must run outside an already restricted sandbox;
ordinary local terminal runs need no special handling. Fish/Pi load tests skip
portably when those binaries are absent, but both executed and passed on this
host.

---

## 6. File map

```text
src/manifest.rs               SQLite state/history + JSON migration
src/integrations.rs           Pi/Claude/Codex/OpenCode integration installers
src/ops.rs                    repo operations + dotenv wiring
src/mcp.rs                    stdio MCP server + direct Pi bridge
src/commands/approvals.rs     interactive/tmux watcher
src/commands/env.rs           rewire command
src/commands/integrate.rs     integration command
src/commands/sandbox.rs       macOS Seatbelt wrapper
tests/approvals_tmux.rs       isolated tmux E2E
tests/fish_activation.rs      real Fish E2E
tests/manifest_concurrency.rs concurrent SQLite E2E
tests/sandbox_macos.rs        filesystem isolation E2E
```

---

## 7. Commit state and next phase

- Branch: `master`; 12 existing commits.
- P3 changes are uncommitted per `AGENTS.md`.
- P4 remains: shared skill/AGENTS.md library and picker, per-repo guidance
  honoring, and reusable workspace templates. See `PLAN.md` §12.
