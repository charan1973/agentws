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

### 3.2 Soft scope, not a hard sandbox

The agent is **not** locked in. We get scoping three ways:
1. **Physical**: each story's repos live as worktrees *under one per-story
   directory*. The agent runs from that directory, so it sees only those repos by
   default — no grep pollution, for free.
2. **Request/approve**: when the agent needs another repo, it asks; a human
   approves; the repo appears in-scope.
3. **Native prompts**: for any stray out-of-scope read, the agent's own permission
   system prompts you.

We skip FUSE / mount-namespaces / microVMs (macOS has no mount namespaces, and
we explicitly don't want a hard restriction).

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

> ✅ **STATUS:** verified working in bash + zsh via `eval "$(agentws init-shell zsh)"`.
> Fish is implemented but untested on this machine. See `HANDOFF.md` §4.

### 3.4 Hybrid expansion: MCP tool + CLI share one manifest *(deferred wiring)*

Two ways to add a repo mid-session, both writing the same `workspace.json`:
- **Agent-initiated** (MCP tool): agent calls `request_repo(name)`; supervisor/
  notification prompts you; on approval the worktree is created.
- **Human-initiated** (CLI): you run `agentws add <repo>` directly.

> The MCP server is implemented & E2E-tested but **not yet wired into agents**
> (user said "leave the mcp for now").

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
    expansion (★)         + CLI (request/approve/deny/pending) +
                         macOS notifications (fired by the MCP tool)
L3  Polish / seamless    tmux inline approval, SQLite manifest + history,
                         pi extension (native request flow + scoped-grep guard),
                         fish verification, symlink node_modules/.env, per-repo hooks
```

★ = the differentiator.

## 6. Data model — `~/.agentws/<story>/workspace.json`

```jsonc
{
  "story": "auth-payments",
  "root": "/Users/charanv/.agentws/auth-payments",
  "created": "2026-06-26T12:00:00Z",
  "repos": [
    {
      "name": "api",
      "origin": "/Users/charanv/work/api",
      "worktree": "/Users/charanv/.agentws/auth-payments/api",
      "branch": "feat/auth-payments",
      "base": "main"
    }
  ],
  "requests": [
    {
      "id": "ab12",
      "repo": "payments-service",
      "reason": "need to update the payment client",
      "status": "approved",            // pending | approved | denied
      "by": "agent",                   // agent | human
      "created": "2026-06-26T12:00:00Z",
      "resolved": "2026-06-26T12:01:30Z"
    }
  ],
  "archived": false
}
```
No `agent` field (that was removed in the activation pivot). v1: JSON + `flock` for
concurrent writers (MCP server, CLI). P3: SQLite. Old manifests written with an
`agent` field still load — serde ignores unknown fields (covered by a test).

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

**P3 upgrade:** run the agent in a `tmux` pane (claude-squad/wtmux style) so the
supervisor can prompt inline in another pane — seamless y/n without a second
terminal.

## 8. Command surface

```
agentws new <story> [--repos a,b,c] [--base main]   # create worktrees
agentws list                                  # all workspaces (* marks active)
agentws status [story]                        # repos, branches, dirty, pending
agentws open <story>                          # print root path (cd via shell wrapper)
agentws use <story>                           # set the global active pointer
agentws delete <story> [--yes]                # remove worktrees + manifest

# expansion (human)
agentws add <repo> [--story s] [--base b]     # add a repo now
agentws remove <repo> [--story s]
agentws archive <story> | restore <story>

# expansion (permission-gated)
agentws request <repo> [--reason] [--story s]
agentws pending
agentws approve <id|repo> [--story s] | deny <id|repo> [--story s]

# conda-style activation (sourced shell function, NOT a binary subcommand)
#   eval "$(agentws init-shell zsh)"; then:
agentws activate <story> [command...]         # cd in + set $AGENTWS_WORKSPACE (run cmd? one-shot)
agentws deactivate

# integration / introspection
agentws mcp                                   # run the MCP server (stdio)
agentws mcp-config <agent>                    # print MCP wiring snippet
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
- [x] Manifest (`workspace.json`) read/write + `flock`
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
- [x] **Conda-style `activate`/`deactivate`** — sourced shell function via `init-shell` (bash+zsh verified, fish implemented). `activate <story> [cmd]` does cd+env or one-shot passthrough.
- [x] `$AGENTWS_WORKSPACE` added to `resolve_story` (priority #2); per-shell scoping, like `conda activate`.
- [x] Hidden `_list-stories` subcommand to feed shell completion.
- [x] **Test suite**: 20 unit tests (util/config/ops/discovery/worktree/manifest) + 1 integration test (`resolve_story` priority chain, its own process so HOME/env mutation is safe). `cargo test` green.
- [x] Doc cleanup (PLAN.md, README.md, HANDOFF.md).

### P3 — Seamless + pi-native *(deferred)*
- [ ] tmux inline approval (agent in one pane, prompt in another)
- [ ] SQLite manifest + request history (JSON+flock is fine for v1)
- [ ] `agentws` **pi extension** (pi has no built-in MCP; MCP server covers claude/codex/opencode; pi needs the extension for native integration)
- [ ] **MCP wiring into agents** (server works + is E2E-tested, but not added to any agent config yet)
- [ ] env-file cross-repo wiring (multree-style)
- [ ] verify **fish** activation (implemented in `init-shell`, untested — no fish on dev host)
- [ ] optional hard-sandbox escape hatch

> Note: `resume`/reattach is **solved by design** — agents key sessions by cwd,
> so `pi -c` / `/resume` from inside an activated workspace just works. Removed
> from the list.

## 10. Project structure

```
agentws/
├── Cargo.toml
├── PLAN.md  README.md  HANDOFF.md
├── src/
│   ├── lib.rs           # pub modules (unit + integration tests target this)
│   ├── main.rs          # thin binary: SIGPIPE reset → cli::run()
│   ├── cli.rs           # clap command definitions + dispatch
│   ├── config.rs        # load/parse config.toml (roots, symlinks, hooks, paths)
│   ├── discovery.rs     # find repos under roots
│   ├── manifest.rs      # workspace.json + flock + resolve_story chain + .current
│   ├── worktree.rs      # git worktree add/remove, default-branch, dirty check
│   ├── ops.rs           # shared: add/remove repo, symlinks, hooks, request ids
│   ├── picker.rs        # ratatui fuzzy multi-select
│   ├── mcp.rs           # MCP stdio server (3 tools) — dormant, works
│   ├── util.rs          # tilde expansion
│   └── commands/
│       ├── new list status open delete use_ws
│       ├── add remove expand lifecycle
│       ├── config discover              # introspection
│       ├── mcp_config completions
│       ├── init_shell                   # conda-style activate/deactivate function
│       └── mod.rs
└── tests/
    └── resolve_priority.rs             # resolve_story priority-chain (own process)
```

## 11. Dependencies

| crate          | use                                   |
|----------------|---------------------------------------|
| clap           | CLI parsing (derive)                  |
| clap_complete  | shell completion generation           |
| serde          | (de)serialize                         |
| serde_json     | manifest                              |
| toml           | config file                           |
| anyhow         | error handling                        |
| directories    | XDG/home paths                        |
| ratatui        | picker TUI                            |
| crossterm      | terminal I/O for picker               |
| fuzzy-matcher  | fuzzy filter in picker                |
| fs2            | file locking for manifest             |
| chrono         | timestamps in manifest/requests       |
| libc           | SIGPIPE reset (unix)                  |
| tempfile (dev) | unit + integration tests              |
