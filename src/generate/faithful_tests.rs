//! The faithful rule, held against the real built dictionary (see `Rules::faithful`).

use super::*;

/// No word past the exemption rank has a primary outline from a gold dictionary that leaves out a
/// key of its first onset unless that saves a stroke over the rules' own reading, so a
/// weakened `G` (`TPWRAF`) or a stripped onset (`RAB`) cannot creep back. Short forms (`shell`,
/// `ends`, the vowel-less skeleton) and the inflections of an exempt root are not held to it, and
/// neither are the pinned words.
#[test]
fn no_primary_outline_past_the_exemption_rank_drops_an_edge_consonant() {
    let setup = crate::dictionary_fixture::setup();
    let briefs = Briefs::built_in().unwrap();
    let exempt = setup.rules.faithful_exempt_rank();
    let mut broken = Vec::new();
    let mut checked = 0;
    for entry in &crate::dictionary_fixture::generated().entries {
        if entry.origin != Origin::Brief {
            continue;
        }
        let rank = setup.rank.get(&entry.word).copied().unwrap_or(usize::MAX);
        if rank < exempt || briefs.word.contains_key(&entry.word) {
            continue;
        }
        let Some(prons) = setup.pron.get(entry.word.as_str()) else { continue };
        checked += 1;
        // an unfaithful outline is allowed only where it saves a whole stroke on the rules' own reading
        let reading = prons.iter().filter_map(|sounds| setup.rules.outline(&entry.word, sounds)).map(|o| o.len()).min();
        if !setup.rules.faithful_any(&entry.outline, &entry.word, prons) && reading.is_some_and(|n| entry.outline.len() >= n) {
            broken.push(format!("{} {} {:?}", entry.word, render_outline(&entry.outline), entry.origin));
        }
    }
    assert!(checked > 5_000, "only {checked} entries checked");
    let mut by: std::collections::BTreeMap<String, Vec<&String>> = std::collections::BTreeMap::new();
    for b in &broken {
        by.entry(b.rsplit(' ').next().unwrap().to_owned()).or_default().push(b);
    }
    let summary: Vec<String> = by.iter().map(|(k, v)| format!("{k}: {} e.g. {:?}", v.len(), &v[..v.len().min(12)])).collect();
    assert!(broken.is_empty(), "{} unfaithful primaries:\n{}", broken.len(), summary.join("\n"));
}

/// The words the complaint was about, in the built dictionary.
#[test]
fn graph_and_its_cluster_family_are_written_with_every_key_of_their_sounds() {
    let cheapest = crate::dictionary_fixture::cheapest();
    
    for word in ["graph", "grab", "green", "grow", "grass", "glass", "blue", "black", "plan", "judge", "job"] {
        let setup = crate::dictionary_fixture::setup();
        assert!(
            setup.rules.faithful_any(cheapest[word], word, &setup.pron[word]),
            "{word} is {}",
            render_outline(cheapest[word])
        );
    }
}

/// An unfaithful gold outline is a candidate only if it saves a stroke.
#[test]
fn a_dropped_sound_must_buy_a_stroke() {
    let setup = crate::dictionary_fixture::setup();
    let briefs = Briefs::built_in().unwrap();
    let pool = |word: &str| enumerate_candidates(setup, &briefs, word, &super::tests::TEXTS);
    // one stroke either way: the shorter spelling of the G, J or cluster buys nothing and loses
    for word in ["graph", "grab", "green", "glass", "blue", "plan"] {
        let faithful_only = pool(word)
            .iter()
            .filter(|c| c.origin == Origin::Brief)
            .all(|c| setup.rules.faithful_any(&c.outline, word, &setup.pron[word]));
        assert!(faithful_only, "{word} has an unfaithful gold candidate in its pool");
    }
    // a word whose faithful reading takes two or more strokes may drop its first sound for one
    for (word, outline) in [("request", "KW"), ("describe", "SKREU")] {
        let one = parse_outline(outline).unwrap();
        assert!(
            pool(word).iter().any(|c| c.outline == one && !setup.rules.faithful_any(&c.outline, word, &setup.pron[word])),
            "{word} should still offer the stroke-saving {outline}"
        );
    }
    // a word among the commonest keeps every gold outline
    let the = pool("get");
    assert!(the.iter().any(|c| render_outline(&c.outline) == "ET"));
}

/// The two words the typist named: `got` keys the whole G, `find` keys the F and the D but not the N.
#[test]
fn got_and_find_are_the_typists_own_outlines() {
    let cheapest = crate::dictionary_fixture::cheapest();
    assert_eq!(render_outline(cheapest["got"]), "TKPWOT");
    assert_eq!(render_outline(cheapest["find"]), "TPAOEUD");
}
