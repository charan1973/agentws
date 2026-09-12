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
- [Skills, guidance, and templates](#skills-guidance-and-templates)
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
eval "$(agentws init-shell zsh)"   # zsh or bash; fish is shown below

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
agentws new <story> [--repos a,b,c] [--base main]   # interactive repos → skills → guidance
  [--skills x,y] [--agents-md x,y] [--template t] [--copy]
agentws list                                         # list workspaces, * marks active
agentws status [story]                               # show repos, branches, requests
agentws open <story>                                 # print workspace root path
agentws use <story>                                  # set the global active pointer
agentws delete [story...] [--dry-run] [--force] [--yes]   # remove worktrees + manifest;
                                                     # bare `delete` = fuzzy multi-select;
                                                     # dirty worktrees kept unless --force
```

### Working inside a workspace

Once activated (or when run from inside a workspace), these resolve the target workspace automatically:

```bash
agentws add <repo> [--story story]                   # add another repo now
agentws remove <repo> [--story story]                # drop a repo from the workspace
agentws rewire [--story story]                       # refresh cross-repo dotenv values
agentws refresh [--story story]                      # recompose guidance + skills/settings
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
agentws history                                       # append-only request audit trail
agentws approve <id-or-repo> [--story story]          # create worktree + mark approved
agentws deny <id-or-repo> [--story story]             # mark denied
agentws approvals [--story story]                     # watch + resolve requests interactively
agentws approvals --tmux [--story story]              # open watcher in a tmux pane
```

### Integration

```bash
agentws mcp                                            # run the MCP server (stdio)
agentws mcp-config claude                              # print MCP wiring snippet
agentws integrate pi claude opencode                  # install project-local integrations
agentws integrate codex                               # install Codex MCP entry (user-global)
agentws completions zsh                                # generate shell completions
agentws init-shell zsh                                 # print activate/deactivate function
agentws config                                         # show resolved config + key paths
agentws discover                                       # list discovered repos
```

`integrate` preserves unrelated settings and refuses to replace a conflicting
`agentws` entry unless `--force` is supplied. Pi gets a native project extension;
Claude and OpenCode get project-local MCP configuration. Codex's supported MCP
CLI writes its user-level configuration, so it is only changed when `codex` is
explicitly selected.

For strict, opt-in filesystem isolation on macOS:

```bash
agentws sandbox -- pi                    # workspace + Git metadata only; network denied
agentws sandbox --allow-network -- pi    # opt network access back in
agentws sandbox --allow-read ~/docs -- command args...
```

The sandbox creates a private temporary directory and denies reads and writes
outside the workspace, required Git metadata, system runtime paths, and any
explicit `--allow-read`/`--allow-write` paths. It uses Apple's deprecated
`sandbox-exec`, so it is an escape hatch rather than the default scoping model.

### Library and templates

```bash
agentws library list
agentws library add ./my-skill                 # skill dir containing SKILL.md
agentws library add ./house-style.md           # reusable AGENTS.md section
agentws library add ./full-stack.toml           # workspace template
agentws library remove review [--kind skill]

agentws template list
agentws template show full-stack
agentws template save full-stack --from auth-payments
agentws template edit full-stack
agentws template delete full-stack
```

## Skills, guidance, and templates

The built-in library lives at `~/.config/agentws/library/`:

```text
library/
├── skills/<name>/SKILL.md
├── agents/<name>.md
└── templates/<name>.toml
```

`agentws new` reuses the existing fuzzy multi-select in this order: repositories,
skills, then AGENTS.md snippets. The corresponding flags make any stage
non-interactive. Library skills are symlinked into the workspace by default so
source edits propagate immediately; pass `--copy` for a point-in-time snapshot.

Every creation and `agentws refresh` composes:

- root `AGENTS.md` and `CLAUDE.md` with workspace scope, selected snippets, and
  pointers to each repository's own instruction files;
- library and namespaced repository-local skills under `.agents/skills/`, plus a
  Claude-compatible `.claude/skills/` mirror;
- a merged `.pi/settings.json` whose explicit `skills` entries point at the
  managed root pool while preserving unrelated Pi settings.

`new`, `add`, `remove`, `approve`, and `restore` refresh composition
automatically. Run `agentws refresh` after a Git pull or library edit that changes
repo-local skills/instructions. Pi requires you to approve its project-trust
prompt on first use; agentws cannot pre-trust a project.

A template can contain all or part of the creation setup:

```toml
repos = ["api", "web", "svc-*"]
skills = ["react-testing"]
agents_md = ["house-style"]
base = "main"
symlinks = ["node_modules"]
post_create = "pnpm install"
copy_skills = false
```

Missing repository/skill/guidance fields fall back to the interactive picker.
Names and glob selectors are resolved at creation time. To make bare
`agentws new <story>` use a preset, configure:

```toml
[default]
template = "full-stack"
```

---

## Configuration

`~/.config/agentws/config.toml`:

```toml
# Roots that will be scanned for git repositories.
repo_roots = ["~/work"]

# Additional library roots using skills/, agents/, templates/ subdirectories.
library_dirs = ["~/team-agentws-library"]

# Default base branch to create story branches from
# (default: each repo's default branch).
# default_base = "main"

# Paths to symlink from each original repo into its worktree.
# symlinks = ["node_modules", ".env*"]

# Shell command run inside each worktree right after creation.
# post_create = "npm ci"

# Optional cross-repo dotenv wiring. `api` exposes PORT as `api.port`;
# `web` consumes it and falls back to 5000 when api is not in the workspace.
[env.api]
file = ".env.local"
exposes = { port = "PORT" }

[env.web]
file = ".env.local"
consumes = { API_URL = "http://localhost:{api.port}" }
defaults = { "api.port" = "5000" }

[default]
template = "full-stack"
```

Managed values are kept in a clearly marked block, preserving the rest of the
dotenv file. Wiring runs after workspace membership changes and can be refreshed
manually with `agentws rewire`. Env paths must stay inside their repo and may not
traverse symlinks.

---

## Permission-gated expansion

The differentiator: an agent can discover it needs another repo mid-session, request it, and a human approves it.

Two ways to add a repo:

1. **Human-initiated:** `agentws add <repo>`
2. **Agent-initiated:** the agent uses the `agentws` MCP server (tools: `list_available_repos`, `request_repo`, `check_request`).

When the agent calls `request_repo`, `agentws` writes a pending request to the manifest and fires a notification. The human runs `agentws approve <id>` in any terminal. On approval, a worktree is created **under the workspace root**, so it appears in-scope to the running agent with no restart.

For inline approval while using tmux, start a watcher before launching the agent:

```bash
agentws activate auth-payments
agentws approvals --tmux   # opens a small watcher pane; current pane stays active
pi
```

The watcher focuses its pane and rings the terminal bell when a request arrives,
then accepts `a`/`y` to approve, `d`/`n` to deny, `s` to skip it for the current
watcher session, or `q` to close the watcher. Without tmux, run
`agentws approvals` in another terminal for the same live prompt.

To wire the native/MCP tools into an agent for the active workspace:

```bash
agentws integrate pi               # .pi/extensions/agentws.ts
agentws integrate claude opencode  # .mcp.json + opencode.json
agentws integrate codex            # Codex user-level MCP registration
```

Pi's generated extension registers the three repo tools natively and prompts
before path-based file tools leave the workspace. The other harnesses use the
stdio MCP server. `agentws mcp-config <agent>` remains available when you prefer
to copy a snippet manually.

Workspace state is authoritative in `workspace.db` (SQLite/WAL). Existing
`workspace.json` manifests migrate automatically on first access and are kept as
a backup. `agentws history` reads the append-only request event log.

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
cargo test                       # run unit + integration tests (59 tests)
cargo clippy --all-targets -- -D warnings
cargo install --path . --locked  # install to ~/.cargo/bin/agentws
```

The crate exposes both a binary (`src/main.rs`) and a library (`src/lib.rs`), so
the modules are testable: in-module unit tests (`#[cfg(test)]`) cover the pure
functions and the git/worktree/discovery/manifest logic. The test suite covers
story resolution, bulk deletion, concurrent SQLite writers, tmux approval, real
Fish activation, Pi extension loading, macOS Seatbelt isolation, and the full
P4 library/template/composition lifecycle.

---

## Status

See [`HANDOFF.md`](HANDOFF.md) for the current development state, including known issues and the next steps.
