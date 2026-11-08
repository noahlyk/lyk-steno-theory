# 09 — The solver plugin: read a missed run as the word it meant, without a table of every outline

**Status: built, on the experimental branch `experiment/wasm-fallback-resolver` (not on `main`).**
Measured (see `docs/decisions.md`, the last three entries, for the why of each piece): top-1 86.7%
on 15,458 backups (90.1% weighted by use, 94.8% top-5, 92.3% for the 2,000 commonest words), below
§6's 97%/93% bar; wasm answers identical to native, 2.3 ms mean / 6 ms p99, 67 MB wasm memory. `deploy` ships no backups at all (the solver
replaces them; `build` keeps them in `out/lyk` as the test set), so §6's thinning phase is moot. §1's
RAM figure was not measured: keymux memory-maps its dictionary caches, and the backups cache shrinks
88 -> 0 MB on disk; the solver adds ~62 MB resident. A one-key misstroke layer is built (single strokes only). §3c layer 3 (switching mid-piece) is not.

## 1. The problem, measured

`out/lyk/02-lyk-backups.json` today (built 2026-11-08):

| | |
|---|---|
| backup entries | **3,864,383** (131 MB of JSON) |
| distinct words they cover | 64,012 |
| entries per word | median **63**, p90 93, max 231 |
| by stroke count | 2: 519k · 3: 1.48M · 4: 1.15M · 5: 540k · 6: 177k · 7–8: 1.2k |
| everything else (words, short, alternates, extras, phrases) | 125k entries, 3.4 MB |

keymux holds a dictionary as `FxHashMap<Vec<u32>, String>` plus a reverse `by_text` map
(`keymux/src/steno/dict.rs:48-50`): roughly 100–150 bytes per entry both ways, so the backups
alone are on the order of **0.5–1 GB resident** once the engine is loaded (the `PHRO*PLZ` unload
stroke exists because of this). And it is still not exhaustive: the user's live tape
(`~/.local/state/keymux/steno-tape.txt`) has **4,015 misses** (`?` lines) in 116k strokes, every
one a *single* stroke, and most of them phonetically right strokes the dictionary simply does not
list: `TRAEUPB`, `TOLT` (then `TOELT` → total), `TRAU` (then `TRAOU` → true), `SOPB`, `KREU`,
`PWRAFPB`, `PO*EUT`. Enumerating 63 outlines per word is the wrong tool: the space of reasonable
strokes for a word is larger than any list, and the list is what costs the RAM.

The idea (the user's): stop enumerating. Ship the *rules* to keymux as a WASM plugin that, given a
run of strokes nothing in the dictionary matched, works out the word. Because every lyk stroke
after the first carries the join key, a run has no word-boundary ambiguity, and the engine
re-asks on every new stroke, so the plugin can revise its guess as the run grows (`KO/+-PL` →
"com", `+PU` → "compu…", `+TER` → "computer"). Then the dictionary can be thin: primaries, short
forms, briefs, phrases, suffixes, plus only the backups the solver gets wrong.

## 2. What keymux already does for us (read, not assumed)

- **The growing run is re-handed whole on every stroke.** `translate_stroke`
  (`keymux/src/steno/translate.rs:633`) builds the history window; for a stroke the plugin's
  `joins()` says is a continuation, `history_window` (`:1103`) does **not** stop at the
  dictionary's longest key — the run grows as long as the keyboard says so. `outlines()` lists
  windows longest first; `longest_match` asks `Chain::lookup` for each until one answers, and the
  winner **replaces** the earlier translations (`translation.replaced = window[i..]`). So the
  plugin is asked for the full run first, and its new answer retracts its previous one. No
  engine change needed for "keep guessing as strokes arrive".
- **The plugin is asked only for one word**: a single stroke, or a run where every stroke after
  the first answers `joins()` true (`Chain::is_one_word`, `plugin.rs`). Dictionary steps ahead of
  it in folder order are tried first. So the plugin never overrides a dictionary hit and never
  sees unrelated strokes.
- **The instance persists** (`Plugin { store, memory, .. }` is kept and reused), so a lexicon
  built or embedded in the plugin's linear memory is paid for once. There are **no host
  imports**: the plugin cannot read files; everything it knows must be in the `.rs`.
- **No fuel / timeout.** A slow `fallback` call blocks the stroke; an infinite loop hangs the
  daemon. Every loop in the solver must be bounded by construction (beam width, run length).
- **ABI_VERSION 1**: `alloc(len)`, `fallback(ptr,len) -> packed`, optional `joins(bits)`. The
  outline is rendered with the chain's own `Layout`, so the `+` bit (bit 23) survives
  (keymux `5a4460f`). The ABI boilerplate is in `git show 49a3660^:src/rules/plugin_template.rs.in`
  (bottom section) and `keymux/docs/plugins/README.md`.
- **Right now nothing exports `joins`.** `49a3660` removed the old plugin and the deployed folder
  has no `.rs`, so `Chain::joins` is false for every stroke: keymux currently treats `+` as an
  ordinary key — runs only work because every one is a literal entry, and the join-key semantics
  (`compose.rs` composition, unbounded run windows, "a `+` stroke is never a word alone") are
  off. The new plugin restores them as a side effect; note this in `decisions.md`.

## 3. The algorithm: lexicon-tree decoding over a sound lattice

This is the speech-recognition shape (token passing through a pronunciation trie), not a
"generate every outline" shape. Three stages, each a pure function.

### 3a. Stroke → sound alternatives (a small lattice per stroke)

`Rules::resolve_stroke` picks *one* reading per stroke. The solver must keep **all** readings,
because a chord legitimately writes several sounds:

- consonant chords: every forward entry of `onset`/`onset_cluster`/`coda`/`coda_cluster` that maps
  to those keys (`-F` is F and V, `-Z` is Z and S, `*T` is TH, …), and every way to **cover** the
  bank's key bits with catalog chords, not just the greedy lowest-key cover `decompose` takes
  (`PHR` is M-R and also P-H-L? — enumerate, bound to the few that exist);
- vowel chords: every sound that writes those keys — the heard vowel (`vowel`), the spelled
  variants (`AA.o`, `EY.a`, every `letter_vowel` × sound pair that `Rules::parse` adds), and the
  `vowel_coda` combos (`AR`, `OPBG`) with their coda consumed;
- no vowel keys: a bare consonant stroke, which is how the generator writes a reduced syllable
  (`-BL` for *-ble*, `-PB` for *-on*) and a dropped first syllable — so the lattice offers
  "an unstressed vowel was here" as a cheap insertion, not an error;
- a star nothing claims: a marker (as `decode_right` already treats it), cost 0, no sound;
- the join and spell bits stripped; a run whose first stroke carries `SPELL` goes to
  `resolve_spelling` unchanged (letter-exact mode stays as it is).

Output per stroke: a tiny DAG of sound sequences (typically 1–6 alternatives). The run's lattice is
the concatenation. Stroke boundaries carry **no** cost and **no** constraint: the backups show the
generator cuts anywhere (`S/+T/+A/+-RB/+U/+-PB` is one sound a stroke), so the solver must too.

Build the reverse tables by inverting the *forward* tables in `Rules::parse`
(`src/rules/mod.rs:431-470`), extended to keep every label per chord instead of one; the single
reading `resolve_stroke` returns stays for `resolve`'s plain read-back display.

### 3b. Decode: beam search with the pronunciation trie

State = (position in the run lattice, trie node, cost, back-pointer). Start at (0, root).
Transitions, each with a cost (units: the lattice's own, `STROKE = 8` in `src/rules/lattice.rs`):

| move | cost | why |
|---|---|---|
| consume a lattice sound that matches the trie edge | 0 | the ordinary case |
| substitute within a confusion class (vowel↔vowel by a small table: `IH~IY~EH`, `AH~UH~AO`, `EY~EH`…; `F~V`, `S~Z`, `T~D`, `K~G`, `TH~DH`, `N~NG`) | 2 (vowel) / 5 (consonant) | spelled vowels and voicing are the generator's own slack |
| trie edge is an unstressed vowel the run lacks | 1 | reduced syllables are struck without their vowel |
| trie edge is a secondary-stressed vowel the run lacks | 3 | |
| trie edge is a primary-stressed vowel the run lacks | 8 | nearly never right |
| trie edge is a consonant the run lacks | 4 | folding drops a consonant sometimes (`building`'s `-LGD`) |
| run has a sound the trie does not | 6 | |
| trie node is a word end → emit word, return to root | `PIECE` (3) + rank cost | fewer pieces beat fragments |
| rank cost | `round(2·log2(rank+2))` | *the* is free, rank 50k costs ~31 |

Plus two **exact-token** shortcuts evaluated at stroke boundaries, before the lattice:

- **suffix chords** (`rules/suffixes.toml`, single and two-stroke, join bit set, never the first
  stroke): emit the suffix text at cost 1, unconditionally — this is the "suffix VIP" rule
  `decisions.md` says must stay unconditional;
- **stem table**: if the run's first stroke(s) exactly equal a word's *primary* outline
  (`generate::single_stroke_stems_of`, extended to multi-stroke primaries), emit that word at
  cost 0 (`SKP/+-R` → "and"+"er" → "ander", `PW-LG/+-S` → "buildings"). This is the old plugin's
  `STEMS` table, which `resolve` still honours (`site/reference/fallback.json`).

Beam: after each lattice position keep the best `K` states (K = 256 to start; tune on the eval),
deduplicated by trie node. Run length is bounded by the window (≤ 8 strokes, ≤ ~30 sounds), so
work is ≤ 30 × 256 × branching, well under the budget. Target: **< 200 µs per call** in wasm,
hard ceiling 1 ms. No recursion, no unbounded loop.

Result: the best path's words. If its cost exceeds a threshold (start at 24 + 6 per stroke), answer
with the **current plain read-back** (`Rules::resolve`'s spelling) so the behaviour floor is
exactly today's, never worse. Below threshold: join the pieces with the spelling rules the
generator already uses for compositions (`drop_silent_e` in `resolve.rs`; the family link rules in
`src/family.rs:104` for *make→making*, *run→running*, *y→i*) — reuse, do not reinvent.

### 3c. Spelled strokes: the orthographic third of the dictionary

`49a3660`'s own measurement: 1.31M of the 3.86M backups come from `orthography.rs`, which reads a
word's **spelling as a pseudo-pronunciation** (each letter a sound; a vowel letter offered
short, long and weak; `c`/`g` both ways) and runs the same lattice over it. The solver covers this
in three layers:

1. **A vowel struck by its letter inside a phonetic stroke** (`O` for *control*): already cost 0,
   because 3a inverts the `letter_vowel` variants `Rules::parse` adds to the forward table.
2. **A piece spelled as it looks** (`con/+tem/+pla/+tion`): every word gets a second path in the
   trie, its pseudo-pronunciation, built by the *same* `orthography.rs` reading so the two can
   never disagree. Edges on that path are letter classes, not phonemes — `a` matches `AE`/`EY`/
   `AH0` at cost 0, `c` matches K or S — a richer edge symbol, nothing more. Trie roughly doubles
   (~300k nodes, ~4 MB). Because pieces are independent segments of the run, a phonetic stem plus
   a spelled remainder (what `spelled_remainder` / `REST_QUOTAS` generate today) decodes with no
   extra machinery. **In v1.**
3. **Switching mid-piece** (*con* by sound, *tem* by letters, inside one word): nothing in the
   current dictionary does this, so the eval cannot test it. It needs each word's letters aligned
   to its phonemes at pack time (grapheme–phoneme DP) so the two paths share positions and the
   beam can hop between them at cost 0; maybe +50% trie. **Deferred** until the tape shows the
   user actually strikes this way — the one open question typing has to answer.

### 3d. Why this beats the list

The list answers "is this exact outline one of the 63 I wrote down". The solver answers "which
word's sounds are closest to the sounds these keys write", which covers every outline the list
has **and** the ones it does not (`TRAEUPB`, `TOLT`, `TRAU` above), and frequency breaks ties the
way the generator's own cost function does (`candidates.rs`: strokes, then keys, then frequency as
the arbiter).

## 4. Data layout and memory

Lexicon = the ranked words that have a pronunciation, top 100k ranks (the same bound every CLI
default uses): **57,948 words, 63,739 pronunciations, 401,923 phonemes, 138,796 trie nodes**
(measured from `data/frequency/en_full.txt` ∩ `data/pronunciation/cmudict.dict`; MFA/WikiPron
fallbacks per `Setup::load` add a few thousand more).

Packed flat, no pointers, little-endian, built by lyk and emitted into the plugin source:

| table | layout | size |
|---|---|---|
| trie nodes | sorted so each node's children are contiguous: `sym: u8, stress: u8, child_count: u8, flags: u8, first_child: u32, word: u32` (12 B) | 139k × 12 ≈ **1.7 MB** |
| words | one `u32` offset table + concatenated UTF-8 | 58k × ~10 B ≈ **0.6 MB** |
| ranks | `u32` per word (or implicit: words sorted by rank) | 0.2 MB |
| stems | sorted `(u32 stroke, u32 word)` for single-stroke primaries; multi-stroke primaries as a second small trie keyed by stroke bits | 47k × 8 ≈ **0.4 MB** |
| reverse chord tables, suffixes, spelling table, confusion classes | as the old template did, as `const` arrays | < 50 KB |

**≈ 3 MB total**, embedded as byte-string literals in the generated `.rs` (the old template
embedded its tables the same way; rustc handles a few MB of literal fine — verify the compile
time, `opt-level=2`, stays under ~30 s, since keymux compiles on first load). Peak wasm memory:
3 MB data + beam scratch (256 states × ~16 B × 30 positions ≈ 120 KB). Against today's
0.5–1 GB this is noise, and it lets the backups file shrink to whatever the eval says it must keep.

Do **not** use `include_bytes!` of a sibling `.bin`: keymux caches the compiled plugin by the
`.rs` content hash only, so a changed `.bin` would not recompile. One file is the contract.

## 5. Where the code lives (one source of truth, both sides)

- `src/rules/solve/` — the solver core, written to compile both natively and as the plugin:
  `core.rs` (no_std-friendly: no `HashMap`, no alloc beyond `Vec`; reads the packed tables through
  a `&[u8]`), `pack.rs` (lyk-side: builds the packed tables from `Setup` + the built dictionary),
  `mod.rs` (lyk-side `Rules::solve_run`, wrapping `core` for `resolve`, `lookup`, the site data,
  and the eval).
- `src/rules/plugin.rs` + `plugin_template.rs.in` — resurrected from `49a3660^` and changed: the
  template is `include_str!("solve/core.rs")` + the emitted tables + the fixed ABI section
  (`alloc`/`fallback`/`joins`). The plugin's `fallback` caches its last answer by outline string
  (the engine asks the same run repeatedly within one stroke's search).
- `build`/`deploy` write `05-lyk-solver.rs` beside the dictionaries (`output.rs` currently
  *deletes* `lyk-orthographic-fallback.rs`; keep deleting that old name, add the new one). The
  `.built` hash must cover the plugin too.
- New hidden CLI command `solve <outline>` (prints the ranked readings with costs and pieces) and
  `solve --eval <step>` (§6). `resolve`/`lookup`'s "Reading" row (`answer.rs` `How::Reading`) uses
  the solver's best answer, with the plain read-back as its floor.

## 6. The eval harness decides everything — build it first

`solve --eval N` over every Nth backup entry (the 3.86M pairs are a labelled test set: outline →
word the generator meant):

1. **recovery**: top-1 and top-5 rate, broken down by stroke count and by rank band
   (<2k, <10k, <30k, rest);
2. **latency**: mean, p99, max per call, native; then the same inside wasmtime through keymux's
   own plugin loader (a keymux test or a tiny harness binary — the wasm number is the one that
   matters);
3. **the tape**: every `?` miss in the steno tape, with the stroke the user struck *next*
   (usually the correction) as a weak label — print the solver's answer beside it and eyeball
   200 of them;
4. **disagreement with the dictionary**: for every backup the solver gets *wrong*, which word it
   said instead and that word's rank — these are the entries the thinned dictionary must keep.

Acceptance to go to phase 2 (thinning): top-1 ≥ 97% on 2–3-stroke backups, ≥ 93% on 4+, wasm
p99 < 1 ms. If the first cut is far from this, the costs in §3b are the first thing to tune, the
confusion classes second, the beam width third; report the numbers, not a feeling.

Phase 2: `build --thin` (or a `[backups] mode` in rules) writes only the backups the solver gets
wrong plus every entry of other roles; measure keymux RSS with the engine loaded before and
after (strike a word, read `/proc/<pid>/status` VmRSS); re-run `score --check-readme`.

## 7. Docs that must change in the same commit (CLAUDE.md rule)

- `site/reference/fallback.json` ("The stroke read-back"): becomes the solver's page — what the
  plugin does, the cost table in §3b in plain words, the floor behaviour, the suffix and stem
  rules (unchanged), the threshold, what `resolve` prints. The sentence "No engine runs this
  reading … lyk ships no plugin" is false the moment this lands.
- `site/reference/build-and-files.json`, `cheatsheet.json`, `join-key.json`, `spelling-mode.json`,
  `typable.json`, `ways-to-join.json`: the places `49a3660` edited to say there is no plugin.
- `docs/decisions.md`: why the orthographic plugin was removed and a *different* plugin is back
  (the old one re-spelled sounds; this one finds the word), the memory numbers from §1, and the
  `joins` note from §2.
- `README.md` build/deploy section; `python3 tools/site_data.py` and `node site/check.mjs`
  rerun, `site/data/` committed.

## 8. Risks and how each is bounded

- **Wrong word with high confidence** (the solver "fixes" a stroke the user meant literally).
  Only reached on a dictionary miss, so it can only replace today's raw-keys output; the
  threshold floor keeps the plain read-back for anything far from every word. The eval's §6.4
  list is the watch list.
- **Latency in wasm**: beam and run length are fixed bounds; measure in wasmtime, not natively.
- **Plugin compile time** on first load with a 3 MB literal: measure; if over ~30 s, split the
  literal into several `const`s (rustc is quadratic in some literal paths) before anything else.
- **Thinning changes behaviour**: phase 2 is gated on the eval and on a before/after of every
  `[[word|OUTLINE]]` in the lessons and Reference (`tools/site_data.py` already verifies these
  against `out/lyk`).
- **Branching**: this branch (`experiment/wasm-fallback-resolver-v2`, cut from `49a3660`) is
  intentionally off the linear line; it fast-forwards onto `main` only when §6 passes, as one
  reviewed sequence of commits, no merge commit, no PR.
