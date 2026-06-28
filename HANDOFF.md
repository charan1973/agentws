# agentws — Handoff / Status

**Date:** 2026-06-28
**State:** Mid-pivot. Build compiles. **`activate`/`deactivate` has a runtime bug (not yet resolved).**

> Read this first, then `PLAN.md`. This file is the source of truth for where
> things stand right now.

---

## TL;DR

- The project **pivoted architecture** mid-build (see §2). The old "agentws launches
  the agent" model was ripped out and replaced with a **conda/venv-style activation
  model** (workspace owns the dir; you `activate` then run any agent natively).
- **Build compiles clean.** Installed at `~/.cargo/bin/agentws`.
- **The conda-style `activate`/`deactivate` is NOT working yet** — §4 documents the
  bug. This is the blocker. Everything else works.
- 4 commits on `main`; the pivot work is **uncommitted** in the working tree.

---

## 1. What agentws is

A Rust CLI that creates per-story workspaces for AI coding agents (pi/claude/codex/
opencode) across a polyrepo/microservice codebase. Each story = a fresh directory
`~/.agentws/<story>/` containing `git worktree`s of only the repos that story
touches, so the agent is scoped by default (no token waste / context pollution from
grepping the whole `work/` tree). Mid-session, an agent can *request* another repo
and a human approves it.

---

## 2. The architecture pivot (IMPORTANT — most recent work)

### Old model (REMOVED)
agentws *owned* the agent process: it spawned the agent (`agent::launch`), tracked
an `agent.kind`/`pid` in the manifest, and tried to manage resume. This duplicated
the agent, blocked its native features, and made resume a per-agent nightmare.

### New model (conda/venv style — IN PROGRESS)
agentws owns only the **workspace** (worktrees + dir). You **activate** it — a
sourced shell function that `cd`s you in and `export`s `AGENTWS_WORKSPACE` — then
you run `pi`/`claude`/`codex` yourself. agentws **never spawns an agent** (pure
passthrough). Because the agent runs natively with `cwd = ~/.agentws/<story>`, the
agent's own resume (`pi -c`, `pi -r`, `/resume`) works for free, keyed by dir.
**Resume is no longer our problem.**

`AGENTWS_WORKSPACE` is **per-shell** (set by the sourced function, not globally) —
exactly like `conda activate`. Two terminals = two independent values.

### What got removed
- `src/agent.rs` (deleted)
- `src/commands/launch.rs` (deleted)
- `Launch` CLI command (removed)
- `--agent` / `--no-launch` flags on `New` (removed)
- `default_agent` config field (removed)
- `agent` / `AgentRef` fields on the manifest (removed; `#[serde(default)]` keeps
  old manifests loadable)

### What got added
- `init-shell` now emits a conda-style `agentws` shell function (`activate`/
  `deactivate`/delegate), for bash/zsh/fish. **This has the bug — see §4.**
- `AGENTWS_WORKSPACE` added to the top of `resolve_story`'s fallback chain.
- New hidden `_list-stories` subcommand to feed shell completion of `activate`.
- `new` now just creates the workspace + prints the activate hint (no launch).

### Resolution chain for "which workspace?" (priority order)
1. `--story <name>` flag (explicit)
2. **`$AGENTWS_WORKSPACE`** (per-shell; set by `activate`)  ← NEW
3. cwd is inside a workspace dir
4. `~/.agentws/.current` (global fallback pointer)
5. exactly one workspace exists → use it
6. else: helpful error listing workspaces

---

## 3. What works (verified)

- **Workspace core (P0):** `new` (fuzzy picker or `--repos`), `list`, `status`,
  `open`, `delete`. Worktrees on `feat/<story>` branches, manifest (`workspace.json`
  + flock), `AGENTS.md` scope stub. End-to-end tested.
- **Expansion (P1+P2):** `add`/`remove` (human), `request`/`approve`/`deny`/
  `pending` (permission-gated), `archive`/`restore`. The MCP stdio server
  (`agentws mcp`: `list_available_repos`/`request_repo`/`check_request`) is
  **implemented and E2E-tested** over real JSON-RPC — agent requests → human
  approves → worktree appears in-scope, no restart. (User deferred wiring it into
  agents, so it's dormant but functional.)
- **Run from any dir (P1):** the `--story`/cwd/`.current`/single-workspace chain
  (steps 1,3,4,5) all verified. SIGPIPE handled (no broken-pipe panic).
- **Quality of life:** `use`, completions, symlink config + `post_create` hook,
  `mcp-config` snippets.

## 4. What's BROKEN — the current blocker 🔴

**The conda-style `activate`/`deactivate` shell function is not getting defined
when you source `agentws init-shell bash`.**

### Symptoms
- `agentws init-shell bash` prints a function body that *looks* correct.
- But `source <(agentws init-shell bash)` in bash does **not** define `agentws` as
  a function — `type agentws` still shows the binary path, and calling
  `agentws activate …` falls through to clap, which errors:
  `error: unrecognized subcommand 'activate'`.
- Same failure for the passthrough (`activate <story> <cmd>`), `deactivate`, and
  the env-var resolution path that depends on `activate` having set
  `AGENTWS_WORKSPACE`.

### What I ruled out
- **Not a stale-binary issue:** reproduced after `cargo install --path . --force`
  against the fresh build. The binary is current.
- **Not a compile issue:** `cargo build` is clean.

### Most likely cause (unverified — THIS is where to pick up)
The emitted bash function in `src/commands/init_shell.rs` (`print_posix`) is
probably malformed in a way that makes `source` silently fail to define it. Prime
suspects, in order:
1. The raw-string emission / the leading `agentws() {` line — something about how
   `println!` renders it makes bash reject or skip the function definition. Check
   with: `agentws init-shell bash > /tmp/a.sh; bash -n /tmp/a.sh` (syntax check)
   and `bash -x /tmp/a.sh` (trace). Also `source /tmp/a.sh; type agentws`.
2. Process-substitution `<(...)` edge case — try the file-based form above to
   remove that variable.
3. The `\$AGENTWS_WORKSPACE` backslash-escaping inside the heredoc-style raw
   string may be producing a literal `\$` that breaks the `case`/`export`. Inspect
   the *exact* bytes emitted (the earlier `cat -A` attempt failed because macOS
   `cat` has no `-A`; use `cat -v` or `od -c | head`).

### Concrete next steps when resuming
1. **Capture exact emitted bytes:** `agentws init-shell bash > /tmp/aw.sh &&
   cat -v /tmp/aw.sh | head -40`. Look for mangled escaping.
2. **Syntax-check:** `bash -n /tmp/aw.sh` — report any parse error.
3. **Source + inspect:** `bash -c 'source /tmp/aw.sh; type agentws; declare -f
   agentws | head'`.
4. Fix `src/commands/init_shell.rs` (`print_posix`, and re-check `print_fish`)
   until `source <(agentws init-shell bash)` defines the function and all 9 E2E
   cases from the earlier test pass.
5. The intended E2E (re-run once fixed) is in git history / this session:
   activate sets cwd+env, passthrough runs+returns, deactivate restores, env var
   scopes `resolve_story`, per-shell isolation, delegate still routes real
   subcommands, `_list-stories` feeds completion, bad story → clean error.

---

## 5. File map (current, after the pivot)

```
src/
├── main.rs            # SIGPIPE reset → cli::run()
├── cli.rs             # clap cmds: new/list/use/status/open/delete,
│                      #   add/remove, request/approve/deny/pending,
│                      #   archive/restore, mcp/mcp-config/completions,
│                      #   init-shell, _list-stories(hidden)
├── config.rs          # config.toml (repo_roots, symlinks, post_create)
├── discovery.rs       # walk roots for git repos
├── manifest.rs        # workspace.json + flock + resolve_story (chain) + .current
├── worktree.rs        # git worktree add/remove, default-branch, dirty check
├── ops.rs             # add/remove repo, symlinks, hooks, request ids
├── picker.rs          # ratatui fuzzy multi-select
├── mcp.rs             # MCP stdio server (3 tools) — dormant, works
├── util.rs            # tilde expand
└── commands/
    ├── new.rs list.rs status.rs open.rs delete.rs use_ws.rs
    ├── add.rs remove.rs expand.rs lifecycle.rs
    ├── mcp_config.rs completions.rs
    └── init_shell.rs   # 🔴 conda-style fn — HAS THE BUG
```
Deleted: `src/agent.rs`, `src/commands/launch.rs`.

---

## 6. How to build / install / use

```bash
cd ~/coding/projects/agentws
cargo build                              # dev
cargo install --path . --locked --force  # → ~/.cargo/bin/agentws

# config (one-time)
mkdir -p ~/.config/agentws
printf 'repo_roots = ["~/work"]\n' > ~/.config/agentws/config.toml

# core (works today)
agentws new my-story --repos svc-a,svc-b
agentws list
agentws status
cd "$(agentws open my-story)"

# the intended conda flow (BROKEN until §4 is fixed):
eval "$(agentws init-shell bash)"
agentws activate my-story        # cd in + set $AGENTWS_WORKSPACE
agentws activate my-story pi     # run pi in workspace, return
agentws deactivate
```

---

## 7. Commit state

- **Committed (4):** P0 core → docs → P1+P2 expansion → active-workspace pointer.
- **Uncommitted (working tree):** the entire architecture pivot (removed
  agent.rs/launch.rs/agent-fields; new init-shell conda function; AGENTWS_WORKSPACE
  in resolve chain; `_list-stories`). **Builds clean. activate/deactivate buggy.**
- Suggested on resume: **fix §4 first, then commit the pivot as one clean commit.**
  (Don't commit the broken activate.) If you'd rather checkpoint now, commit with a
  `wip:` prefix and amend after the fix.

---

## 8. Deferred / out of scope (per user)

- pi MCP extension (user said "leave the mcp for now").
- tmux inline approval, SQLite manifest, env-file cross-repo wiring, optional hard
  sandbox, prompt `[story]` marker, auto-activate on `new`. All noted in PLAN.md P3.
