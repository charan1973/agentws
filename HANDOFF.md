# agentws — Handoff / Status

**Date:** 2026-06-29 (updated)
**State:** Conda-style activation pivot **complete and verified** (bash + zsh). Build clean. Installed at `~/.cargo/bin/agentws`.

> Read this first, then `PLAN.md`. This file is the source of truth for where
> things stand right now.

---

## TL;DR

- The project **pivoted architecture**: the old "agentws launches the agent" model
  was ripped out and replaced with a **conda/venv-style activation model** (workspace
  owns the dir; you `activate` then run any agent natively).
- **Build compiles clean.** Installed at `~/.cargo/bin/agentws`.
- **`activate`/`deactivate` WORKS** — verified in bash + zsh (full flow: activate →
  cwd+env set, `status` resolves via env, passthrough, deactivate restores,
  per-shell isolation). See §4 for the earlier "blocker" — it was a **test
  artifact, not a code bug**.
- 6 commits on `main`; latest folds in the zsh `compdef` guard + verification.

---

## 1. What agentws is

A Rust CLI that creates per-story workspaces for AI coding agents (pi/claude/codex/
opencode) across a polyrepo/microservice codebase. Each story = a fresh directory
`~/.agentws/<story>/` containing `git worktree`s of only the repos that story
touches, so the agent is scoped by default (no token waste / context pollution from
grepping the whole `work/` tree). Mid-session, an agent can *request* another repo
and a human approves it.

---

## 2. The architecture pivot

### Old model (REMOVED)
agentws *owned* the agent process: it spawned the agent (`agent::launch`), tracked
an `agent.kind`/`pid` in the manifest, and tried to manage resume. This duplicated
the agent, blocked its native features, and made resume a per-agent nightmare.

### New model (conda/venv style — DONE)
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
- `init-shell` emits a conda-style `agentws` shell function (`activate`/
  `deactivate`/delegate), for bash/zsh/fish. **Verified in bash + zsh.**
- `AGENTWS_WORKSPACE` added to the top of `resolve_story`'s fallback chain.
- Hidden `_list-stories` subcommand to feed shell completion of `activate`.
- `new` now just creates the workspace + prints the activate hint (no launch).

### Resolution chain for "which workspace?" (priority order)
1. `--story <name>` flag (explicit)
2. **`$AGENTWS_WORKSPACE`** (per-shell; set by `activate`)
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
  **implemented and E2E-tested** over real JSON-RPC. (User deferred wiring it into
  agents, so it's dormant but functional.)
- **Run from any dir:** the full `--story`/env/cwd/`.current`/single-workspace chain.
  SIGPIPE handled.
- **Conda-style activation (the pivot):** ✅ bash + zsh verified end-to-end:
  - `eval "$(agentws init-shell zsh)"` defines the function.
  - `agentws activate <story>` → `cd` in + `export AGENTWS_WORKSPACE`.
  - `agentws activate <story> <cmd>` → one-shot passthrough (runs, returns).
  - `agentws deactivate` → restores cwd, unsets env.
  - commands like `agentws add`/`status` resolve the workspace via the env var
    (no `--story` needed) while activated.
  - per-shell isolation (two shells, two workspaces).
  - `agentws activate nope` → clean error, rc=1.

## 4. The earlier "blocker" — RESOLVED (was a test artifact) ✅

**Symptom seen last session:** `activate`/`deactivate` didn't work — `type agentws`
still showed the binary, and calls errored `unrecognized subcommand 'activate'`.

**Root cause:** NOT a code bug. The test invoked
`source <(agentws init-shell bash)` (**process substitution**). In bash, sourcing a
function definition from a FIFO (proc-sub) does **not** reliably define the
function — `type -t agentws` returned `file`, not `function`.

**The fix was already the documented usage:** use the idiomatic `eval` form, which
is what the README recommends and what conda/starship/pyenv use:

| Form | Result |
|------|--------|
| `eval "$(agentws init-shell bash)"` (README form) | ✅ function defined, full flow works |
| `source <(agentws init-shell bash)` (proc-sub) | ❌ bash FIFO quirk — function not defined |
| `source /tmp/file` | ✅ works |

The emitted shell code is correct: `bash -n` passes, `source <file>` defines the
function, and `eval "$(...)"` works perfectly in both bash and zsh.

**One real fix made during verification:** zsh printed `command not found: compdef`
in a non-interactive shell, because `compdef` only exists after `compinit` runs.
Guarded the `compdef _agentws agentws` call so it silently skips when compinit
hasn't run:
```zsh
if (( $+functions[compdef] )); then
  compdef _agentws agentws
fi
```
Now there's no warning in `zsh -c`, and completion still installs in interactive
shells where compinit has run.

**Fish:** implemented in `init_shell.rs` (`print_fish`) but **untested** — fish is
not installed on this machine. Verify when convenient.

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
    └── init_shell.rs   # conda-style activate/deactivate function (bash+zsh verified)
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

# shell integration (one-time, in your rc file)
eval "$(agentws init-shell bash)"   # or zsh

# core
agentws new my-story --repos svc-a,svc-b
agentws list

# the conda flow (verified working)
agentws activate my-story        # cd in + set $AGENTWS_WORKSPACE
agentws activate my-story pi     # run pi in workspace, return
agentws deactivate
```

---

## 7. Commit state

- 6 commits on `main`. The pivot, the zsh `compdef` guard, README, and this status
  are all committed.
- Tree is clean after the verification commit.

---

## 8. Deferred / out of scope (per user)

- pi MCP extension (user said "leave the mcp for now").
- Fish verification (implemented, untested here).
- tmux inline approval, SQLite manifest, env-file cross-repo wiring, optional hard
  sandbox, prompt `[story]` marker, auto-activate on `new`. All noted in PLAN.md P3.
