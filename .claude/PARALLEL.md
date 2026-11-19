# Parallel Claude Code cheat sheet

Flags below were checked against `claude --help`; re-check there if unsure.

- `claude -w <name>`: new session in a fresh git worktree (`.claude/worktrees/<name>`). `--tmux` adds a tmux session (needs `-w`).
- `claude --bg --worktree <name> "task"`: same, started in the background; prints an id.
- `claude agents`: list and manage background sessions.
- `claude attach <id|name>`, `logs <id|name>`, `stop <id>`, `rm <id>`: drive one session.
- Ctrl+B: background a running task (not in `--help`; check `/help` for your version).
- Subagents in `.claude/agents/`: `rule-implementer` (code + Reference edit, one commit) and `docs-syncer` (audit one page). Both use `isolation: worktree`.

## N independent features
1. One worktree per feature: `claude --bg --worktree feat-a "..."`, then `feat-b`, etc.
2. Keep them on disjoint files (different rules and pages). Two touching one Reference page will conflict.
3. `claude agents` to watch; review each branch's diff when done.

## Rules
- `data/` is a shared symlink: never write to it from a worktree.
- Stage by path (`git add rules/x.toml site/reference/y.json site/data/...`), never `git add -A`.
- A code change and its Reference page edit land in the same commit.
- Checks before landing: `python3 tools/site_data.py`, `node site/check.mjs`, `node site/test.mjs`, `cargo run --release -- score --check-readme`.
- No PRs.

## Land (linear history, no merge commits)
```
git rebase main                       # in the feature worktree
git merge --ff-only <branch>          # run in the main checkout, on main
```
Land one branch at a time; rebase the next onto the new main.

## Clean up
```
git worktree remove .claude/worktrees/<name>
git branch -d <branch>
```
Background sessions: `claude rm <id>` (removes its worktree when safe).
