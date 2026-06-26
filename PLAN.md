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
   directory*. The agent is launched from that directory, so it sees only those
   repos by default — no grep pollution, for free.
2. **Request/approve**: when the agent needs another repo, it asks; a human
   approves; the repo appears in-scope.
3. **Native prompts**: for any stray out-of-scope read, the agent's own
   permission system prompts you (Claude Code / Codex already do this).

We deliberately skip FUSE / mount-namespaces / microVMs (you're on macOS, which
has no mount namespaces anyway, and you explicitly don't want a hard restriction).

### 3.3 Hybrid expansion: MCP tool + CLI share one manifest

Two ways to add a repo mid-session, both writing the same `workspace.json`:
- **Agent-initiated** (MCP tool): agent calls `request_repo(name)`; a supervisor
  prompts you; on approval the worktree is created.
- **Human-initiated** (CLI): you run `agentws add <repo>` directly.

### 3.4 Branch naming: `feat/<story>` off each repo's default branch

Every repo in a story gets a branch named `feat/<story-name>` (e.g.
`feat/auth-payments`), created from that repo's default branch (usually `main`).
Override per story with `--base <branch>` if a story should branch off something
else (e.g. `develop`, or a release branch).

### 3.5 Workspace location: `~/.agentws/<story>/`

Workspaces live under `~/.agentws/`, **not** inside `work/`. This keeps your real
repo directory clean — no stray worktree folders next to your checkouts. The
agent runs from `~/.agentws/<story>/` and only ever sees that story's repos. Your
`work/` repos are the *source* the worktrees branch from; they're never touched.

### 3.6 Picker: `ratatui` fuzzy multi-select

The repo picker is a full-screen terminal UI built with `ratatui` (the standard
Rust TUI library) + `crossterm`, with fuzzy filtering (`fuzzy-matcher`). Type to
filter, Space to toggle, Enter to confirm.

### 3.7 `new` launches the agent automatically

`agentws new <story>` creates the workspace **and** starts the agent in one step,
so you're prompting immediately. `--no-launch` skips the launch if you just want
to create. `--agent <kind>` selects the agent (default: `claude`).

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

### P0 — Workspace core *(in progress)*
- [x] Cargo crate scaffold, CLI skeleton (clap)
- [x] Config loading (`~/.config/agentws/config.toml`)
- [x] Repo discovery + cache over configured roots
- [x] `ratatui` fuzzy multi-select picker
- [x] `git worktree add`/`remove`, default-branch detection
- [x] Manifest (`workspace.json`) read/write
- [x] `new` / `list` / `status` / `open` / `delete`
- [x] `AGENTS.md` scope stub injected at workspace root
- [ ] Launch adapter: Claude Code from root (default agent)
- [ ] End-to-end smoke test

### P1 — Adapters + quality of life
- [ ] Codex / OpenCode / pi launchers, `--agent`
- [ ] Auto-wire `agentws mcp` into each agent's MCP config
- [ ] `add` / `remove`, `archive` / `restore`
- [ ] `--symlink node_modules,.env` + per-repo setup hooks
- [ ] Shell completions + `agentws cd` wrapper

### P2 — Permission-gated expansion (the differentiator)
- [ ] `agentws mcp` server: `list_available_repos`, `request_repo`, `check_request`
- [ ] `request` / `approve` / `deny` / `pending` CLI
- [ ] Supervisor watcher + macOS notifications
- [ ] Approval → worktree-under-root + manifest update + path returned to agent

### P3 — Seamless + pi-native
- [ ] tmux inline approval (agent in one pane, supervisor in another)
- [ ] SQLite manifest + request history
- [ ] `agentws` **pi extension** (native request flow + optional scoped-grep guard)
- [ ] env-file wiring between repos (multree-style)
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
    ├── config.rs          # load config.toml, resolve paths
    ├── discovery.rs       # find repos under roots, cache
    ├── manifest.rs        # workspace.json read/write + lock
    ├── worktree.rs        # git worktree add/remove, default-branch
    ├── picker.rs          # ratatui fuzzy multi-select
    ├── agent.rs           # AgentKind enum + launchers
    ├── util.rs            # tilde expansion, helpers
    └── commands/
        ├── mod.rs
        ├── new.rs
        ├── list.rs
        ├── status.rs
        ├── open.rs
        └── delete.rs
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
