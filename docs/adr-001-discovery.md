# ADR-001: Discovery without `repo_roots` in config

**Status:** proposed (addresses a question raised before P4 work). Supersedes the
current global `repo_roots` config item.

## Question
> I don't want to add `repo_roots` to the config file. Why is it needed at all?

## What `repo_roots` does today
It bounds repo discovery so the agentws doesn't walk the whole filesystem. It is
the single source for the picker and for resolving a repo *by name*. Consumers
(verified by grep):
- `commands/new.rs` — scans configured roots → repo picker. Bails if empty.
- `ops::find_repo(name)` — scans configured roots, finds by name. Used by
  `add`, `request`/`expand`, and MCP `request_repo`.
- `mcp.rs::list_available_repos` — scans configured roots, lists for the agent.
- `commands/discover.rs` — scans configured roots, prints a table.
- `commands/config.rs` — displays them.

## Why it is *not* needed as a global config item
Nothing about a repo set is genuinely global. A workspace's repos are **context**,
and we already have two strong sources of context:

1. **cwd at `new` time** — you stand in the directory that *contains* your repos
   (e.g. `~/work`) and create the workspace from there.
2. **The manifest** — every `RepoEntry` already stores the repo's absolute
   `origin` path. The workspace already *knows* where its repos live.

So the roots can be **derived from cwd at creation and stored per-workspace**, then
read back from the manifest for everything else. No global config required for the
core flow.

## Decision
- **`new` derives discovery roots from cwd** (scan cwd's tree with the existing
  bounded depth-4 walk + skip-list). No config read.
- **`new` accepts `--root <dir>` (repeatable)** to override when cwd isn't the
  right parent, or to scan more than one location.
- **Store `roots: Vec<PathBuf>` in `workspace.json`** at creation.
- **`find_repo` / MCP `list_available_repos` / `discover` read roots from the
  active workspace's manifest** (via `resolve_story`), not from config.
- **`add` accepts an explicit path** (`agentws add ./sibling-repo` or an absolute
  path), so a repo outside any stored root needs no config either.
- **`repo_roots` is removed from config.** The config file becomes optional and
  holds only `symlinks` / `post_create` (and templates / `library_dirs` later).
  If absent, `config::load()` returns defaults — no "not configured" bail.
- Bare `agentws discover` (no active workspace) scans cwd; `--root` optional.

### Edge cases
- **cwd is itself a single git repo, not a parent of repos** → discovery finds
  only that repo. agentws prints a clear hint ("run from the directory *containing*
  your repos, or pass `--root`") rather than silently scanning the parent (explicit
  > magic).
- **Empty discovery** → same hint, with the cwd it scanned shown.
- **Stale roots** (a workspace's roots dir moved) → existing repos still work
  (their `origin` is absolute per-entry); only *new* discovery is affected, and
  `--root` overrides per-call.

## Consequences
- ✅ Zero-config core flow. No config file to create/edit just to start.
- ✅ Each workspace is self-contained (knows its roots) → portable, robust to
  later changes.
- ✅ Aligns with the activation model: derive from context, don't own global state.
- ✅ MCP / `add` / `discover` keep working, just read roots from the manifest.
- ⚠️ Minor migration: old workspaces (pre-change) have no `roots` field — treat
  the field as `#[serde(default)]` and fall back to "scan cwd" or `--root` for
  those. Existing repos in old workspaces are unaffected.

## Migration shape (when we build it)
1. Add `roots: Vec<PathBuf>` to `Workspace` (`#[serde(default)]`).
2. `new`: roots = `--root` if given else `[cwd]`; persist into manifest; drop the
   config read + "not configured" bail.
3. `ops::find_repo(name, roots)` — take roots as an arg (caller passes
   `manifest.roots`). `add_repo_to_workspace` already has the `ws`, so it passes
   `&ws.roots`.
4. `mcp.rs`: resolve active workspace, pass its roots to discovery/find_repo.
5. `discover`: roots from active workspace if any, else cwd, else `--root`.
6. Remove `repo_roots` from `Config` + example + tests; keep `symlinks`/`post_create`.
7. Update tests, README, PLAN §6 (manifest), §8 (commands), §11 (config example).

## What this is *not*
This does **not** remove the picker or bounded discovery — we still scan a bounded
set (cwd or `--root`) with the skip-list. It only removes the *global config*
declaration and makes the source of roots contextual + per-workspace.
