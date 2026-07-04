# agentws

A Rust CLI that creates per-story workspaces for AI coding agents (`pi`, `claude`, `codex`, `opencode`) across a polyrepo / microservice codebase.

Each story gets a fresh directory — `~/.agentws/<story>/` — containing `git worktree`s of only the repos that story touches. The agent is scoped by default (no token waste or context pollution from grepping the whole `work/` tree), but it can request access to another repo at any time and a human approves it.

> **Design philosophy:** `agentws` does **not** spawn agents. It owns the workspace. You `activate` it (conda-style), then run any agent natively. Resume, `/resume`, model switching — all of that just works because the agent sees the workspace as its project directory.
>
> For current development status, see [`HANDOFF.md`](HANDOFF.md).

---

## Table of contents

- [Quick start](#quick-start)
- [The activation model](#the-activation-model)
- [Commands](#commands)
- [Configuration](#configuration)
- [Permission-gated expansion](#permission-gated-expansion)
- [Tips](#tips)

---

## Quick start

```bash
# 1. install
cargo install --path . --locked
# or build and copy target/release/agentws somewhere on PATH

# 2. configure (one-time)
mkdir -p ~/.config/agentws
printf 'repo_roots = ["~/work"]\n' > ~/.config/agentws/config.toml

# 3. add shell integration (one-time per shell)
eval "$(agentws init-shell zsh)"   # or bash / fish

# 4. create a workspace
agentws new auth-payments --repos svc-auth,svc-api

# 5. activate it, then run any agent normally
agentws activate auth-payments
pi                                 # native pi, scoped to the workspace

# or one-shot passthrough
agentws activate auth-payments pi -c

# done for the day
agentws deactivate
```

---

## The activation model

Think `conda activate`, not `docker run`.

When you activate a workspace, `agentws` does three things **in your current shell**:

1. `cd ~/.agentws/<story>/`
2. `export AGENTWS_WORKSPACE=<story>`
3. remembers where you came from for `deactivate`

That's it. After that, `pi`, `claude`, `codex`, or `opencode` run normally with the workspace as their project directory. They see only the repos inside that workspace, and they use their own native session/resume logic.

Because `activate`/`deactivate` change your shell state, they are implemented as a **sourced shell function**, not as a binary. Add this to your shell rc:

```bash
# ~/.zshrc or ~/.bashrc
eval "$(agentws init-shell zsh)"
```

For fish:

```fish
# ~/.config/fish/config.fish
agentws init-shell fish | source
```

`AGENTWS_WORKSPACE` is **per-shell** (not global). Two terminal tabs can have two different active workspaces, exactly like `conda`.

---

## Commands

### Workspace lifecycle

```bash
agentws new <story> [--repos a,b,c] [--base main]   # create worktrees + activate hint
agentws list                                         # list workspaces, * marks active
agentws status [story]                               # show repos, branches, requests
agentws open <story>                                 # print workspace root path
agentws use <story>                                  # set the global active pointer
agentws delete <story> [--yes]                       # remove worktrees + manifest
```

### Working inside a workspace

Once activated (or when run from inside a workspace), these resolve the target workspace automatically:

```bash
agentws add <repo> [--story story]                   # add another repo now
agentws remove <repo> [--story story]                # drop a repo from the workspace
agentws archive <story>                              # remove worktrees, keep manifest
agentws restore <story>                              # recreate worktrees from manifest
agentws code [--story story]                          # open in VS Code (multi-root)
```

`agentws code` opens the workspace in VS Code via a generated multi-root
`.code-workspace` (one top-level folder per repo worktree), so each worktree
shows as a flat root and gets its own Source Control entry — handy for diffing
a single worktree against `main` in the GUI. The file is regenerated on
`new`/`add`/`remove`/`restore` and removed on `archive`; `agentws code` also
recreates it if missing. Like the others, `story` is inferred from
`$AGENTWS_WORKSPACE` / cwd, so a bare `agentws code` from inside an activated
workspace just works.

### Activation (sourced shell function)

`activate`/`deactivate` are **shell functions**, not binary subcommands — load them
once via [`agentws init-shell`](#integration) (see [The activation model](#the-activation-model)):

```bash
agentws activate <story>            # cd into the workspace + set $AGENTWS_WORKSPACE
agentws activate <story> pi -c      # one-shot: run a command there, then return
agentws deactivate                  # restore cwd + unset env
```

### Permission-gated expansion

```bash
agentws request <repo> [--story story] [--reason ...] # queue a request
agentws pending                                       # show pending requests
agentws approve <id-or-repo> [--story story]          # create worktree + mark approved
agentws deny <id-or-repo> [--story story]             # mark denied
```

### Integration

```bash
agentws mcp                                            # run the MCP server (stdio)
agentws mcp-config claude                              # print MCP wiring snippet
agentws completions zsh                                # generate shell completions
agentws init-shell zsh                                 # print activate/deactivate function
agentws config                                         # show resolved config + key paths
agentws discover                                       # list discovered repos
```

---

## Configuration

`~/.config/agentws/config.toml`:

```toml
# Roots that will be scanned for git repositories.
repo_roots = ["~/work"]

# Default base branch to create story branches from
# (default: each repo's default branch).
# default_base = "main"

# Paths to symlink from each original repo into its worktree.
# symlinks = ["node_modules", ".env*"]

# Shell command run inside each worktree right after creation.
# post_create = "npm ci"
```

---

## Permission-gated expansion

The differentiator: an agent can discover it needs another repo mid-session, request it, and a human approves it.

Two ways to add a repo:

1. **Human-initiated:** `agentws add <repo>`
2. **Agent-initiated:** the agent uses the `agentws` MCP server (tools: `list_available_repos`, `request_repo`, `check_request`).

When the agent calls `request_repo`, `agentws` writes a pending request to the manifest and fires a notification. The human runs `agentws approve <id>` in any terminal. On approval, a worktree is created **under the workspace root**, so it appears in-scope to the running agent with no restart.

To wire the MCP server into an agent:

```bash
agentws mcp-config claude   # prints the snippet for ~/.claude.json
agentws mcp-config codex    # prints the snippet for ~/.codex/config.toml
```

For `pi`, there is no built-in MCP; use the CLI commands directly or build a pi extension.

---

## Tips

- **Always activate.** Commands like `add`, `request`, `status` resolve the workspace in this order: `--story` flag → `$AGENTWS_WORKSPACE` env var → cwd inside a workspace → the global `.current` pointer → single workspace.
- **Resume is free.** Because agents key sessions by project directory, `pi -c` or `/resume` inside an activated workspace resumes that workspace's conversation automatically.
- **Piping is safe.** `agentws list | head` exits quietly instead of panicking on a broken pipe.
- **Switching stories.** `agentws use <story>` sets the global pointer for commands run outside an activated shell. Inside an activated shell, the env var wins.

---

## Development

```bash
cargo build                      # build
cargo test                       # run unit + integration tests (21 tests)
cargo clippy --all-targets       # lint
cargo install --path . --locked  # install to ~/.cargo/bin/agentws
```

The crate exposes both a binary (`src/main.rs`) and a library (`src/lib.rs`), so
the modules are testable: in-module unit tests (`#[cfg(test)]`) cover the pure
functions and the git/worktree/discovery/manifest logic, and an integration test
under `tests/` (`resolve_priority.rs`) runs in its **own process** so it can
mutate `HOME` / `$AGENTWS_WORKSPACE` / cwd without racing other tests.

---

## Status

See [`HANDOFF.md`](HANDOFF.md) for the current development state, including known issues and the next steps.
