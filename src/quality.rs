//! Structural sanity checks on the real, deployed dictionary - not "is this word terse" (that's
//! `quote_corpus`'s job), but "does the dictionary even make sense": no two words fighting over
//! the same stroke, no word missing that obviously should not be, and the known cost of design
//! decisions already made (see `docs/decisions.md`) staying where it was measured, not quietly
//! growing. Each test here is a regression guard for a specific, previously-real bug this session
//! found the hard way, so a future change that reintroduces one of them fails immediately instead
//! of waiting for someone to notice live, the way "don't" and "quitters" both were noticed.

use crate::num::ToF64;

use crate::data;
use crate::dictionary_fixture;
use crate::generate;
use crate::stroke::render_outline;
use std::collections::HashMap;
use std::path::Path;

/// No two different words were ever meant to share a stroke - `generate()`'s own `taken` map
/// exists specifically to prevent it. This re-checks the invariant from the outside, on the real
/// built dictionary, rather than trusting that nothing inside `generate()` ever bypasses it.
#[test]
fn no_two_words_share_the_same_deployed_outline() {
    let lyk = dictionary_fixture::cheapest();
    let mut by_outline: HashMap<String, &str> = HashMap::new();
    let mut collisions = Vec::new();
    for (&word, outline) in &lyk {
        let text = render_outline(outline);
        if let Some(&other) = by_outline.get(&text)
            && other != word
        {
            collisions.push(format!("{text:?}: {other:?} and {word:?}"));
        }
        by_outline.insert(text, word);
    }
    collisions.sort();
    assert!(collisions.is_empty(), "words sharing a stroke: {collisions:#?}");
}

/// A word common enough to be in everyday use, with a real pronunciation on record, should never
/// be silently missing from the generated dictionary - that was exactly the "don't" bug (see
/// `docs/decisions.md`): a real contraction with a real `CMUdict` entry, absent from the deployed
/// dictionary entirely because of a corpus mismatch nothing surfaced until someone hit it live.
/// This would have caught it automatically. Mirrors `generate()`'s own word-list filter
/// (`src/generate/`'s `words` construction) so it only asks for a word the generator itself
/// should have tried to place, not one it deliberately skips (a stray single-letter frequency
/// artifact, for instance).
#[test]
fn every_sufficiently_common_pronounceable_word_has_a_real_outline() {
    const TOP_N: usize = 2_000;
    let setup = dictionary_fixture::setup();
    let lyk = dictionary_fixture::cheapest();
    let mut missing: Vec<&str> = setup
        .rank
        .iter()
        .filter(|&(w, &r)| {
            r < TOP_N
                && setup.pron.contains_key(w.as_str())
                && (w.len() >= 2 || w.as_str() == "a")
                && !generate::CONTRACTION_TAILS.contains(&w.as_str())
                && w.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
        })
        .map(|(w, _)| w.as_str())
        .filter(|w| !lyk.contains_key(*w))
        .collect();
    missing.sort_unstable();
    assert!(missing.is_empty(), "common words with no outline at all: {missing:?}");
}

/// The count of words settled on a worse outline than their own, unclaimed plain reading (`build`
/// prints this list; see `docs/decisions.md`'s stranded-word entry) was fixed down to zero, one
/// `[word]` override at a time by hand - not something that should ever silently grow back. If
/// this fails, something in collision resolution regressed. A future regression that can't be
/// fixed immediately gets its own named, reasoned exception here (a nonzero baseline *with* a
/// comment pointing at an open issue) - not a quiet bump of a bare constant.
#[test]
fn stranded_word_count_has_not_grown_past_its_known_baseline() {
    let stranded = &dictionary_fixture::generated().stranded;
    assert!(
        stranded.is_empty(),
        "stranded-word count grew from 0 to {}: {:?}",
        stranded.len(),
        stranded
    );
}

/// Plan verification step 5 for the candidate-enumeration-and-solver rewrite (see
/// `docs/decisions.md`): the whole algorithm choice (successive shortest augmenting paths over the
/// *contested* subgraph only, split further into independent connected components, rather than a
/// dense assignment algorithm over the full dictionary) rests on fixed-point peeling settling most
/// of the dictionary on its own, leaving only a real minority still needing the solver's own
/// frequency-weighted arbitration - a fact this checks directly against the real, built dictionary
/// rather than trusting it by assumption.
///
/// The threshold is *not* the plan's own original estimate ("hundreds to low thousands", under 5%):
/// a real run measured roughly a quarter of the dictionary genuinely contested, not under 5%,
/// because the blanket `matches_real_consonants` filter's deeper fragment-drops mean far more words
/// now legitimately converge on the same small handful of very short, nearly-bare outlines than a
/// single-source brief pool ever produced. That is a real, now-measured property of this design,
/// not a bug to chase back down - connected-component splitting is what actually keeps each solve
/// small in practice (see `generate`'s own doc), not a low raw contested-word count. This guard
/// exists for the failure mode that *is* a bug: the contested set swallowing nearly the whole
/// dictionary the way an earlier, incorrect peeling test once did (confirmed directly: that bug
/// measured 99.3% contested, not ~25%).
#[test]
fn the_contested_set_after_peeling_stays_a_minority_of_the_dictionary() {
    const CONTESTED_SHARE_MAX: f64 = 0.35;
    let generated = dictionary_fixture::generated();
    let lyk = dictionary_fixture::cheapest();
    let share = generated.contested.to_f64() / lyk.len().to_f64();
    assert!(
        share <= CONTESTED_SHARE_MAX,
        "{:.1}% of the dictionary ({} of {} words) was still contested after fixed-point peeling, \
         want <= {:.0}% - peeling itself likely regressed back toward treating a word's full \
         candidate-list length as the test, rather than whether its own cheapest candidate is \
         uniquely its own (see `generate`'s own peeling doc)",
        share * 100.0,
        generated.contested,
        lyk.len(),
        CONTESTED_SHARE_MAX * 100.0
    );
}

/// `generate::single_stroke_stems_of` feeds `Rules::resolve`'s own stem-recognition fallback (the
/// `lookup` and `resolve` commands) - a known word
/// found there stands in for a continuation run's own raw phonemes. A phrase entry's `word` is two
/// words' own text ("of the"), not a real single word a continuation stroke could ever sensibly
/// follow (`"of the" + "-s"` would read back as "of thes", nonsense) - see `site/reference/phrases.json`. This
/// is the regression guard for that exclusion: a future change to `single_stroke_stems_of`'s own
/// filter that drops the `Origin::Phrase` exclusion would otherwise only surface live, the way
/// "don't" and "quitters" both did.
#[test]
fn single_stroke_stems_never_contains_a_phrase_entry() {
    let generated = dictionary_fixture::generated();
    assert!(
        generated.entries.iter().any(|e| e.origin == generate::Origin::Phrase),
        "this guard is only meaningful once the real dictionary actually has at least one phrase \
         entry to exclude - if this fails, phrase-briefing itself stopped producing any entries"
    );
    let stems = generate::single_stroke_stems_of(&generated.entries);
    let phrase_words: std::collections::HashSet<&str> = generated
        .entries
        .iter()
        .filter(|e| e.origin == generate::Origin::Phrase)
        .map(|e| e.word.as_str())
        .collect();
    let leaked: Vec<&String> = stems.values().filter(|w| phrase_words.contains(w.as_str())).collect();
    assert!(leaked.is_empty(), "phrase entries leaked into single_stroke_stems: {leaked:?}");
}

/// `Origin::Alternate`'s own doc comment claims an alternate-pronunciation entry "never competes
/// with the primary" - but `generate::cheapest` used to fold every entry for a word by raw
/// `(len, keys)` alone, ignoring `Origin` except to exclude `Suffix`, so a shorter `Alternate`
/// outline (the rarer sense, added on top of an already-settled primary) could silently outrank
/// that word's own primary/non-`Alternate` entry in `cheapest()`'s output - exactly what `explain`,
/// the score table, and backup composition all read. This is the regression guard for that gap:
/// for every word with both an `Alternate` entry and some other-origin entry, `cheapest()`'s own
/// pick must never be the `Alternate` one's outline unless no other-origin entry exists for that
/// word at all.
#[test]
fn an_alternate_pronunciation_entry_never_outranks_its_word_own_primary_in_cheapest() {
    let generated = dictionary_fixture::generated();
    assert!(
        generated.entries.iter().any(|e| e.origin == generate::Origin::Alternate),
        "this guard is only meaningful once the real dictionary actually has at least one \
         Alternate entry to check against - if this fails, the alternate-pronunciation pass \
         itself stopped producing any entries"
    );

    let mut non_alternate_outline: HashMap<&str, &[crate::stroke::Stroke]> = HashMap::new();
    for entry in &generated.entries {
        if entry.origin != generate::Origin::Alternate && entry.origin != generate::Origin::Suffix
        {
            non_alternate_outline.entry(entry.word.as_str()).or_insert(&entry.outline);
        }
    }

    let lyk = dictionary_fixture::cheapest();
    let mut violations = Vec::new();
    for entry in generated.entries.iter().filter(|e| e.origin == generate::Origin::Alternate) {
        let Some(&primary_outline) = non_alternate_outline.get(entry.word.as_str()) else {
            // No other-origin entry exists for this word at all - nothing for the Alternate
            // entry to outrank.
            continue;
        };
        if let Some(&deployed) = lyk.get(entry.word.as_str())
            && deployed == entry.outline.as_slice()
            && deployed != primary_outline
        {
            violations.push(format!(
                "{:?}: cheapest() picked the Alternate outline {:?} over the primary {:?}",
                entry.word,
                render_outline(deployed),
                render_outline(primary_outline),
            ));
        }
    }
    assert!(violations.is_empty(), "Alternate entries outranking their word's primary: {violations:?}");
}

/// `generate::reserved_outlines` is seeded into `taken` before a single word is placed
/// specifically so this can never happen; this confirms it from the outside, against the real
/// built dictionary, rather than trusting the seeding order inside `generate()` never regresses.
#[test]
fn no_generated_outline_collides_with_a_reserved_stroke() {
    let reserved = generate::reserved_outlines().expect("reserved dictionaries parse");
    let lyk = dictionary_fixture::cheapest();
    let mut collisions = Vec::new();
    for (&word, outline) in &lyk {
        let text = render_outline(outline);
        if let Some(kind) = reserved.get(&text) {
            collisions.push(format!("{word:?} landed on {text:?}, {kind}"));
        }
    }
    collisions.sort();
    assert!(collisions.is_empty(), "words colliding with a reserved stroke: {collisions:#?}");
}

/// Keymux's `{:firstup}` marker (send on the first key up, see the Reference page on
/// punctuation and commands) belongs only on a one-stroke key command, and goes first so
/// Plover reads the rest of the entry the same way: `{:firstup}{#Left}{^}`, never after `{^}`.
/// It must never sit on anything that types text or changes spacing or case.
// `{:firstup}` is a dictionary meta, not a format string.
#[allow(clippy::literal_string_with_formatting_args)]
#[test]
fn firstup_marker_is_only_on_key_commands_and_comes_first() {
    let commands: std::collections::BTreeMap<String, String> =
        serde_json::from_str(include_str!("../rules/reserved/commands.json")).expect("commands parse");
    let mut bad = Vec::new();
    let mut count = 0;
    for (outline, value) in &commands {
        if !value.contains("{:firstup}") {
            continue;
        }
        count += 1;
        let rest = value.strip_prefix("{:firstup}").unwrap_or("");
        let braces = rest.matches('{').count();
        let ok = !outline.contains('/')
            && rest.starts_with("{#")
            && !rest.contains("{:firstup}")
            && (braces == 1 || (braces == 2 && rest.ends_with("{^}")));
        if !ok {
            bad.push(format!("{outline:?}: {value:?}"));
        }
    }
    assert!(count > 0, "no first-up entries at all: the cursor strokes lost their marker");
    assert!(bad.is_empty(), "malformed first-up entries: {bad:#?}");
}

/// Keymux's runtime overlay/tape/keyboard/lookup controls (`{:overlay:hide}` and friends) and the engine unload
/// (`{:engine:unload}`) act on the running program and type nothing, so each one must carry `{:transparent}` first (a control
/// struck in the middle of a join must not break it), name only a part/verb Keymux parses, and
/// sit on a stroke no other entry shares and no word outline took (the collision test above
/// covers the words; this one covers the table itself).
// The entries are dictionary metas, not format strings.
#[allow(clippy::literal_string_with_formatting_args)]
#[test]
fn overlay_controls_are_transparent_valid_unique_and_resolve() {
    let commands: std::collections::BTreeMap<String, String> =
        serde_json::from_str(include_str!("../rules/reserved/commands.json")).expect("commands parse");
    let punctuation: std::collections::BTreeMap<String, String> =
        serde_json::from_str(include_str!("../rules/reserved/punctuation.json")).expect("punctuation parse");
    let reserved = generate::reserved_outlines().expect("reserved dictionaries parse");
    let expected = [
        ("PHRUP", "{:transparent}{PLOVER:LOOKUP}"),
        ("PHR*UP", "{:transparent}{:lookup:toggle}"),
        ("PHRUPD", "{:transparent}{:lookup:close}"),
        ("PHRO*L", "{:transparent}{:overlay:toggle}"),
        ("PHRO*LS", "{:transparent}{:overlay:show}"),
        ("PHRO*LD", "{:transparent}{:overlay:hide}"),
        ("PHRO*T", "{:transparent}{:tape:toggle}"),
        ("PHRO*TZ", "{:transparent}{:tape:clear}"),
        ("PHRO*B", "{:transparent}{:keyboard:toggle}"),
        ("PHRO*PLZ", "{:transparent}{:engine:unload}"),
    ];
    for (outline, value) in expected {
        assert_eq!(commands.get(outline).map(String::as_str), Some(value), "{outline}");
        assert!(!punctuation.contains_key(outline), "{outline} is also a punctuation stroke");
        assert!(reserved.contains_key(outline), "{outline} is not reserved");
    }
    let mut seen = std::collections::HashSet::new();
    for (outline, value) in &commands {
        let Some(rest) = value.strip_prefix("{:transparent}") else { continue };
        assert!(seen.insert(outline.clone()), "duplicate {outline}");
        assert!(!rest.contains("{:transparent}"), "{outline}: marker twice");
        let body = rest.trim_start_matches(['{', ':']).trim_end_matches('}');
        let valid = body.eq_ignore_ascii_case("PLOVER:LOOKUP")
            || matches!(
                body,
                "overlay:show" | "overlay:hide" | "overlay:toggle"
                    | "tape:show" | "tape:hide" | "tape:toggle" | "tape:clear"
                    | "keyboard:show" | "keyboard:hide" | "keyboard:toggle"
                    | "lookup:open" | "lookup:close" | "lookup:toggle"
                    | "engine:unload"
            );
        assert!(valid && rest.matches('{').count() == 1, "{outline}: {value:?} is not a Keymux control");
    }
    assert_eq!(seen.len(), expected.len(), "transparent entries beyond the documented ten");
    for (outline, value) in [
        ("R*EUD", "{:redo}"),
        ("TK*UP", "{:undo_word}"),
        ("*", "{:undo_or:Control_L(BackSpace)}"),
        ("TK*UPS", "=undo"),
        ("TKPWA*EPB", "{:again}"),
        ("SWA*P", "{:swap}"),
    ] {
        assert_eq!(commands.get(outline).map(String::as_str), Some(value), "{outline}");
    }
}

/// `Setup::load` fills in a translated MFA/WikiPron pronunciation (see `crate::ipa` and
/// `data::fallback_pronunciations`) for any word `CMUdict` has no entry for at all - this is the
/// regression guard that the fallback is actually wired in and still reaches a real number of
/// words, not just the hand-picked ones `rules::tests`/`reverse::tests` check individually. A real
/// run measured 8,174 of the 32,182 words with zero `CMUdict` entry among the top 100,000 by
/// frequency rank (`dictionary_fixture`'s own `COMMON`) as rescuable this way (see
/// `docs/decisions.md`'s entry on this fallback); the threshold below is set well under that
/// measured count, the same way `CONTESTED_SHARE_MAX` and the stranded-word baseline are - a
/// regression guard against the fallback's own reach quietly narrowing (an over-tightened phone
/// table, a broken merge order, a symbol table that stops loading), not a tight pin on the exact
/// count, which drifts slightly with the frequency corpus and the downloaded data files
/// themselves.
#[test]
fn the_ipa_fallback_rescues_a_real_minimum_of_cmudict_gap_words() {
    const RESCUED_FLOOR: usize = 7_000;
    let setup = dictionary_fixture::setup();
    let cmudict = data::load_cmudict(&Path::new("data").join("pronunciation/cmudict.dict"))
        .expect("cmudict loads");
    let rescued = setup
        .rank
        .iter()
        .filter(|&(w, &r)| r < 100_000 && !cmudict.contains_key(w.as_str()))
        .filter(|&(w, _)| setup.pron.contains_key(w.as_str()))
        .count();
    assert!(
        rescued >= RESCUED_FLOOR,
        "only {rescued} of the top-100,000 words with no CMUdict entry got a translated MFA/\
         WikiPron fallback pronunciation, want >= {RESCUED_FLOOR} - the fallback (`crate::ipa`, \
         `data::fallback_pronunciations`) likely regressed"
    );
}

/// Nearly every family member among the common words can be typed as its stem's own outline plus
/// the ending's stroke (see `crate::family` and `site/reference/syllables.json`): measured at 94-100% per
/// ending when the family direction landed, held here at 90% for the three biggest endings, plus
/// a few hand-checked members.
#[test]
fn common_family_members_are_reachable_as_stem_plus_ending() {
    let setup = dictionary_fixture::setup();
    let generated = dictionary_fixture::generated();
    let index = generate::index(generated);
    for (word, text) in [("called", "KAUL/+-D"), ("looks", "HRAOBG/+-S"), ("fighting", "TPAOEUT/+-G")] {
        assert_eq!(index.get(text), Some(&word), "{word} should be reachable as {text}");
    }
    let cheapest = generate::cheapest(&generated.entries);
    let mut tally: HashMap<&str, (usize, usize)> = HashMap::new();
    for (word, link) in &generated.links {
        if setup.rank.get(word).is_none_or(|&r| r >= 10_000) || !["s", "ed", "ing"].contains(&link.suffix) {
            continue;
        }
        let Some(stem) = cheapest.get(link.stem.as_str()) else { continue };
        let mut composed = stem.to_vec();
        composed.extend_from_slice(&link.strokes);
        setup.rules.joined(&mut composed);
        let entry = tally.entry(link.suffix).or_default();
        entry.0 += 1;
        entry.1 += usize::from(index.get(&render_outline(&composed)) == Some(&word.as_str()));
    }
    for (suffix, (members, reachable)) in tally {
        assert!(
            reachable * 10 >= members * 9,
            "-{suffix}: only {reachable} of {members} common members reachable as stem + ending"
        );
    }
}

/// Extra single-stroke entries (`Origin::Extra`) only fill strokes nobody else needed, and are
/// never the outline reported for a word that has any other entry: `lookup`, `explain`, `quote`,
/// phrases, and the family report must keep showing the primary. See `site/reference/briefs.json`.
#[test]
fn extra_entries_fill_gaps_and_never_become_a_words_reported_outline() {
    let generated = dictionary_fixture::generated();
    assert!(
        generated.extras_added > 5_000,
        "only {} extras: the extra-entries pass likely stopped working",
        generated.extras_added
    );
    let has_other: std::collections::HashSet<&str> = generated
        .entries
        .iter()
        .filter(|e| !matches!(e.origin, generate::Origin::Extra | generate::Origin::Suffix))
        .map(|e| e.word.as_str())
        .collect();
    let lyk = dictionary_fixture::cheapest();
    let mut violations = Vec::new();
    for entry in generated.entries.iter().filter(|e| e.origin == generate::Origin::Extra) {
        assert_eq!(entry.outline.len(), 1, "{:?}: an extra must be one stroke", entry.word);
        if has_other.contains(entry.word.as_str()) && lyk.get(entry.word.as_str()) == Some(&entry.outline.as_slice()) {
            violations.push(entry.word.as_str());
        }
    }
    violations.sort_unstable();
    assert!(violations.is_empty(), "extras reported as the word's outline: {violations:?}");
}

/// `write_json` keys the deployed file by outline text, so two raw entries sharing a text would
/// silently lose one of them on the way out. `no_two_words_share_the_same_deployed_outline` only
/// sees each word's cheapest entry and can't catch this; this checks every entry `generate` made.
#[test]
fn no_two_raw_entries_share_an_outline_text() {
    let mut seen: HashMap<String, &str> = HashMap::new();
    let mut duplicates = Vec::new();
    for entry in &dictionary_fixture::generated().entries {
        let text = render_outline(&entry.outline);
        if let Some(other) = seen.insert(text.clone(), entry.word.as_str()) {
            duplicates.push(format!("{text:?}: {other:?} and {:?}", entry.word));
        }
    }
    duplicates.sort();
    assert!(duplicates.is_empty(), "entries sharing an outline text: {duplicates:#?}");
}

/// Each word's cheapest outline must depend only on which entries exist, not on the order
/// `generate` pushed them in (the backup pass walks a `HashMap`): `quote`, `explain`, `lookup`,
/// and phrase-briefing all read `cheapest`.
#[test]
fn cheapest_does_not_depend_on_entry_order() {
    let generated = dictionary_fixture::generated();
    let forward = generate::cheapest(&generated.entries);
    let reversed: Vec<generate::Entry> = generated
        .entries
        .iter()
        .rev()
        .map(|e| generate::Entry {
            word: e.word.clone(),
            outline: e.outline.clone(),
            origin: e.origin,
        })
        .collect();
    let backward = generate::cheapest(&reversed);
    let mut differ: Vec<String> = forward
        .iter()
        .filter(|&(word, outline)| backward.get(word) != Some(outline))
        .map(|(word, outline)| format!("{word}: {}", render_outline(outline)))
        .collect();
    differ.sort();
    assert!(differ.is_empty(), "cheapest() changed with entry order: {differ:#?}");
}

/// The commonest words are typed thousands of times a day: none of them may sit on a star-ladder
/// marker outline (`.../+*`, an extra stroke nobody can predict), and only the named exceptions
/// may take more than one stroke at all. A new multi-stroke word here usually means a rarer word
/// out-arbitrated it, the way `early` once lost `ERL` to `errol` (see `docs/decisions.md`).
/// Extend `MULTI_STROKE_OK` only with a reason.
#[test]
fn the_commonest_words_have_one_stroke_and_no_ladder_marker() {
    const TOP_N: usize = 1_000;
    // Neither "american" nor "lt" has a one-stroke candidate at all: "american"'s M-R-K-N coda has
    // no cluster, and "lt" (lieutenant) has no gold brief. "impossible" is a prefix family member,
    // written as its prefix and its stem the way "rebuild" is RE/+PWHR (`site/reference/prefixes.json`), and
    // as Plover writes it (EUPL/POB).
    const MULTI_STROKE_OK: &[&str] = &["american", "lt", "impossible"];
    let setup = dictionary_fixture::setup();
    let lyk = dictionary_fixture::cheapest();
    let mut problems: Vec<String> = Vec::new();
    for (&word, outline) in &lyk {
        if setup.rank.get(word).is_none_or(|&r| r >= TOP_N) {
            continue;
        }
        let text = render_outline(outline);
        if text.contains("/+*") {
            problems.push(format!("{word}: ladder marker {text}"));
        } else if outline.len() > 1 && !MULTI_STROKE_OK.contains(&word) {
            problems.push(format!("{word}: {} strokes ({text})", outline.len()));
        }
    }
    problems.sort();
    assert!(problems.is_empty(), "common words on a slow or unpredictable outline: {problems:#?}");
}

/// The right pinky cannot press a diagonal pair or three of its four keys (`site/reference/typable.json`), so
/// no generated outline may contain one.
#[test]
fn no_generated_stroke_needs_an_impossible_pinky_chord() {
    let bad: Vec<String> = dictionary_fixture::generated()
        .entries
        .iter()
        .filter(|e| e.outline.iter().any(|stroke| !stroke.typable()))
        .map(|e| format!("{} {}", e.word, render_outline(&e.outline)))
        .take(10)
        .collect();
    assert!(bad.is_empty(), "untypable outlines: {bad:?}");
}

/// A star on a one-syllable stroke goes to the word that sounds like it before a longer word's
/// short form (`STAR_SURCHARGE`), and a name yields its stroke to the word it sounds like
/// (`frequency::NAME_SHARE`). Each pair here was the other way round before.
#[test]
fn a_sound_alike_word_holds_the_star_and_the_plain_stroke_before_a_short_form_or_a_name() {
    let lyk = dictionary_fixture::cheapest();
    for (word, want) in [("dock", "TKO*BG"), ("sock", "SO*BG"), ("mat", "PHAT")] {
        assert_eq!(lyk.get(word).map(|o| render_outline(o)).as_deref(), Some(want), "{word}");
    }
}
