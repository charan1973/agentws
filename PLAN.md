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

### 3.2 Soft scope, not a hard sandbox  *(see HANDOFF.md §4 re: activate bug)*

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

> 🔴 **STATUS:** the `init-shell` shell function has a runtime bug — see
> `HANDOFF.md` §4. Build compiles; activate/deactivate not yet functional.

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
launch the agent **from that root**:

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
L1  Agent adapters       Claude Code / Codex / OpenCode / pi launchers;
                         auto-wire `agentws mcp` into each agent's MCP config
L2  Permission-gated     MCP server (request_repo/list_available_repos/
    expansion (★)         check_request) + CLI (request/approve/deny/pending) +
                         supervisor watcher + macOS notifications
L3  Polish / seamless    tmux inline approval, SQLite manifest + history,
                         pi extension (native request flow + scoped-grep guard),
                         symlink node_modules/.env, per-repo setup hooks
```

★ = the differentiator.

## 6. Data model — `~/.agentws/<story>/workspace.json`

```jsonc
{
  "story": "auth-payments",
  "root": "/Users/charanv/.agentws/auth-payments",
  "agent": { "kind": "claude", "pid": 12345 },
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
  ]
}
```
v1: JSON + `flock` for concurrent writers (MCP server, CLI, supervisor). P3:
SQLite.

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
2. **Supervisor** (background process started by `agentws launch`) watches the
   manifest → on a pending request, posts a **macOS notification** + bell:
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
agentws new <story> [--repos a,b,c] [--agent claude] [--base main] [--no-launch]
agentws list                                  # all workspaces
agentws status [story]                        # repos, branches, dirty, pending
agentws open <story>                          # print root path (cd via shell wrapper)
agentws delete <story>                        # remove worktrees + manifest
agentws launch <story>                        # (re)start agent + supervisor

# P1
agentws add <repo> | remove <repo>            # human-initiated expansion
agentws archive <story> | restore <story>

# P2 — expansion
agentws mcp                                   # run as MCP server (stdio)
agentws request <repo> [--reason]             # agent-facing (records pending)
agentws pending                               # list pending requests
agentws approve <id> | deny <id>

# shared
agentws config                                # show / edit config + repo cache
agentws discover                              # refresh repo cache
```

## 9. Phasing

### P0 — Workspace core *(done — committed `678d95f`)*
- [x] Cargo crate scaffold, CLI skeleton (clap)
- [x] Config loading (`~/.config/agentws/config.toml`, auto-writes an example)
- [x] Repo discovery + skip-list over configured roots
- [x] `ratatui` fuzzy multi-select picker (`fuzzy-matcher`)
- [x] `git worktree add`/`remove`, default-branch detection, idempotent reuse
- [x] Manifest (`workspace.json`) read/write + `flock`
- [x] `new` / `list` / `status` / `open` / `delete`
- [x] `AGENTS.md` scope stub injected at workspace root
- [x] Launch adapter: Claude Code from root (default agent) — `agent::launch`
- [x] End-to-end smoke test (`new`/`list`/`status`/`open`/`delete` verified)

> Manual checks still owed: try the **interactive picker** in a real TTY
> (`agentws new x` with no `--repos`), and a real `claude` launch (spawn path
> is implemented but not yet run live against the agent).

### P1 — Adapters + quality of life *(done)*
- [x] Codex / OpenCode / pi launchers via `--agent` (generic `agent::launch`)
- [x] `mcp-config <agent>` prints wiring snippets (claude/codex/opencode/pi) — safer than auto-editing user configs
- [x] `add` / `remove` (human-initiated expansion), `archive` / `restore`
- [x] `launch` to (re)start an agent in an existing workspace
- [x] `symlinks` config (glob) + `post_create` hook, applied on create/add/restore
- [x] Shell completions (`completions`) + `init-shell` cd helper
- [x] **Active-workspace pointer** (`~/.agentws/.current`): commands run from *any* directory resolve the target via `--story` flag → cwd → active pointer → single-workspace → helpful error. Set on `new`/`use`/`launch`; cleared on `delete`; shown as `*` in `list`
- [x] SIGPIPE handled (no broken-pipe panic when piping to `head`)

### P2 — Permission-gated expansion (the differentiator) *(done)*
- [x] `agentws mcp` server: `list_available_repos`, `request_repo`, `check_request` (hand-rolled stdio JSON-RPC 2.0)
- [x] `request` / `approve` / `deny` / `pending` CLI
- [x] macOS notification **fired directly from the MCP `request_repo` tool** (see design note below — supervisor dropped)
- [x] Approval → worktree-under-root + manifest update + path returned to agent
- [x] E2E verified: agent requests → `check_request` pending → `approve` → worktree on `feat/<story>` → `check_request` returns path, no restart

### P3 — Seamless + pi-native *(deferred — see note)*
- [ ] tmux inline approval (agent in one pane, prompt in another)
- [ ] SQLite manifest + request history (JSON+flock is fine for v1)
- [ ] `agentws` **pi extension** (pi has no built-in MCP; MCP server covers claude/codex/opencode; pi needs the extension for native integration)
- [ ] env-file cross-repo wiring (multree-style)
- [ ] `resume` / reattach
- [ ] optional hard-sandbox escape hatch

## 10. Project structure

```
agentws/
├── Cargo.toml
├── PLAN.md
└── src/
    ├── main.rs            # entry → cli::run()
    ├── cli.rs             # clap command definitions + dispatch
    ├── config.rs          # load config.toml (roots, agent, symlinks, hooks)
    ├── discovery.rs       # find repos under roots
    ├── manifest.rs        # workspace.json read/write + lock + resolve/infer
    ├── worktree.rs        # git worktree add/remove, default-branch
    ├── ops.rs             # shared: add/remove repo, symlinks, hooks, ids
    ├── picker.rs          # ratatui fuzzy multi-select
    ├── agent.rs           # AgentKind enum + launchers
    ├── mcp.rs             # MCP stdio server (3 tools)
    ├── util.rs            # tilde expansion
    └── commands/
        ├── new.rs  list.rs  status.rs  open.rs  delete.rs
        ├── add.rs  remove.rs  launch.rs
        ├── expand.rs       # request / approve / deny / pending
        ├── lifecycle.rs    # archive / restore
        ├── mcp_config.rs  completions.rs  init_shell.rs
        └── mod.rs
```

## 11. Dependencies (P0)

| crate        | use                                   |
|--------------|---------------------------------------|
| clap         | CLI parsing (derive)                  |
| serde        | (de)serialize                         |
| serde_json   | manifest                              |
| toml         | config file                           |
| anyhow       | error handling                        |
| directories  | XDG config/cache/home paths           |
| ratatui      | picker TUI                            |
| crossterm    | terminal I/O for picker               |
| fuzzy-matcher| fuzzy filter in picker                |
| fs2          | file locking for manifest             |
| chrono       | timestamps in manifest/requests       |
