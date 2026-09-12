# agentws — Handoff / Status

**Date:** 2026-09-13
**State:** P0-P4 complete. **59 tests green; strict Clippy green.**

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
- P4 is implemented: shared library/pickers, durable skill/guidance selections,
  root composition with repo-local honoring, refresh, and reusable templates.
- P3 is commit `eb22453` on `master`; the P4 commit follows it.

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
agentws add/remove/rewire/refresh/archive/restore
agentws library list/add/remove
agentws template list/show/save/delete/edit
agentws request/pending/history/approve/deny/approvals
agentws mcp/mcp-config/integrate/sandbox
agentws config/discover/completions/init-shell
```

`mcp-config` is retained as a manual-snippet alternative to `integrate`.

---

## 4. Configuration additions

```toml
library_dirs = ["~/team-agentws-library"]

[default]
template = "full-stack"

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

The built-in library is `~/.config/agentws/library/` with `skills/`, `agents/`,
and `templates/` subdirectories. External `library_dirs` use the same layout;
the built-in library wins name collisions, followed by configured roots in
order.

---

## 5. P4 composition and templates

- `new` selects repositories, skills, then AGENTS.md snippets through the shared
  fuzzy picker. `--repos`, `--skills`, `--agents-md`, and `--template` provide
  non-interactive inputs; `--copy` snapshots library skills instead of linking.
- Root `AGENTS.md` and `CLAUDE.md` contain workspace scope, selected reusable
  snippets, and live pointers to each worktree's instruction/skill locations.
- Library skills are materialized under `.agents/skills/`; repo-local skills
  from `.agents/skills`, `.pi/skills`, `.claude/skills`, and
  `.opencode/skills` receive stable `<repo>-<skill>` names.
  `.claude/skills/` mirrors the root pool.
- `.pi/settings.json` receives exact relative managed skill paths while
  preserving unrelated keys and user-managed skill entries. Pi still prompts
  for project trust; agentws does not and cannot pre-approve it.
- SQLite schema v2 stores `skills`, `agents_md`, creation setup, and applied
  template metadata. Existing schema-v1 databases gain the new tables on open;
  legacy JSON defaults remain compatible.
- `refresh` is idempotent and runs automatically after workspace membership
  changes and library add/remove. Run it after Git pulls or direct library edits.
- Templates support repository globs, skills, snippets, base, symlinks, hook,
  and copy mode. Partial templates invoke pickers for missing selection fields;
  `[default].template` makes a bare `new` non-interactive when the template is
  complete. `template save --from` snapshots a workspace.

---

## 6. Verification

Current results:

```text
cargo test --all-targets                   59 passed, 0 failed
cargo clippy --all-targets -- -D warnings  clean
```

Breakdown: 50 library tests and 9 integration tests across story resolution,
bulk delete safety, tmux approval, SQLite concurrency, Fish activation, and
macOS Seatbelt, plus the P4 template/composition lifecycle. Pi extension loading
is included in the library test suite.

The Seatbelt integration test must run outside an already restricted sandbox;
ordinary local terminal runs need no special handling. Fish/Pi load tests skip
portably when those binaries are absent, but both executed and passed on this
host.

---

## 7. File map

```text
src/manifest.rs               SQLite state/history + JSON migration
src/library.rs                central/external library discovery + import/remove
src/templates.rs              template model, glob expansion, snapshot persistence
src/composition.rs            root guidance, skill pools, Pi settings refresh
src/integrations.rs           Pi/Claude/Codex/OpenCode integration installers
src/ops.rs                    repo operations + dotenv wiring
src/mcp.rs                    stdio MCP server + direct Pi bridge
src/commands/approvals.rs     interactive/tmux watcher
src/commands/env.rs           rewire command
src/commands/integrate.rs     integration command
src/commands/sandbox.rs       macOS Seatbelt wrapper
src/commands/library.rs       library list/add/remove
src/commands/template.rs      template list/show/save/delete/edit
src/commands/refresh.rs       single/all-workspace composition refresh
tests/approvals_tmux.rs       isolated tmux E2E
tests/fish_activation.rs      real Fish E2E
tests/manifest_concurrency.rs concurrent SQLite E2E
tests/sandbox_macos.rs        filesystem isolation E2E
tests/p4_composition.rs       P4 template/composition/lifecycle E2E
```

---

## 8. Commit state and next phase

- Branch: `master`; P3 is pushed as `eb22453`.
- P4 is complete and committed on `master`.
- No scheduled implementation phase remains; §13 is an explicitly deferred
  cleanup backlog.
