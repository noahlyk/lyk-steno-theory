# Handoff plans

Self-contained task write-ups for a fresh agent: read the one you are assigned, not the others,
unless it points you at another. The docs they mention as `docs/*.md` are the Reference pages
(`site/reference/<id>.json`) now; the map is in [`../docs/README.md`](../docs/README.md).

| Plan | What | Touches | Status |
|---|---|---|---|
| [01-suffix-expansion.md](01-suffix-expansion.md) | Grow `rules/suffixes.toml` | `rules/suffixes.toml`, `rules/plover.toml` (prefixes), Reference `suffixes` | done |
| [02-brief-expansion.md](02-brief-expansion.md) | More words worth a deliberate brief, with a keep/reject rule | `rules/briefs.toml`, Reference `briefs` | done |
| [03-docs-restructure.md](03-docs-restructure.md) | Reorder the docs into a sequential read | all docs | done: the docs are the site's lessons and Reference |
| [04-claude-md-docs-importance.md](04-claude-md-docs-importance.md) | `.claude/CLAUDE.md`: docs outrank code | `.claude/CLAUDE.md` | done |
| [05-kpa-family-consistency.md](05-kpa-family-consistency.md) | A consistent capitalize/lowercase stroke scheme | `rules/reserved/commands.json`, deployed config | **awaiting a decision** among three candidate schemes |
| [06-phrase-briefs.md](06-phrase-briefs.md) | Scope a lyk-native phrase-brief system | new subsystem | scoping doc; its step 2 is built as plan 08 |
| [07-next-phase.md](07-next-phase.md) | keymux tape+undo fix, multi-file output and per-file reload, frequency, `+` redesign, phrasing, housekeeping | `src/`, `rules/`, `../keymux/` | done (workstreams 1-7; 5 is plan 08) |
| [08-phrasing-grammar.md](08-phrasing-grammar.md) | Starter + auxiliary + verb in one stroke | `rules/grammar.toml`, `src/phrases/grammar.rs`; `tools/phrasing_prototype.py` is the spec it is checked against | **built** (3,758 phrases in `02-lyk-phrasing.json`); open: questions, more starters and verbs |
| [09-wasm-fallback-resolver.md](09-wasm-fallback-resolver.md) | The solver plugin: decode a missed `+`-run into its word, so `deploy` ships no backups | `src/rules/solve/`, `src/solve.rs`, Reference `fallback` | solver **built and on `main`** (deploy ships it, no backups); the revised plan text (spelled-strokes third, commit `8de0b62`) is only on `origin/experiment/wasm-fallback-resolver`, unmerged |

## Ground rules

- Repo: `/home/fib/code/noahlyk/lyk-steno-theory`. Land work by fast-forwarding `main`; no PRs. Do not
  run two agents on overlapping files; check `git status` and `git log --oneline -3` before starting
  and before committing.
- A change to `rules/*.toml` or `src/*.rs` needs, before committing: `cargo build --release`,
  `cargo test --release`, `cargo run --release -- score --check-readme` (paste the corrected table
  into `README.md` if it flags one), `cargo clippy --release --all-targets`, and the matching
  Reference page edit in the same commit.
- Report real numbers, including a change that makes something worse for something else
  (`cargo run --release -- score`).
- Redeploy after a change to the generated dictionary or plugin: `lyk deploy` (validates with
  keymux, keeps the previous files for `lyk deploy --rollback`).
