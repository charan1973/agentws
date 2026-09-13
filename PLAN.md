# agentws — Plan

A CLI that creates per-story workspaces for AI coding agents (Claude Code, Codex,
OpenCode, pi) across a polyrepo / microservice codebase. Each story gets a fresh
directory containing `git worktree`s of only the repos that story touches, so the
agent is scoped by default — no token waste or context pollution from grepping the
whole `work/` tree. When the agent discovers it needs *another* repo mid-session,
it requests it and a human approves; the repo then appears in-scope with no
restart.

---

## 1. The problem

- Microservice architecture: a single story spans 3+ repos.
- All repos live under one `work/` directory.
- Starting an agent in `work/` makes it explore everything — wasted tokens,
  context pollution, no guarantee it stays inside the right repos.
- You don't always know the full repo set up front; the agent may discover more
  as it works. A hard sandbox is wrong — you want it to be able to reach out,
  *with permission*.

## 2. What already exists (and the gap)

Multi-repo worktree orchestration is well-solved: `wtp`, `projector/pj`, `wtmux`,
`flow`, `grove`, `etz`, `multree`, `workspace-manager`, etc. Hard-sandbox tools
(`abox`, `hawt`) take the opposite philosophy (microVM isolation).

**The gap none of them close:** *human-approved, mid-session repo expansion.*
`wtmux` states it plainly — agents fix their attached-directory set at launch and
can't be re-scoped mid-session — so everyone bakes the full repo set in up front.
`wtp`/`pj` let you add a repo from a *separate* terminal, but the running agent
isn't re-scoped and you'd restart it. Nobody does "agent asks → human approves →
repo appears in the live session".

**agentws's differentiator is exactly that.** And it's achievable without restart
because of the layout insight below.

## 3. Decisions (locked in)

These were the open questions, now decided. Explained in plain terms.

### 3.1 Build from scratch in Rust, single static binary

Greenfield — we want full control over the permission/expansion layer and the
agent adapters. Rust gives one distributable binary with no runtime dependency.
We re-implement the (already-solved) worktree plumbing ourselves, which is
deliberate: it's thin, and keeping it in-tree means the expansion protocol can
touch every layer cleanly.

### 3.2 Soft scope by default; hard sandbox is opt-in

The agent is **not** locked in. We get scoping three ways:
1. **Physical**: each story's repos live as worktrees *under one per-story
   directory*. The agent runs from that directory, so it sees only those repos by
   default — no grep pollution, for free.
2. **Request/approve**: when the agent needs another repo, it asks; a human
   approves; the repo appears in-scope.
3. **Native prompts**: for any stray out-of-scope read, the agent's own permission
   system prompts you.

We skip FUSE / mount-namespaces / microVMs (macOS has no mount namespaces, and
we explicitly don't want a hard restriction in the normal flow). P3 adds
`agentws sandbox` as a macOS-only Seatbelt escape hatch for commands that do need
strict filesystem isolation; it is never enabled implicitly.

### 3.3 Conda-style activation (PIVOTED — supersedes the old launcher model)

**agentws does NOT spawn agents.** It owns the workspace (worktrees + dir); you
`activate` it (a sourced shell function: `cd` in + `export AGENTWS_WORKSPACE`)
then run `pi`/`claude`/`codex` yourself, natively. The agent resumes itself
(`pi -c`, `/resume`) keyed by the workspace dir — **resume is no longer our
problem**.

- `AGENTWS_WORKSPACE` is **per-shell** (set by the sourced function), exactly
  like `conda activate`.
- `activate <story> <cmd>` = one-shot passthrough (run cmd in workspace, return).
- Old launcher (`agent.rs`, `launch` command, `agent` manifest field, `default_agent`)
  was **removed**.

> ✅ **STATUS:** verified in bash, zsh, and Fish. The Fish E2E covers persistent
> activation/deactivation and one-shot command restoration.

### 3.4 Hybrid expansion: native/MCP tools + CLI share one manifest

Two ways to add a repo mid-session, both writing the same SQLite database:
- **Agent-initiated** (MCP tool): agent calls `request_repo(name)`; supervisor/
  notification prompts you; on approval the worktree is created.
- **Human-initiated** (CLI): you run `agentws add <repo>` directly.

`agentws integrate` installs project-local Pi, Claude, and OpenCode integrations;
Codex is registered through its supported user-level MCP CLI only when explicitly
selected. `mcp-config` remains the manual-snippet alternative.

### 3.5 Branch naming: `feat/<story>` off each repo's default branch

Every repo in a story gets `feat/<story-name>`, off that repo's default branch.
Override per story with `--base <branch>`.

### 3.6 Workspace location: `~/.agentws/<story>/`

Workspaces live under `~/.agentws/`, **not** inside `work/`. Keeps your real repo
dir clean. The agent runs from `~/.agentws/<story>/` and sees only that story's
repos.

### 3.7 Picker: `ratatui` fuzzy multi-select

Full-screen terminal UI built with `ratatui` + `crossterm`, fuzzy filtering via
`fuzzy-matcher`. Type to filter, Space to toggle, Enter to confirm.

## 4. The key insight — layout *is* the scoping

If every repo's worktree is a child of one per-story workspace root, and we
run the agent **from that root** (after `agentws activate`):

- The agent's working directory contains **only** the chosen repos → no grep
  pollution, for free.
- `--add-dir` is **not needed** (that flag is only for dirs *outside* the working
  directory). Launchers simplify to just running the agent from the root.
- **Expansion needs no restart**: a new repo worktree added *under the root*
  appears inside the working directory, so the agent sees it on its next
  `ls`/`read`. This sidesteps the "can't re-scope a running agent" constraint
  that blocks every other tool.

## 5. Architecture

```
L0  Workspace core      config, repo discovery+cache, fuzzy picker,
                         git worktree create, manifest, lifecycle commands
L1  Activation           conda-style `activate`/`deactivate` shell function
                         (cd in + $AGENTWS_WORKSPACE); agents run natively
L2  expansion (★)         native/MCP tools + CLI (request/approve/deny/pending) +
                         macOS notifications (fired by the MCP tool)
L3  Polish / seamless    tmux inline approval, SQLite manifest + history,
                         agent integration + pi-native tools/scope guard,
                         cross-repo env wiring, Fish verification, optional Seatbelt
```

★ = the differentiator.

## 6. Data model — `~/.agentws/<story>/workspace.db`

SQLite/WAL is authoritative. The schema separates current state from audit data:

- `workspace`: singleton story/root/created/archive metadata + schema version
- `repos`: current worktree membership and branch/base information
- `requests`: current request state (`pending` / `approved` / `denied`)
- `request_events`: append-only request creation and status transitions, including
  actor, timestamp, repo, and reason

All read-modify-write operations use `BEGIN IMMEDIATE` with a busy timeout, so
concurrent CLI, watcher, Pi, and MCP writers cannot silently lose updates. A
concurrency E2E exercises eight simultaneous writers. Existing `workspace.json`
manifests migrate automatically on first access and remain untouched as a backup;
unknown legacy fields such as the removed `agent` field are still accepted.

## 7. Hybrid expansion protocol (the differentiator)

The hard part is **TTY contention**: the agent owns the terminal (it's a TUI), so
the MCP server can't print a y/n prompt over it (MCP stdio is JSON-RPC, not a
free stdin). Pragmatic v1:

**Design note (v1 simplification):** instead of a long-running supervisor process
watching the manifest, the MCP `request_repo` tool **fires the notification
directly** when the agent calls it (it's a child of the agent and can run
`osascript`). This removes a whole class of background-process lifecycle bugs
while keeping the exact same UX. The tmux-inline variant (P3) is where a
supervisor pane would return.

1. **Agent** calls MCP tool `request_repo(name, reason?)` → writes a `pending`
   request to the manifest → returns *"queued; call `check_request(id)`"*.
2. **Notification** is fired directly by the MCP `request_repo` tool (it's a
   child of the agent and can run `osascript`) — a **macOS notification** + bell:
   *"agentws: agent wants `payments-service` — run `agentws approve ab12`"*.
3. **Human** runs `agentws approve ab12` (any terminal) → supervisor runs
   `git worktree add` **under the workspace root** → flips status to `approved`.
4. **Agent** polls `check_request(ab12)` → gets `approved, path: ./payments`
   → reads it. **No restart** — it's inside the working directory.

CLI side is symmetric: `agentws add payments-service` (human-initiated, no prompt)
writes straight to `approved`. Both paths share one manifest writer.

**P3 upgrade (implemented):** `agentws approvals --tmux` opens a persistent
approval watcher in a small tmux pane without launching or owning the agent. The
current pane remains active; when a request arrives, the watcher rings the bell,
focuses its pane, and prompts for approve/deny/skip. `agentws approvals` provides
the same live prompt in the current terminal.

## 8. Command surface

```
agentws new <story> [--repos a,b,c] [--base main]   # create worktrees
agentws list                                  # all workspaces (* marks active)
agentws status [story]                        # repos, branches, dirty, pending
agentws open <story>                          # print root path (cd via shell wrapper)
agentws use <story>                           # set the global active pointer
agentws delete [story...] [--all] [--dry-run] [--force] [--yes]
agentws uninstall [--dry-run] [--force] [--include-config] [--include-binary]

# expansion (human)
agentws add <repo> [--story s] [--base b]     # add a repo now
agentws remove <repo> [--story s]
agentws rewire [--story s]                    # refresh managed dotenv values
agentws archive <story> | restore <story>

# expansion (permission-gated)
agentws request <repo> [--reason] [--story s]
agentws pending
agentws history [--story s]                   # append-only request audit trail
agentws approve <id|repo> [--story s] | deny <id|repo> [--story s]
agentws approvals [--story s] [--tmux]        # live inline approval watcher

# conda-style activation (sourced shell function, NOT a binary subcommand)
#   eval "$(agentws init-shell zsh)"; then:
agentws activate <story> [command...]         # cd in + set $AGENTWS_WORKSPACE (run cmd? one-shot)
agentws deactivate

# integration / introspection
agentws mcp                                   # run the MCP server (stdio)
agentws mcp-config <agent>                    # print MCP wiring snippet
agentws integrate <agent...> [--force]        # pi/claude/codex/opencode/all
agentws sandbox [flags] -- <command...>       # macOS hard-isolation escape hatch
agentws completions <shell>                   # generate shell completions
agentws init-shell [shell]                    # print activate/deactivate function
agentws config                                # show resolved config + key paths
agentws discover                              # scan roots, list discovered repos
agentws _list-stories                         # (hidden) story names for completion
```

## 9. Phasing

### P0 — Workspace core *(done)*
- [x] Cargo crate scaffold, CLI skeleton (clap) + thin `lib.rs` (testable)
- [x] Config loading (`~/.config/agentws/config.toml`, auto-writes an example) + `parse()` for tests
- [x] Repo discovery + skip-list over configured roots
- [x] `ratatui` fuzzy multi-select picker (`fuzzy-matcher`)
- [x] `git worktree add`/`remove`, default-branch detection, idempotent reuse
- [x] Initial manifest (`workspace.json`) read/write + `flock` (superseded by P3 SQLite)
- [x] `new` / `list` / `status` / `open` / `delete`
- [x] `AGENTS.md` scope stub injected at workspace root
- [x] End-to-end smoke test (`new`/`list`/`status`/`open`/`delete` verified)

### P1 — Quality of life *(done)*
- [x] `mcp-config <agent>` prints wiring snippets (claude/codex/opencode/pi) — safer than auto-editing user configs
- [x] `add` / `remove` (human-initiated expansion), `archive` / `restore`
- [x] `symlinks` config (glob) + `post_create` hook, applied on create/add/restore
- [x] Shell completions (`completions`) + `init-shell` helper
- [x] **Active-workspace pointer** (`~/.agentws/.current`): resolve target via `--story` → `$AGENTWS_WORKSPACE` → cwd → active pointer → single-workspace → helpful error. Set on `new`/`use`; cleared on `delete`; `*` in `list`
- [x] SIGPIPE handled (no broken-pipe panic when piping to `head`)
- [x] `config` / `discover` — inspect resolved config + list discovered repos

### P2 — Permission-gated expansion (the differentiator) *(done)*
- [x] `agentws mcp` server: `list_available_repos`, `request_repo`, `check_request` (hand-rolled stdio JSON-RPC 2.0)
- [x] `request` / `approve` / `deny` / `pending` CLI
- [x] macOS notification **fired directly from the MCP `request_repo` tool** (see design note below — supervisor dropped)
- [x] Approval → worktree-under-root + manifest update + path returned to agent
- [x] E2E verified: agent requests → `check_request` pending → `approve` → worktree on `feat/<story>` → `check_request` returns path, no restart

### Activation pivot + tests *(done)*
- [x] **Ripped out agent-launching** (deleted `agent.rs`/`launch.rs`, `--agent`/`--no-launch`, `default_agent`, `agent` manifest field). agentws never spawns agents.
- [x] **Conda-style `activate`/`deactivate`** — sourced shell function via `init-shell` (bash, zsh, and Fish verified). `activate <story> [cmd]` does cd+env or one-shot passthrough.
- [x] `$AGENTWS_WORKSPACE` added to `resolve_story` (priority #2); per-shell scoping, like `conda activate`.
- [x] Hidden `_list-stories` subcommand to feed shell completion.
- [x] **Test suite**: 57 unit tests + 12 integration tests. `cargo test --all-targets` and strict Clippy are green.
- [x] Doc cleanup (PLAN.md, README.md, HANDOFF.md).

### P3 — Seamless + pi-native *(done)*
- [x] tmux inline approval (`agentws approvals --tmux`; persistent watcher pane, auto-focus + bell, approve/deny/skip prompt; E2E-tested with an isolated tmux server)
- [x] SQLite/WAL manifest + append-only request history; legacy JSON auto-migration and concurrent-writer E2E
- [x] `agentws` **Pi extension** with native repo tools and an out-of-workspace path guard; load-tested in installed Pi
- [x] **Agent wiring** via `agentws integrate`: project-local Pi/Claude/OpenCode plus explicit Codex CLI registration
- [x] env-file cross-repo wiring (expose/consume/default templates, managed blocks, automatic lifecycle refresh + `rewire`)
- [x] verify **Fish** activation in real Fish 4.9.3 (persistent and one-shot flows)
- [x] optional macOS hard-sandbox escape hatch (network opt-in, private temp, outside read/write denial E2E)

> Note: `resume`/reattach is **solved by design** — agents key sessions by cwd,
> so `pi -c` / `/resume` from inside an activated workspace just works. Removed
> from the list.

### P4 — Skills/AGENTS.md picker, per-repo honoring, templates *(done — see §12)*
- [x] central library (`~/.config/agentws/library/` + `library_dirs`) + picker: skills & AGENTS.md snippets in `new`
- [x] per-repo skills/AGENTS honoring (root pointers + namespaced skill pool + explicit Pi settings)
- [x] templates (`new --template`, `template save --from`, list/show/delete/edit, defaults + partial presets)

### P5 — Cleanup and uninstall *(done — see §13)*
- [x] bulk delete by names, fuzzy picker, or `--all`, with per-workspace dirty counts
- [x] dirty-worktree preservation, dry-run, aggregate confirmation, and mandatory typed `--all` confirmation
- [x] MCP `delete_workspaces` preview/confirmed-deletion path
- [x] uninstall preview + mandatory confirmation; config and binary removal are explicit opt-ins

## 10. Project structure

```
agentws/
├── Cargo.toml
├── PLAN.md  README.md  HANDOFF.md
├── src/
│   ├── lib.rs           # pub modules (unit + integration tests target this)
│   ├── main.rs          # thin binary: SIGPIPE reset → cli::run()
│   ├── cli.rs           # clap command definitions + dispatch
│   ├── config.rs        # config.toml (roots, symlinks, hooks, env wiring)
│   ├── discovery.rs     # find repos under roots
│   ├── manifest.rs      # SQLite/WAL state + history + JSON migration + resolution
│   ├── worktree.rs      # git worktree add/remove, default-branch, dirty check
│   ├── ops.rs           # repo ops, symlinks/hooks, env wiring, request ids
│   ├── picker.rs        # ratatui fuzzy multi-select
│   ├── mcp.rs           # MCP stdio server + direct Pi tool bridge
│   ├── integrations.rs  # Pi extension + Claude/Codex/OpenCode installers
│   ├── library.rs       # shared skills/snippets/templates discovery + import
│   ├── templates.rs     # template model, glob expansion, snapshots
│   ├── composition.rs   # root guidance + skill pools + Pi settings
│   ├── util.rs          # tilde expansion
│   └── commands/
│       ├── new list status open delete use_ws
│       ├── add remove expand lifecycle
│       ├── approvals env integrate sandbox library template refresh uninstall
│       ├── config discover mcp_config completions
│       ├── init_shell                   # conda-style activate/deactivate function
│       └── mod.rs
└── tests/
    ├── approvals_tmux.rs delete_bulk.rs resolve_priority.rs
    ├── fish_activation.rs manifest_concurrency.rs
    ├── sandbox_macos.rs uninstall.rs
    └── p4_composition.rs
```

## 11. Dependencies

| crate          | use                                   |
|----------------|---------------------------------------|
| clap           | CLI parsing (derive)                  |
| clap_complete  | shell completion generation           |
| serde          | (de)serialize                         |
| serde_json     | MCP, integrations, legacy migration   |
| toml           | config file                           |
| anyhow         | error handling                        |
| directories    | XDG/home paths                        |
| ratatui        | picker TUI                            |
| crossterm      | terminal I/O for picker               |
| fuzzy-matcher  | fuzzy filter in picker                |
| chrono         | timestamps in manifest/requests       |
| libc           | SIGPIPE reset (unix)                  |
| rusqlite       | SQLite manifest + request history      |
| tempfile (dev) | unit + integration tests              |

## 12. P4: skills/AGENTS.md picker, per-repo honoring, templates

**Status: implemented and E2E-tested.**

### 12.1 Problem / goal
- Before P4, `new` picked repos only and wrote a single root `AGENTS.md`. There was no way
  to pick skills or AGENTS.md snippets from a shared library, and no presets.
- Per-repo guidance (each repo's own `AGENTS.md` / `.agents/skills/`) was invisible
  from the workspace root: agents run from the root and don't auto-descend into
  subdirs, and pi discovers project skills only up to the git root — and each
  worktree *is* its own git root — so per-repo skills aren't found from the root.
- agentws's only lever is the **filesystem at the workspace root** (it does not
  control the agent process), so the solution is composition at the root.

### 12.2 Grounding — how agents discover these
- **pi** discovers skills from global (`~/.pi/agent/skills/`, `~/.agents/skills/`)
  and **project** locations (`.pi/skills/`, `.agents/skills/` in cwd + ancestors up
  to the git root), plus an explicit `skills` array in `.pi/settings.json` and
  `--skill <path>`. Installed Pi 0.84.4 also documents loading `AGENTS.md` or
  `CLAUDE.md` from global/parent/current directories.
- **Codex** reads scoped `AGENTS.md`, scans repository `.agents/skills/`, and
  follows symlinked skill directories. **Claude** uses `CLAUDE.md` and
  `.claude/skills/`; **OpenCode** reads `AGENTS.md` and `.agents/skills/`.
- agentws therefore emits the universal `AGENTS.md` + `.agents/skills/` baseline,
  a matching `CLAUDE.md` + `.claude/skills/` mirror, and explicit Pi settings.

### 12.3 Central library (new)
```
~/.config/agentws/library/
  skills/<name>/SKILL.md      # Agent Skills standard
  agents/<name>.md            # reusable AGENTS.md snippets/sections
  templates/<name>.toml       # see 12.6
```
- Extensible: `library_dirs = ["~/team-skills-repo"]` → external sources appear in
  the picker too (team-shared git repo; single source of truth).
- `agentws library list | add <path> | remove <name>`.

### 12.4 Picker in `new`
Reuse the existing `ratatui` fuzzy multi-select, sequenced: repos → skills →
agents-md. Non-interactive flags mirror `--repos`: `--skills a,b`,
`--agents-md x,y`, and `--template t` (short-circuits all pickers).

### 12.5 Per-repo honoring — the answer to "how do tools honor per-repo skills/AGENTS.md?"
Three layered mechanisms; **(1) is the default**, (2)+(3) harden it:

1. **Behavioral pointers in root `AGENTS.md`** (universal, always fresh, no
   duplication). Lists each repo's `./<repo>/AGENTS.md` and tells the agent to read
   the repo's own guidance + `./<repo>/.agents/skills/` before editing there. Stays
   correct across `git pull` (points at the real files).
2. **Namespace per-repo skills into the root pool.** Materialize each
   repo-local `.agents/skills`, `.pi/skills`, `.claude/skills`, and
   `.opencode/skills` entries into root
   `.agents/skills/<repo>-<skill>` (repo-prefixed to avoid name clashes). The
   managed view rewrites only the skill name and symlinks its supporting files,
   so the Agent Skills metadata and directory name remain portable.
3. **Explicit per-agent settings lists** (strongest guarantee). `.pi/settings.json`
   `skills` array enumerates exact paths in the root pool — no discovery or
   ambiguity. Existing Pi keys and non-agentws skill entries are preserved.

Plus `agentws refresh`: rebuilds the root `AGENTS.md` + re-syncs symlinks/settings
after `add`/`remove`, a library edit, or a `git pull` that changes a repo's skills.
Idempotent and cheap.

> **Trust (pi):** pi loads **project** skills only after you trust the dir
> (first-run prompt). agentws can't pre-trust — document "approve trust on first
> `pi` run". Global skills (`~/.pi/agent/skills/`) need no trust but aren't
> per-workspace.

### 12.6 Templates
A named preset of the whole `new` selection set:
```toml
# ~/.config/agentws/library/templates/<name>.toml
repos = ["api", "web", "payments"]      # names, or globs like "svc-*"
skills = ["react-testing"]
agents_md = ["house-style"]
base = "main"
symlinks = ["node_modules"]
post_create = "pnpm install"
copy_skills = false
```
- `agentws new <story> --template <name>` → applies non-interactively.
- `agentws template save <name> [--from <story>]` → snapshots an existing
  workspace's selections ("this setup worked, reuse it").
- `agentws template list | show <name> | delete <name> | edit <name>`.
- Templates reference library items **by name**, so a library update propagates to
  every workspace that symlinks it (no stale copies). Partial templates (only some
  fields) fall back to the interactive picker for the rest; a `[default]` template
  makes bare `agentws new <story>` use it.

### 12.7 Composition at workspace root (on `new`/`add`)
- `.agents/skills/<name>` → **symlink** to the library skill (default; `--copy` to
  snapshot). Cross-harness, single source of truth.
- `AGENTS.md` → scope section (existing) + selected snippet sections + the per-repo
  pointers from 12.5.1.
- `.pi/settings.json` = `{ "skills": [...] }`, merged universally because
  workspaces are harness-neutral and do not have an agent target.
- `.claude/skills/<name>` mirrors `.agents/skills/<name>` and root `CLAUDE.md`
  mirrors the composed guidance for Claude-native discovery.

### 12.8 Manifest additions
```jsonc
"skills":    [{ "name": "…", "source": "library|repo", "path": "…" }],
"agents_md": [{ "name": "…", "source": "library" }]
```
Records selections so `refresh`, `status`, and `template save --from` work.

### 12.9 Resolved decisions
1. Library skills default to **symlink**; `new --copy` snapshots them.
2. Workspaces remain agent-neutral, so the universal baseline, Claude mirror,
   and Pi settings are all emitted without an agent-target flag.
3. Library location is `~/.config/agentws/library/` plus ordered `library_dirs`.
4. Pi's installed documentation confirms `AGENTS.md`/`CLAUDE.md` loading; the
   explicit settings path remains as the strongest skill-discovery guarantee.

## 13. P5: workspace cleanup and uninstall

**Status: implemented and E2E-tested.**

### 13.1 Uninstall / global cleanup

```
agentws uninstall --dry-run
agentws uninstall [--force] [--include-config] [--include-binary] [--yes]
```

Every invocation prints the same complete removal plan first. `--dry-run` stops
there; an actual uninstall requires typing the exact word `uninstall`, and
`--yes` deliberately cannot bypass that prompt. The plan shows, per workspace,
its worktree count and dirty-worktree count. Dirty workspaces are preserved by
default and `--force` explicitly opts into deleting them.

Default scope is registered workspaces, the `.current` pointer, and the
workspace root when it will become empty. `--include-config` separately opts
into deleting `~/.config/agentws/` (including the reusable library), while
`--include-binary` opts into deleting the running executable. Targets are
validated before any removal. If Git refuses a worktree removal, the containing
workspace is retained and config/binary cleanup stops.

### 13.2 Bulk delete

`agentws delete [ws...] [--all] [--dry-run] [--force] [--yes]` supports:

- exact names for scripts, or a fuzzy multi-select when no names are supplied;
- `--all`, which always requires typing the eligible workspace count even when
  `--yes` is present;
- a shared plan that reports workspace, worktree, and dirty-worktree counts;
- preservation of dirty workspaces in picker/multi-name/`--all` modes unless
  `--force` is supplied; single-name deletion retains its legacy force behavior;
- best-effort processing across a batch with explicit failures and retained
  workspace directories when Git worktree removal fails; branches are kept.

The MCP/Pi-native `delete_workspaces` tool shares this planner. It requires exact
workspace names, defaults to `dry_run=true`, and requires both `dry_run=false`
and `confirmed=true` for an actual deletion. The human-only `--all` operation is
not exposed through MCP.
