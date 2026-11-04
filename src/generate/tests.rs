use super::*;
use super::solver::solve_contested;
use crate::stroke::parse_outline;

fn entry(word: &str, outline: &str, origin: Origin) -> Entry {
    Entry {
        word: word.to_owned(),
        outline: parse_outline(outline).unwrap(),
        origin,
        lost_to: None,
    }
}

use std::sync::LazyLock;

/// The same `simple`/`wanted` sets `generate` builds from the real, full dictionary, shared
/// across every test below instead of recomputed per word (the groups pass alone touches every
/// word in the dictionary).
static GROUPS: LazyLock<(HashSet<String>, HashSet<String>)> = LazyLock::new(|| {
    let setup = crate::dictionary_fixture::setup();
    let words = ranked_words(setup, 100_000);
    group_outline_sets(setup, &words)
});

fn candidates_for(word: &str, briefs: &Briefs) -> Vec<Candidate> {
    let setup = crate::dictionary_fixture::setup();
    let (simple, wanted) = &*GROUPS;
    let simple: HashSet<&str> = simple.iter().map(String::as_str).collect();
    let wanted: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    enumerate_candidates(setup, briefs, word, &simple, &wanted)
}

/// The real, built-in briefs, minus the named `[word]` overrides - simulates the dictionary's
/// state *after* the acceptance-gate override removals this plan's verification calls for, so
/// a test can check what the new pipeline does unprompted, without needing those overrides
/// gone from `rules/briefs.toml` yet.
fn briefs_without(words: &[&str]) -> Briefs {
    let mut value: toml::Value = toml::from_str(BUILT_IN_BRIEFS).expect("built-in briefs parse");
    if let Some(table) = value.get_mut("word").and_then(toml::Value::as_table_mut) {
        for word in words {
            table.remove(*word);
        }
    }
    value.try_into().expect("briefs minus a few words still deserializes")
}

fn cheapest_candidate(candidates: &[Candidate]) -> &Candidate {
    candidates.iter().min_by_key(|c| c.cost).expect("at least one candidate")
}

/// A word spelled with a literal trailing `s` whose phonetic coda strokes as a voiced `-Z`
/// gets a same-length `-S` alternate too (`Origin::Voicing`), so the cheaper of the two can
/// win on cost instead of `-Z` winning by default just because nothing else was ever offered
/// at that stroke length - see `enumerate_candidates`'s own doc, source 6.
#[test]
fn a_word_spelled_with_s_gets_a_same_length_s_for_z_alternate() {
    let briefs = Briefs::built_in().unwrap();
    let candidates = candidates_for("abhors", &briefs);
    assert!(
        candidates.iter().any(|c| c.origin == Origin::Voicing && render_outline(&c.outline) == "PWHORS"),
        "abhors should offer PWHORS as a Voicing alternate next to its phonetic PWHORZ"
    );
    // Real, measured consequence of this: PWHORS costs less than PWHORZ, so it is the
    // natural pool's cheapest here - not every `-Z`-coda word flips (ergonomics can still
    // prefer `-Z`), but this one does, exactly as the per-word cost comparison intends.
    let cheapest = cheapest_candidate(&candidates);
    assert_eq!(render_outline(&cheapest.outline), "PWHORS");
}

/// A word spelled with a literal `z` (never `s`) never qualifies for the alternate above -
/// `word.ends_with('s')` alone already excludes it, since a word can't end in both letters.
#[test]
fn a_word_spelled_with_z_never_gets_a_spurious_s_alternate() {
    let briefs = Briefs::built_in().unwrap();
    for word in ["buzz", "quiz", "jazz"] {
        let candidates = candidates_for(word, &briefs);
        assert!(
            !candidates.iter().any(|c| c.origin == Origin::Voicing),
            "{word} is spelled with z, not s, and must never get a Voicing alternate"
        );
    }
}

#[test]
fn a_word_named_in_briefs_toml_as_a_literal_outline_gets_exactly_that_one_candidate() {
    // `up` is pinned today (`rules/briefs.toml`, `up = "-P"`) to a one-key brief its own plain
    // rules reading ("UP") would never reach on its own - a human decision this function must
    // never reopen to competition against its own word's other candidate sources.
    let briefs = Briefs::built_in().unwrap();
    let candidates = candidates_for("up", &briefs);
    assert_eq!(candidates.len(), 1, "a literal override must force exactly one candidate");
    assert_eq!(render_outline(&candidates[0].outline), "-P");
    assert_eq!(candidates[0].origin, Origin::Brief);
}

#[test]
fn candidate_cost_orders_by_strokes_then_keys_then_ergonomics() {
    let one_stroke_few_keys = parse_outline("TK").unwrap();
    let one_stroke_more_keys = parse_outline("STKPW").unwrap();
    let two_strokes = parse_outline("TK/TK").unwrap();
    assert!(candidate_cost(&one_stroke_few_keys) < candidate_cost(&one_stroke_more_keys));
    assert!(candidate_cost(&one_stroke_more_keys) < candidate_cost(&two_strokes));
}

#[test]
fn the_take_takes_inconsistency_is_gone_from_the_candidate_pool() {
    // `take` happens to have a short brief Plover lists directly (`TAEBG`); `takes` does not
    // (Plover's own shortest listed candidate is the longer `TAEUBGS`) - today's `best_brief`
    // only ever sees each word's own gold-listed candidates, so `takes` never got a chance to
    // compete against lyk's own shorter spelling-variant reading, `TAEBGS`, the way `take`'s
    // own `TAEBG` already matches. Both are now in the same pool for both words.
    let briefs = Briefs::built_in().unwrap();
    let take = candidates_for("take", &briefs);
    let takes = candidates_for("takes", &briefs);
    assert!(
        take.iter().any(|c| render_outline(&c.outline) == "TAEBG"),
        "take's candidates: {:?}",
        take.iter().map(|c| render_outline(&c.outline)).collect::<Vec<_>>()
    );
    assert!(
        takes.iter().any(|c| render_outline(&c.outline) == "TAEBGS"),
        "takes's candidates: {:?}",
        takes.iter().map(|c| render_outline(&c.outline)).collect::<Vec<_>>()
    );
    assert_eq!(render_outline(&cheapest_candidate(&take).outline), "TAEBG");
    assert_eq!(render_outline(&cheapest_candidate(&takes).outline), "TAEBGS");
}

#[test]
fn the_acceptance_gate_reproduces_or_beats_every_removed_override() {
    // Plan verification step 4: the 17 `rules/briefs.toml` overrides whose own comments cited
    // the greedy-ordering/fragment-check bug this whole redesign exists to fix have now been
    // removed from the real file - this runs the real, full dictionary (`dictionary_fixture`,
    // the built-in briefs as they actually ship) and checks every one of those 17 words landed
    // on the override's own outline or something strictly cheaper.
    //
    // `on`/`from`/`no`/`then`/`day`/`which` were briefly a different shape of check here: an
    // earlier version of `gold_candidates` rejected any raw candidate `matches_real_consonants`
    // flagged (a half-held chord, like `which`'s own `KH` missing half of `WH`'s real chord),
    // which forced these six back up to their full phonetic reading - a real, user-confirmed
    // regression against the day-4 philosophy reversal ("nothing about a candidate needs to
    // sound like the word"), reverted once found. The remaining 17 words still compare that
    // way: the real dictionary's cost must be at or below the removed override's own cost.
    // `which` itself moved back out of this list - not because that bug came back, but because
    // the ergonomics keyboard-geometry revision's own retuning later shifted a real, unrelated
    // global-solve collision (`which` vs. `charge`, both wanting the text "KH") the other way;
    // see `rules/briefs.toml`'s own `which = "KH"` entry and comment, which pins it back.
    let setup = crate::dictionary_fixture::setup();
    let lyk = crate::dictionary_fixture::cheapest();
    for (word, overridden) in OVERRIDDEN_WORDS {
        let mut forced = parse_outline(overridden).expect("every override text parses");
        setup.rules.joined(&mut forced);
        let overridden_cost = candidate_cost(&forced);
        let actual = lyk
            .get(word)
            .unwrap_or_else(|| panic!("{word:?} has no outline at all in the real dictionary"));
        let actual_cost = candidate_cost(actual);
        assert!(
            actual_cost <= overridden_cost,
            "{word}: the real dictionary's outline ({}, cost {actual_cost}) is worse than the \
             removed override {overridden:?} (cost {overridden_cost})",
            render_outline(actual)
        );
    }
}

#[test]
#[ignore = "diagnostic: run with --ignored --nocapture to inspect the natural candidate pool"]
fn dump_natural_candidates_for_the_override_words() {
    let briefs = briefs_without(&[
        "make", "but", "on", "from", "no", "then", "day", "which", "wow", "honest", "bottle",
        "public", "hiding", "gasps", "flowers", "grunting", "honey", "bloody", "against",
        "protect", "perhaps", "govern",
    ]);
    for word in [
        "make", "but", "on", "from", "no", "then", "day", "which", "wow", "honest", "bottle",
        "public", "hiding", "gasps", "flowers", "grunting", "honey", "bloody", "against",
        "protect", "perhaps", "govern",
    ] {
        let candidates = candidates_for(word, &briefs);
        let mut rows: Vec<(String, Origin, u32)> = candidates
            .iter()
            .map(|c| (render_outline(&c.outline), c.origin, c.cost))
            .collect();
        rows.sort_by_key(|(_, _, cost)| *cost);
        println!("{word}:");
        for (text, origin, cost) in rows {
            println!("  {text:<16} {origin:?} cost={cost}");
        }
    }
}

/// The exact `[word]` overrides the acceptance gate (`docs/decisions.md`) removes once this
/// pipeline is proven to reproduce or beat them, next to the outline each one forces today.
const OVERRIDDEN_WORDS: &[(&str, &str)] = &[
    ("make", "PHAEUBG"),
    ("but", "PWUT"),
    ("on", "OB"),
    ("from", "PR"),
    ("no", "PHO"),
    ("then", "THEB"),
    ("day", "TAEU"),
    ("wow", "WOU"),
    ("honest", "OPBS"),
    ("bottle", "PWOLT"),
    ("public", "PUBLG"),
    ("hiding", "HAOEUGD"),
    ("gasps", "TKPWAFPS"),
    ("flowers", "TPHROURZ"),
    ("grunting", "TKPWRUPBGT"),
    ("honey", "HOPB/EU"),
    ("bloody", "PWHROD/EU"),
];

#[test]
fn the_natural_pool_never_costs_more_than_the_override_it_would_replace() {
    // Verification step 1 of the plan this implements: before the real solver ever runs,
    // each of these words' own natural candidate list (brief, rules, spelling, brevity,
    // ladder - the override itself excluded) must have a cheapest member that costs no more
    // than the override it is meant to replace. This is the half of "reproduces or beats"
    // that is checkable per word, in isolation, before frequency-weighted arbitration against
    // the rest of the dictionary decides which word actually keeps a contested outline - see
    // `the_fragment_check_words_all_find_something_cheaper_not_worse` below for what these
    // words' own cheapest candidate actually turns out to be, and why.
    let words: Vec<&str> = OVERRIDDEN_WORDS.iter().map(|(w, _)| *w).collect();
    let briefs = briefs_without(&words);
    for &(word, overridden) in OVERRIDDEN_WORDS {
        let mut forced = parse_outline(overridden).expect("every override text parses");
        // Multi-stroke overrides (`honey`, `bloody`) are written `"HOPB/EU"` - their own
        // outline needs the same join-key marking `enumerate_candidates`'s own `push` applies
        // to every candidate, for a fair cost comparison.
        setup_for_join(&mut forced);
        let overridden_cost = candidate_cost(&forced);
        let candidates = candidates_for(word, &briefs);
        let cheapest = cheapest_candidate(&candidates);
        assert!(
            cheapest.cost <= overridden_cost,
            "{word}: natural pool's cheapest ({}, cost {}) is worse than the override \
             {overridden:?} (cost {overridden_cost}) - a real bug in enumeration or the cost \
             function, not an acceptable tradeoff",
            render_outline(&cheapest.outline),
            cheapest.cost
        );
    }
}

fn setup_for_join(outline: &mut [Stroke]) {
    let setup = crate::dictionary_fixture::setup();
    setup.rules.joined(outline);
}

#[test]
fn six_of_the_fragment_check_overrides_reproduce_exactly_in_isolation() {
    // Unlike `make`/`but` (`drop_fragments` finds an even deeper, legal strip than the human
    // override itself used) or `on`/`from`/`no`/`then`/`day`/`which` (their own override text
    // is a real, legal candidate again - see `gold_candidates`'s own doc for the blanket-filter
    // revert), these six stranded-word fixes are already the natural pool's own cheapest
    // candidate, confirmed here in isolation before the real solver (with every other word also
    // contesting the dictionary) ever runs. `flowers` wants `TPHROURS`, not the old override's
    // `TPHROURZ`: the S-for-Z voicing step (`enumerate_candidates`'s own doc, source 6) now
    // offers that respelling too, and it costs less - a real improvement on the override this
    // test only ever checked was *reproduced*, never that it was optimal.
    let exact: &[(&str, &str)] = &[
        ("wow", "WOU"),
        ("honest", "OPBS"),
        ("bottle", "PWOLT"),
        ("public", "PUBLG"),
        ("hiding", "HAOEUGD"),
        ("flowers", "TPHROURS"),
    ];
    let words: Vec<&str> = exact.iter().map(|(w, _)| *w).collect();
    let briefs = briefs_without(&words);
    for &(word, want) in exact {
        let candidates = candidates_for(word, &briefs);
        let cheapest = cheapest_candidate(&candidates);
        assert_eq!(
            render_outline(&cheapest.outline),
            want,
            "{word}: expected the override's own outline to already be the natural pool's \
             cheapest"
        );
    }
}

fn candidate(text: &str) -> Candidate {
    Candidate { outline: parse_outline(text).unwrap(), origin: Origin::Rules, cost: 0 }
}

/// A small, hand-worked bipartite graph, checked against a by-hand-computed optimum before
/// trusting `solve_contested` against the real dictionary (plan verification step 2). Three
/// words, deliberately overlapping candidates:
///
/// - `a` can take `T` (edge cost 1,000) or `K` (edge cost 2,000)
/// - `b` can take `T` (edge cost 50) or `K` (edge cost 80)
/// - `c` can take `K` (edge cost 1) or `P` (edge cost 50)
///
/// Only two perfect matchings exist (every word must get a distinct text): `a`=T,`b`=K,`c`=P
/// costs 1,000 + 80 + 50 = 1,130; `a`=K,`b`=T,`c`=P costs 2,000 + 50 + 50 = 2,100 (any
/// assignment giving `c` its own cheapest, `K`, leaves `a` and `b` both needing `T`, which
/// isn't a perfect matching). The cheaper one is the only correct answer.
/// The cheapest assignment of distinct texts to words, by trying every one: each word takes one
/// of its `(text, cost)` options, or, when `escape` is given, its own escape cost instead. `None`
/// when no assignment uses each text once.
fn brute_force(options: &[Vec<(usize, u128)>], escape: Option<&[u128]>) -> Option<u128> {
    fn go(options: &[Vec<(usize, u128)>], escape: Option<&[u128]>, i: usize, used: &mut Vec<usize>, cost: u128, best: &mut Option<u128>) {
        if best.is_some_and(|b| cost >= b) {
            return;
        }
        if i == options.len() {
            *best = Some(cost);
            return;
        }
        if let Some(escape) = escape {
            go(options, Some(escape), i + 1, used, cost + escape[i], best);
        }
        for &(t, c) in &options[i] {
            if !used.contains(&t) {
                used.push(t);
                go(options, escape, i + 1, used, cost + c, best);
                used.pop();
            }
        }
    }
    let mut best = None;
    go(options, escape, 0, &mut Vec::new(), 0, &mut best);
    best
}

/// A deterministic pseudo-random sequence from `seed` (xorshift64).
fn xorshift(mut seed: u64) -> impl FnMut() -> u64 {
    move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    }
}

#[test]
fn solve_contested_matches_a_small_hand_worked_optimum() {
    let contested: Vec<(&str, Vec<(Candidate, u64)>)> = vec![
        ("a", vec![(candidate("T"), 1_000), (candidate("K"), 2_000)]),
        ("b", vec![(candidate("T"), 50), (candidate("K"), 80)]),
        ("c", vec![(candidate("K"), 1), (candidate("P"), 50)]),
    ];
    let chosen = solve_contested(&contested, &vec![u64::MAX / 4; contested.len()]);
    let text_of = |word: &str, index: usize| -> String {
        let (_, candidates) = contested.iter().find(|(w, _)| *w == word).unwrap();
        render_outline(&candidates[index].0.outline)
    };
    assert_eq!(text_of("a", chosen[&"a"]), "T");
    assert_eq!(text_of("b", chosen[&"b"]), "K");
    assert_eq!(text_of("c", chosen[&"c"]), "P");
}

/// A second hand-worked case, specifically to confirm this is genuine global optimization and
/// not "give the cheapest text to whichever word is worth the most": `d` can take `T` (edge
/// cost 10) or `K` (edge cost 500); `e` can take `T` (edge cost 100) or `K` (edge cost 200).
/// `e`'s own edges cost more in absolute terms (it is the "more valuable" word, in the sense
/// that satisfying it is expensive either way), but the cheapest *total* is still `d`=T, `e`=K
/// (10 + 200 = 210), not `d`=K, `e`=T (500 + 100 = 600) - `d` cannot tolerate losing `T` nearly
/// as well as `e` can tolerate losing it, which a naive "richest word wins the cheapest slot"
/// rule would get backwards.
#[test]
fn solve_contested_minimizes_total_cost_not_the_richest_words_own_pick() {
    let contested: Vec<(&str, Vec<(Candidate, u64)>)> = vec![
        ("d", vec![(candidate("T"), 10), (candidate("K"), 500)]),
        ("e", vec![(candidate("T"), 100), (candidate("K"), 200)]),
    ];
    let chosen = solve_contested(&contested, &vec![u64::MAX / 4; contested.len()]);
    let text_of = |word: &str, index: usize| -> String {
        let (_, candidates) = contested.iter().find(|(w, _)| *w == word).unwrap();
        render_outline(&candidates[index].0.outline)
    };
    assert_eq!(text_of("d", chosen[&"d"]), "T");
    assert_eq!(text_of("e", chosen[&"e"]), "K");
}

#[test]
fn solve_contested_matches_brute_force_on_a_real_failing_case() {
    // Exact edge costs pulled from a real build's "he"/"eh"/"heh" component: `solve_contested`
    // was observed assigning "he" (by far the heaviest word here) its own *worst* candidate
    // (HAOE) while "eh" and "heh" took the cheap "E"/"HE" texts "he" should have won easily -
    // a real bug this test pins down outside the full ~100k-word build.
    let data: &[(&str, &[(&str, u64)])] = &[
        ("heh", &[("HE", 20_516_952_000), ("HE/+*", 41_033_904_000), ("HE/+*/+*", 61_550_856_000)]),
        (
            "ye",
            &[
                ("HE", 10_364_688_000),
                ("KWRAO*E", 10_416_408_000),
                ("KWRAOE/+*", 20_770_752_000),
                ("KWRAOE/+*/+*", 31_135_440_000),
            ],
        ),
        ("hedge", &[("HE", 1_780_554_000), ("HEPBLG/+*", 3_568_219_554), ("HEPBLG/+*/+*", 5_348_773_554)]),
        (
            "he",
            &[
                ("E", 5_521_880_364_000),
                ("HE", 5_527_396_728_000),
                ("HAOE", 5_538_429_456_000),
                ("HAO*E", 5_543_945_820_000),
                ("HAOE/+*", 11_065_826_184_000),
                ("HAOE/+*/+*", 16_593_222_912_000),
            ],
        ),
        (
            "yee",
            &[
                ("HAOE", 1_214_840_000),
                ("KWRAO*E", 1_218_470_000),
                ("KWRAOE/+*", 2_429_680_000),
                ("KWRAOE/+*/+*", 3_642_100_000),
            ],
        ),
        ("eh", &[("E", 75_253_178_000), ("E/+*", 150_581_534_000), ("E/+*/+*", 225_909_890_000)]),
        (
            "edge",
            &[
                ("E", 19_534_515_000),
                ("EPBLG", 19_612_614_030),
                ("EPBLG/+*", 39_166_644_030),
                ("EPBLG/+*/+*", 58_720_674_030),
            ],
        ),
        (
            "hee",
            &[
                ("HAOE", 6_231_828_000),
                ("HAO*E", 6_238_035_000),
                ("HAOE/+*", 12_451_242_000),
                ("HAOE/+*/+*", 18_670_656_000),
            ],
        ),
    ];
    let contested: Vec<(&str, Vec<(Candidate, u64)>)> = data
        .iter()
        .map(|&(word, edges)| (word, edges.iter().map(|&(text, cost)| (candidate(text), cost)).collect()))
        .collect();

    let mut texts: Vec<&str> = data.iter().flat_map(|(_, edges)| edges.iter().map(|&(t, _)| t)).collect();
    texts.sort_unstable();
    texts.dedup();
    let options: Vec<Vec<(usize, u128)>> = data
        .iter()
        .map(|(_, edges)| edges.iter().map(|&(t, c)| (texts.binary_search(&t).unwrap(), u128::from(c))).collect())
        .collect();
    let want = brute_force(&options, None).expect("a perfect matching exists for this graph");

    let chosen = solve_contested(&contested, &vec![u64::MAX / 4; contested.len()]);
    let got: u128 = data.iter().enumerate().map(|(i, (word, _))| options[i][chosen[word]].1).sum();
    assert_eq!(got, want, "solver found {got}, brute force found the true optimum {want}");
}

#[test]
fn solve_contested_handles_a_larger_random_bipartite_graph() {
    // A brute-force cross-check at a size too large to verify by hand: 8 words, each with 2-4
    // candidate texts drawn from a shared pool of 10, deterministic pseudo-random costs. Brute
    // force enumerates every perfect matching directly; the solver must find the same optimum.
    let mut rand = xorshift(12345);
    let texts = ["S", "T", "K", "P", "W", "H", "R", "A", "O", "E"];
    let words: Vec<&str> = vec!["w0", "w1", "w2", "w3", "w4", "w5", "w6", "w7"];
    let mut options: Vec<Vec<(usize, u64)>> = Vec::new();
    for _ in &words {
        let n = 2 + usize::try_from(rand() % 3).unwrap();
        let mut chosen: Vec<usize> = Vec::new();
        while chosen.len() < n {
            let t = usize::try_from(rand() % u64::try_from(texts.len()).unwrap()).unwrap();
            if !chosen.contains(&t) {
                chosen.push(t);
            }
        }
        let opts: Vec<(usize, u64)> = chosen.into_iter().map(|t| (t, 1 + rand() % 1000)).collect();
        options.push(opts);
    }
    let contested: Vec<(&str, Vec<(Candidate, u64)>)> = words
        .iter()
        .zip(&options)
        .map(|(&w, opts)| (w, opts.iter().map(|&(t, cost)| (candidate(texts[t]), cost)).collect()))
        .collect();

    let wide = |options: &[Vec<(usize, u64)>]| -> Vec<Vec<(usize, u128)>> {
        options.iter().map(|o| o.iter().map(|&(t, c)| (t, u128::from(c))).collect()).collect()
    };
    let want = brute_force(&wide(&options), None).expect("a perfect matching exists for this graph");

    let chosen = solve_contested(&contested, &vec![u64::MAX / 4; contested.len()]);
    let got: u128 = words.iter().enumerate().map(|(i, w)| u128::from(options[i][chosen[w]].1)).sum();
    assert_eq!(got, want, "solver found {got}, brute force found the true optimum {want}");
}

#[test]
fn written_capitalizes_only_the_pronoun_i() {
    assert_eq!(written("i"), "I");
    assert_eq!(written("i'm"), "I'm");
    assert_eq!(written("i would"), "I would");
    assert_eq!(written("and i"), "and I");
    assert_eq!(written("it"), "it");
    assert_eq!(written("i've been"), "I've been");
    assert_eq!(written("ice"), "ice");
}

/// Many random graphs, deliberately over-subscribed (more words than the shared text pool can
/// always serve), each checked against brute force where any word may instead take its own
/// no-match escape at its given cost. Pins the early-exit Dijkstra and its capped potential
/// update to the exact optimum, Hall violations included.
#[test]
fn solve_contested_matches_brute_force_with_escapes_across_many_seeds() {
    let texts = ["S", "T", "K", "P", "W", "H"];
    let names = ["w0", "w1", "w2", "w3", "w4", "w5", "w6", "w7"];
    for seed0 in 1..=300u64 {
        let mut rand = xorshift(seed0.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let word_count = 3 + usize::try_from(rand() % 6).unwrap();
        let mut options: Vec<Vec<(usize, u64)>> = Vec::new();
        let mut escape: Vec<u64> = Vec::new();
        for _ in 0..word_count {
            let n = 1 + usize::try_from(rand() % 3).unwrap();
            let mut chosen: Vec<usize> = Vec::new();
            while chosen.len() < n {
                let t = usize::try_from(rand() % 6).unwrap();
                if !chosen.contains(&t) {
                    chosen.push(t);
                }
            }
            let opts: Vec<(usize, u64)> = chosen.into_iter().map(|t| (t, 1 + rand() % 1000)).collect();
            let worst = opts.iter().map(|&(_, c)| c).max().unwrap_or(0);
            escape.push(worst + 1 + rand() % 2000);
            options.push(opts);
        }
        let contested: Vec<(&str, Vec<(Candidate, u64)>)> = names[..word_count]
            .iter()
            .zip(&options)
            .map(|(&w, opts)| (w, opts.iter().map(|&(t, cost)| (candidate(texts[t]), cost)).collect()))
            .collect();
        let wide: Vec<Vec<(usize, u128)>> =
            options.iter().map(|o| o.iter().map(|&(t, c)| (t, u128::from(c))).collect()).collect();
        let escapes: Vec<u128> = escape.iter().copied().map(u128::from).collect();
        let want = brute_force(&wide, Some(&escapes)).expect("every word can always escape");
        let chosen = solve_contested(&contested, &escape);
        let got: u128 = (0..word_count).map(|i| u128::from(chosen.get(names[i]).map_or(escape[i], |&c| options[i][c].1))).sum();
        assert_eq!(got, want, "seed {seed0}: solver found {got}, brute force {want}");
    }
}

#[test]
fn a_standalone_suffixs_own_entry_never_wins_cheapest_even_when_shorter() {
    // "less" is both a real word (HRES, 4 keys) and the display text of the standalone
    // "-less" suffix's own continuation stroke (+-LS, 2 keys) - a stroke that is never a
    // word by itself when struck alone (see the join key), so it must never be picked as
    // "the" outline for the real word just because it numerically looks cheaper.
    let entries = vec![
        entry("less", "+-LS", Origin::Suffix),
        entry("less", "HRES", Origin::Rules),
    ];
    let best = cheapest(&entries);
    assert_eq!(best.get("less").copied(), Some(&parse_outline("HRES").unwrap()[..]));
}

#[test]
fn multi_syllable_words_get_suffix_prefix_and_syllable_joins() {
    let generated = crate::dictionary_fixture::generated();
    let has = |word: &str, outline: &str| {
        generated.entries.iter().any(|e| e.word == word && render_outline(&e.outline) == outline)
    };
    // The wider backup endings and prefixes: a stem you know, then a suffix or after a prefix
    assert!(has("foolish", "TPAOL/+-RB"), "fool + -ish");
    assert!(has("artist", "ART/+EUFT"), "art + -ist");
    // An ending past the family set builds on the stem's plain reading, not its brief (`TPH-F`)
    assert!(has("information", "EUPB/+TPORPL/+AEUGS"), "inform + -ation");
    assert!(has("andy", "APBD/+AOE") && !has("andy", "SKP/+AOE"), "and + -y, never the brief");
    assert!(!has("wither", "W/+-R"), "wither is not with + -er");
    assert!(has("unclear", "UPB/+KHRAOER"), "un + clear");
    assert!(has("decode", "TKE/+KOED"), "de + code");
    // Syllable paths, beside however short the primary is
    assert!(has("table", "TAEU/+-BL"));
    assert!(has("information", "EUPB/+TPOR/+PHAEU/+-GS"));
    // Every word of two syllables or more in the fixture has some joined way in
    let joined: HashSet<&str> = generated
        .entries
        .iter()
        .filter(|e| e.outline.len() >= 2 && !render_outline(&e.outline).contains("/+*"))
        .map(|e| e.word.as_str())
        .collect();
    let setup = crate::dictionary_fixture::setup();
    let missing: Vec<&str> = generated
        .entries
        .iter()
        .map(|e| e.word.as_str())
        .filter(|w| {
            setup.pron.get(*w).and_then(|p| p.first()).is_some_and(|p| {
                p.iter().filter(|s| s.ends_with(|c: char| c.is_ascii_digit())).count() >= 2
            })
        })
        .filter(|w| setup.count.get(*w).copied().unwrap_or(0) > 0 && !joined.contains(w))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    assert!(missing.len() * 100 < joined.len() * 3, "{} multi-syllable words have no joined way in: {:?}", missing.len(), &missing[..missing.len().min(20)]);
}
