# Punctuation & commands

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

**[Reserved strokes](reserved-strokes.md)** explained *why* these are pinned ahead of every word.
This chapter is the actual content: what's in `rules/reserved/punctuation.json` and
`rules/reserved/commands.json` (copies of the deployed `03-lyk-punctuation.json` and
`01-lyk-commands.json`), grouped by what they do, not the raw JSON.

## Punctuation (`rules/reserved/punctuation.json`)

196 strokes: 144 forked from Plover's own punctuation dictionary, plus the 52 glued
fingerspelling letters (`A*` a, `A*P` A, ...) documented in
**[fingerspelling & misc keys](fingerspelling.md#fingerspelling-in-running-text)**. Most marks have far more than one
stroke because Plover's convention gives each mark several chords for several different contexts —
struck plain, struck doubled, struck with a leading or trailing glue so it doesn't force an
unwanted space, struck after a quote or before a parenthesis, and so on. A handful of the busiest
marks, by stroke count:

| Mark | Strokes | A few of them |
|---|---|---|
| `.` | 30 | `-FP`, `TP-P`, `P-FP`, `TPHR-FRPBLT` |
| `,` | 14 | `-RB`, `KW-BG`, `W-BG` |
| `?` | 11 | `H-F`, `STP`, `STPH*` |
| (word joiner, no space) | 5 | `SP`, `S-P`, `S*PS` |
| `!` | 5 | `TP-BG`, `SKHAPL`, `STKPWHR-FPLT` |
| `;` | 4 | `SP-PT`, `SKWR*RBGS` |
| `:` | 3 | `STP-FPLT`, `STPH-FPLT` |

Plover's own period dictionary hands it enough strokes that two of the bare, vowel-less ones —
`-P` and `-PL` — were never missed when freed for briefs instead (see `rules/briefs.toml`'s `up`
and `am`): a mark with 30 other ways to strike it doesn't need a short, phonetically obvious
right-hand chord more than a word used in nearly every sentence does. `up`'s own brief had quietly
lost `-P` to this same reservation for a long time with nothing printed to say so, silently falling
back to the two-key `UP` the rules generate anyway — `generate::generate` now refuses to build at
all if a `[word]`-named brief's own literal outline is already reserved, rather than letting that
happen again unnoticed.

Beyond the sentence-ending marks, the same file carries:

- **Quotes and apostrophes** — opening/closing double quotes (`KR*GS`/`KR-GS`), a possessive or
  contraction apostrophe (`-PBT`, `SKW*T`/`SKW-T`).
- **Parens and brackets** — `P*PB` opens `(`, `P*PBT` closes `)`.
- **Dashes and ellipses** — `TK-LS` for a plain dash, `HR-PS`/`SW-BS` for `...`.
- **Hyphens** — `H-PB` joins the words either side of it with a hyphen and no spaces: *well*
  `H-PB` *known* types `well-known` (`H-B`, `H*FPB` and `HA*EPB` do the same). `H-PBS` types a
  hyphen glued to the next word, with the space before it kept (*ls* `H-PBS` *a* types `ls -a`;
  `HAO*EPB` does the same), and `H-PBZ` two hyphens (`--verbose`): the command-line flag forms.
  `PWHR-T` glues a hyphen to the word before and capitalizes the next.
- **Symbols that are their own stroke, not built from other punctuation** — `@` (`KWRAT`), `%`
  (`PHR*US`), `=` (`KWA*LS`), `™` (`TR*PL`), `°` (`TKPWRAO*E`), a cent sign (`KAO*UBD`), a currency
  sign (`PHR*SZ`), and a few more.
- **Capitalizing the next word** — `KPA` fires `{-|}`, Plover's own "capitalize the next word"
  directive, and keeps the space; `KPA*` does the same glued to the word before, with no space
  (`{^}{-|}`). To capitalize a word already written, or write a word in capitals, see the
  [commands](#commands-rulesreservedcommandsjson) below. This is how proper nouns are
  handled under this theory (see **[the roadmap](roadmap.md)**'s conflict-taxonomy note): lyk
  itself never generates a dedicated outline shape for a name — a name types as whatever its
  ordinary phonetic outline would be, capitalized by firing this stroke first.

## Commands (`rules/reserved/commands.json`)

66 strokes, smaller and more purpose-built than the punctuation file. Grouped by what they do:

| Group | Strokes | What they send |
|---|---|---|
| Cursor movement | `STPH-R`/`-B`/`-P`/`-G` | Left / Down / Up / Right |
| Word-at-a-time movement | `STPH-RB`/`-BG` | Ctrl+Left / Ctrl+Right |
| Line and page movement | `STPH-FR`/`-LG`/`-RPG`/`-FBL` | Home / End / Page Up / Page Down |
| Selecting (any movement above, plus `*`) | `STPH*R`/`*B`/`*P`/`*G`, `STPH*RB`/`*BG`, `STPH*FR`/`*LG`/`*RPG`/`*FBL` | the same key with Shift (Ctrl+Shift for the word-at-a-time pair) |
| Select to start/end of line | `SH-FT`/`SR-RS` | Ctrl+Shift+Home / Ctrl+Shift+End |
| Alt-tabbing between apps | `TPW-R`/`TPW-G`, `TA*BT` | Alt+Left / Alt+Right / Alt+Tab Tab |
| Editing | `TK*EL`, `PW*FP`/`PW-FP`, `TA*B` | Delete, Backspace, Tab |
| Newline / paragraph break | `SKW-BGS`, `R*R`, `SKWRAEURBGS`, `R-R` | Return, or a blank-line paragraph break with the next word capitalized; `R-R` is a line break in the text itself (`{^~\|\n^}`), not the Return key |
| Escape | `TPEFBG`, `EFBG` | Escape |
| Function keys | `TPWUPB` F1, `TPWO` F2, `TPWAOE` F3, `TPWOUR` F4, `TPWAOEUF` F5, `TPWEUBGS` F6, `TPWEFPB` F7, `TPWAEUT` F8, `TPWAOEUPB` F9, `TPWEPB` F10, `TPHREFPB` F11, `TPWEFL` F12 | F1–F12 (Lapwing's own strokes: `TPW` "f", then the number's own sound) |
| App shortcuts via Super | `KPH*F`/`KPH*BG`/`KPH*T`/`KPH-BG` | Super+V / Super+K / Super+W / Super+C |
| App shortcuts via Ctrl | `KHR*F`/`KHR*BG`/`KHR*T`/`KHR-BG` | Ctrl+V / Ctrl+K / Ctrl+W / Ctrl+C |
| The next word in capitals | `KPA*L` | `{<}`: *`KPA*L` pay* types `PAY` |
| Capitalize the word just written | `KPAD`, `KA*PD` | `{*-\|}`: *pay `KPAD`* types `Pay` |
| The word just written in capitals | `*UPD` | `{*<}`: *pay `*UPD`* types `PAY` |
| Plover engine control | `PHRO*F`, `PHRO*PB`, `PHROLG` | Suspend / Resume / Toggle Plover |
| Add a translation | `TKUPT` | Plover's "add translation" dialog |
| The literal/raw stroke | `^`, `SP-LS` | `{*!}` — Plover's "don't autocorrect this one" |

Plover's own engine-control strokes (suspend/resume/toggle, add-translation) are here because
they're reserved the same way any other command is: a word can never silently take over the
stroke a steno engine itself depends on to keep working.

The `STPH` movement family and the F-keys are Lapwing's own strokes (`data/dictionaries/lapwing-commands.json`),
minus its number-key (`#`) variants, which lyk's stroke model has no key for. Lapwing's repeated-arrow chords
(`STPH-RS` for Left twice, and so on) and its Plover-engine-only commands (`{PLOVER:CONFIGURE}`, `{MODE:CAPS}`)
were left out, so there is no caps-lock: for a run of words in capitals, strike `KPA*L` before each.

Next: **[fingerspelling & misc keys](fingerspelling.md)**.
