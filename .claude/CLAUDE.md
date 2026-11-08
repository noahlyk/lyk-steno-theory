# lyk-steno-theory

**Docs outrank code here.** The user has said this explicitly and repeatedly: *"DOCS ARE THE MOST
IMPORTANT THING theory is good but the most important thing is a theory you can implement, I want
100% exhaustive documentation of rules and things I can learn to actually be able to type my
theory."*

**The docs now live on the site, in one place.** The lessons (`site/lessons/*.json`) teach; the
**Reference** (`site/reference/*.json`, route `#/ref`) holds every rule, table and mechanism,
exhaustively and concisely (no history or justification prose). The old `docs/*.md` chapter files
are gone; `docs/` keeps only `README.md` (a pointer and the old-path map), `decisions.md` (the
maintainers' log of why) and `roadmap.md`.

Any change to `rules/*.toml` or `src/*.rs` that changes what a word strokes as, what a brief does,
or how the generator picks between candidates **must** land with the matching **Reference page
edit in the same commit**, not as a follow-up. Pages that go stale are exactly the failure this
project has had repeatedly: a rule changed in code before the docs caught up.

What checks what: `python3 tools/site_data.py` verifies every `[[word|OUTLINE]]` in a lesson or a
Reference page against the built dictionary (`out/lyk`) and rewrites `site/data/`; `node site/check.mjs`
and `node site/test.mjs` check structure, links and the engine; `cargo run --release -- score
--check-readme` catches numeric drift in `README.md`'s tables. **None of them catch prose drift**
(a rule described on a page that no longer matches the code): that has to be done by hand, every
time, as part of the same change. Commit the regenerated `site/data/` with the change. How to
write a page: [`site/README.md`](../site/README.md). The phrasing grammar (lessons 19 and 20) is
checked against `tools/phrasing_prototype.py`, not the dictionary, until the generator builds it.

Learn the theory from the site (lessons in order, then the Reference), not from the repository
files.

**No pull requests.** This repo takes no PRs, ever. Finished work lands by fast-forwarding `main`:
linear history, no merge commits, no force pushes on `main`. Do not open, suggest or leave a PR
open. GitHub cannot delete a PR, so a stray one is closed (`gh pr close`) and its branch deleted.
