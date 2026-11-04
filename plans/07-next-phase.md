# 07: next phase (written 2026-11-02, mid-session, so it survives a context compact)

Self-contained. Read this top to bottom before touching anything. **Working agreement:** noahlyk is
the *learner*; Claude designs the theory, makes the calls, ships them on a branch, measures, and
explains. He gives feedback afterwards. He types on his own deployed build constantly. Don't hand him
option menus; decide.

## Where things stand

- **Repo / branch:** `/home/fib/code/noahlyk/lyk-steno-theory`, worktree
  `.claude/worktrees/improve-solver-and-holes`, branch `worktree-improve-solver-and-holes`, pushed to
  origin. Based on `worktree-handoff-plans` (`84d6b32`); **`origin/main` is stale, don't base on it.**
- **keymux** is a separate repo at `/home/fib/code/noahlyk/keymux`. Work there on its own branch,
  commit there, never push its main. Live config `~/.config/keymux/steno/lyk/` is **production**:
  never write to it without asking (`deploy` writes it; he runs that himself).
- **Landed this session (all on the branch, documented in `docs/decisions.md`):** exact-solver
  early exit; no-match cost = fingerspelling; stroke weight 25 keys; pronoun "I" capitalized, letter
  "i" and contraction tails dropped; `eye`/`oh`/`working` pins; fingerspelling alphabet + Lapwing
  commands reserved; word families (`src/family.rs`, `rules/families.toml`: endings, prefixes,
  irregular past/plural, `not_family` list, family rules incl. star + K+ing swap, roots locked after
  pass 1, `FAMILY_DISCOUNT` 2 keys); `+-PBTS` possessive; `Origin::Extra` (up to 3 extra one-stroke
  entries per word in free strokes, never the reported outline); parallel candidate enumeration;
  `score --write-readme`; `family` command; tape-frequency blend tried and dropped.
- **Numbers vs the previously deployed dictionary:** about -0.075 weighted keys/word, -0.004
  strokes/word. 93 tests, clippy 114 warnings (do not regress), `build` ~13 s.
- Pipeline artifact: https://claude.ai/artifact/5TWgM37ypvJLL5srNu9WDx (at version 6; needs an
  update after the next big changes, avoid black text on dark backgrounds).

## Hard rules (from `.claude/CLAUDE.md` and his feedback)

- Any change to what a word strokes as or how candidates are picked lands **with its `docs/*.md`
  update in the same commit** (and a `docs/decisions.md` entry with measurements).
- Before every commit: `cargo test --release`, `cargo clippy --release --all-targets` (count must stay
  <= 114), `cargo run --release -- score --check-readme` (or `--write-readme`).
- Verify any subagent's claim yourself; hash-check (`sha256sum`) before/after dictionary builds.
- The worktree guard rejects compound shell lines mentioning git or runtime-expanded vars: keep
  commands plain and separate. Scratch lives in `$CLAUDE_JOB_DIR/tmp` (`/home/fib/.claude/jobs/71b4172d/tmp`);
  its analysis scripts (`best.py`, `diff.py`, `strokeloss.py`, `holes.py`, `collateral.py`, ...) are
  ephemeral. **Move the useful ones into `tools/` in the repo (workstream 6).**
- `data/` in the worktree is a symlink to the main checkout's data; `.gitignore` covers it.

## Workstreams, in order of how I'd do them

### 1. keymux: lookup strokes must not pollute the tape or the undo stack (his live bug, do first)

**STATUS: DONE (2026-11-02), not yet installed.** Root cause found: not `*` itself but a longer
entry. When a later stroke combines with the lookup stroke into a multi-stroke entry (`PHRUP/x`,
common in lyk's `+` backups), it *replaces* the lookup's translation; undoing it puts the replaced
translation back and the formatter re-sent its `Command`. Fix (keymux branch `steno-command-strokes`,
worktree `/home/fib/code/noahlyk/keymux/.claude/worktrees/command-strokes`, commit `187444d`, based on
`fix-key-chatter-debounce`): `Translation.command_ran` set the first time the formatter sends a
command, command dropped afterwards (`formatter.rs`); a stroke whose output is only commands is kept
off the tape and the overlay tape (`engine.rs::is_command_only`, `OverlayHub::clear_keys`); two
existing tests updated that asserted the old tape behavior; README documents it. Undo after a command
still skips it and takes back the word before (Plover's `has_undo` rule). 457 keymux tests pass.
**To use it he must rebuild/install keymux from that branch** (I did not touch his install or config).
Remaining for this item: nothing, unless he reports it persists; then check for a different trigger.

**Symptom:** striking a lookup stroke adds an entry to the paper tape; pressing `*` to undo it
re-runs the lookup tool (re-opens the overlay). Annoying while typing.

**Wanted behavior (match Plover):** a command/meta translation (lookup, suspend, add-translation,
etc.) is **do-only**: it executes on the "new" side of an output diff and is *never* re-executed when
that translation is undone or retranslated. Undo of such a stroke must do nothing. Also give the tape
a way to omit (or visually separate) command strokes, and make undo skip over them cleanly so one `*`
does not get "wasted" on a stroke that produced no text.

**How to do it:**
1. Read `keymux/src/steno/` : `engine.rs`, `translate.rs`, `action.rs`, `commands.rs`, `tape.rs`,
   `journal.rs`, `lookup.rs`, `overlay.rs`, `live.rs`. Find where `{PLOVER:LOOKUP}` / the lookup
   command is executed and how undo and the output diff (old actions vs new actions) re-fire it.
2. Check Plover's own source for the exact rule (formatter `_translations_to_actions` / `_do_actions`:
   commands run only for newly added translations, never for `old` ones on undo). Mirror it.
3. Design a proper **action kind**: `Output` (text, undoable, in the tape) vs `Command` (side effect,
   run once on do, ignored on undo/retranslate, flagged in the tape). Apply it to *all* command
   syntax, not only lookup.
4. Tape: command strokes get a distinct marker or are omitted (config flag, default omit), and the
   undo stack skips them.
5. Tests in keymux for: command runs once, `*` after a command does not re-run it, `*` skips the
   command and undoes the previous text, tape omits it.
6. Commit on a keymux feature branch. Do not deploy; tell him the command to rebuild keymux.

### 2. Multi-file dictionary output + per-file caching and hot reload in keymux

**STATUS: DONE (2026-11-03), keymux part not installed.** lyk: `src/output.rs` (`Role`,
`write_dictionary`), seven `02-lyk-*.json` files, `deploy` copies changed files only and removes stale
ones, `docs/files.md`. keymux branch `steno-per-file-cache` (commits `619a219`, `30e2f56`, on top of
`steno-command-strokes`): `src/steno/parts.rs` per-file parse + stat shortcut + racy-mtime guard +
per-file cache under `cache/parts/`, `Dictionary` = layered `Arc<Table>`s, `live.rs` reloads only what
changed. 467 keymux tests pass. Remaining: nothing, unless he reports a problem. Original notes:

**His decision:** have `build`/`deploy` output **several smaller, organized JSON files** instead of one
4 MB `02-lyk-words.json`. Then change keymux so caching and hot reload are **per file**: only
re-read and re-parse files whose contents changed (hash or mtime+size), keep parsed dictionaries
cached, and merge incrementally.

**lyk side:** in `generate::write_json`/`deploy`, split by role, e.g. `02-lyk-primary.json` (one
primary per word), `03-...-extras.json` (`Origin::Extra`), `...-backups.json` (`+`-joined backups,
maybe split by family/prefix/suffix), `...-phrases.json`, `...-suffixes.json`. Keep numeric
prefixes so keymux's priority order stays correct (primary beats extras beats backups). Make the
split deterministic so unchanged files stay byte-identical across builds (this is what makes per-file
reload useful: a small theory change should rewrite only a few files). Verify the union of all files
equals today's single file. Update `STATIC_FILES`/`deploy` in `src/main.rs` and `docs/`.

**keymux side:** find the dictionary loader and reload path (`dict.rs`, `source.rs`, `setup.rs`,
`live.rs`, the hot-reload watcher). Add per-file content hashing and a parsed cache; on a change
event reparse only changed files and rebuild the merged lookup incrementally (layered maps in priority
order). Add timing logs/tests. Also lets the metadata problem go away: file = role, so a "fastest
outline" lookup can prefer the primary file.

### 3. More frequency analysis (written-English frequencies)

**Done 2026-11-04.** Names count a tenth (`src/frequency.rs`); the written blend was measured and
rejected, and the homophone cases it was meant for went to item 4 (`docs/decisions.md`).

Subtitle counts distort which word gets a stroke (`doc` > `dock`, `matt` holds `PHAT`, `aye` took
`eye`). The tape blend was tried and dropped (too small, noisy; see decisions.md). Do:
1. Build a written-English frequency signal from sources already in the repo (5 Gutenberg texts in
   `data/corpus/`, the Google Books bigram file `data/frequency/bigrams_en.txt` whose unigram
   marginals are usable) and consider fetching a larger written unigram list via `fetch.rs` (check
   with him only if it needs a new download host; the repo already fetches several datasets).
2. Blend with the subtitle counts (geometric mean or rank-average, not a sum) so conversational
   words keep weight but written/proper-noun artifacts shrink. Cap the effect of name-like tokens.
3. Measure with the usual diff (weighted keys/strokes per word, which words changed, which top-1000
   words lost/gained a stroke). Re-check the words that caused trouble: doc/dock, matt/mat, aye/eye,
   oh/owe, i/I, mat/sock/tack/dock (stuck on `/+*`).
4. Add a `frequency` report command (top disagreements between sources) and tests. Document in
   `docs/` (new section) and `docs/decisions.md`.

### 4. Better design for multi-stroke / `+` outlines

**Star part done 2026-11-04**: a one-syllable stroke's star goes to its sound-alike before a short
form (`STAR_SURCHARGE`); the `+*` marker stays as the last resort for ~1,230 words, almost all
names (`docs/collisions.md`).

Today `+`-joined entries come from several ad-hoc backup directions plus family offers; lots of
cases are arbitrary (`X/+*`, `X/+*/+*` ladder markers, compounds that split mid-syllable). Redesign
around predictability: **a `+` entry should always decompose into pieces a learner already knows**
(a placed whole word, a fixed prefix, a fixed suffix stroke). Concretely:
- Inventory every `+` entry by origin and by shape (`family`, `suffix`, `prefix`, `compound`, `ladder`
  marker, `fold`) and measure how many words rely on each; find words whose only entry is an arbitrary
  ladder or odd split.
- Replace `/+*` / `/+*/+*` ladder markers for short words (mat, dock, sock, tack, awe, hue) with a
  better disambiguator (one-stroke vowel-variant, or a stem + fixed marker with a defined meaning).
- Unify backup directions behind one table-driven decomposition (stem/prefix/suffix/compound) with a
  clear priority, and make `family`/`explain` show the decomposition for any `+` entry.
- Reduce the star's overloaded meanings (collision marker, family "key already there", spelling
  variant, fingerspelling): pick one meaning per context and document it.
- Document the result in `docs/join-key.md` and `docs/syllables.md`; keep every backup reachable.

### 5. Phrasing grammar (the real route to Magnum speed)

**Designed and measured 2026-11-04: [`08-phrasing-grammar.md`](08-phrasing-grammar.md)**, with a
prototype to type (`tools/phrasing_prototype.py`). Built once it survives typing.

Rule-based, learnable phrases: starter (I/you/he/we/they) + auxiliary (can/will/don't/did/would) +
common verb in one stroke, built from disjoint key regions instead of per-bigram luck. First write the
design (extend `plans/06-phrase-briefs.md`), with measured stroke savings on the Gutenberg and quote
corpora (`cargo test --release quote_corpus -- --nocapture`), then build. Get it to him as a *design he
reviews by typing it*, not a menu.

### 6. Housekeeping: code organization and cleanup

- Split `generate()` (about 700 lines, 12 phases) into one function per phase; remove the 8x
  copy-pasted "taken.insert + entries.push" block; fix `Entry::lost_to` (never set) or delete it.
- Reorganize `src/` into modules (`src/generate/{candidates,solver,backup,family,phrases,output}.rs`,
  `src/rules/`, `src/analysis/`, ...) instead of 3000-line files; keep public API stable.
- Remove dead code (`phrase_candidate` is test-only), unify the `briefs` loading boilerplate in
  `main.rs`, cache the per-word phoneme labelling in `rules.rs` (it recomputes 5-10x per word), swap
  `HashMap<String,..>` SipHash keys for faster/typed keys where it is cheap.
- Move reusable analysis scripts from the job temp dir into `tools/` with a short README, and add a
  `family`-style CLI report for the "best outline changed" diff so the scripts are not needed.
- Keep `docs/` in step; update `docs/README.md` map if files move; refresh `plans/README.md`.

### 7. Smaller items

**Done 2026-11-04**: hyphen and caps strokes documented, `lost_to` replaced by a computed report,
analysis scripts in `tools/`.

- Hyphen strokes (`H-PB` etc.) are undocumented in `docs/punctuation.md`; add them.
- Lapwing parity: repeated-arrow chords and engine-only commands were skipped; revisit if wanted.
- He asked which "KWBG" period stroke he meant; `KWBG` does not exist anywhere in the reserved files.
  Re-ask only if it comes up again.
- Update the pipeline artifact after workstreams 2-4.
- Open question for him, only if it blocks: none right now. Decide and ship.

## Suggested order

1 (keymux bug) -> 2 (multi-file + keymux per-file reload; also unblocks "primary vs extra" metadata)
-> 6 (housekeeping, before the next large feature so the code is easier to change) -> 3 (frequency)
-> 4 (`+` redesign) -> 5 (phrasing design, then build) -> 7 / artifact. Re-measure and re-document at
each step, and tell him what to deploy and what to look for when he types.
