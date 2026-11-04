//! Per-word views: how the rules write a word step by step (`explain`), the commonest words they
//! get wrong (`mismatches`), and which keys the gold dictionaries give each group of sounds
//! (`learn`).

use super::Setup;
use crate::stroke::{Stroke, render_outline};
use std::collections::HashMap;

/// How the rules write a word, syllable by syllable, next to what Plover and Lapwing write, and
/// next to the real, settled dictionary's own answer: `ours` below is the rules alone, in
/// isolation, for seeing how the theory derives a word step by step - it is deliberately not the
/// deployed outline, which also accounts for briefs, collisions, and the backup-entry pass (see
/// `deployed`, printed separately, for that; `dictionary` is `generate::cheapest` over a real
/// `generate::generate` run, built once by the caller).
pub fn explain(setup: &Setup, dictionary: &HashMap<&str, &[Stroke]>, word: &str) {
    let word = word.to_lowercase();
    let Some(variants) = setup.pron.get(&word) else {
        // No pronunciation at all for the rules to run from: spelling mode, letter by letter,
        // is the real fallback here, not a plain miss - see `Rules::spell_word`.
        match setup.rules.spell_word(&word) {
            Some(outline) => println!(
                "{word}: no pronunciation in CMUdict, spelled out: {}",
                render_outline(&outline)
            ),
            None => println!("{word}: no pronunciation in CMUdict, and not spellable (a-z only)"),
        }
        return;
    };
    for sounds in variants {
        println!("{word}  /{}/", sounds.join(" "));
        match setup.rules.syllables(&setup.rules.label(&word, sounds)) {
            None => println!("  no vowel"),
            Some(syllables) => {
                for syllable in &syllables {
                    let stroke = setup.rules.syllable_stroke(syllable);
                    println!(
                        "  [{}] {} [{}]  ->  {}",
                        syllable.onset.join(" "),
                        syllable.vowel,
                        syllable.coda.join(" "),
                        stroke.map_or_else(|| "cannot be written in one stroke".to_owned(), |s| s.to_string())
                    );
                }
            }
        }
    }
    let (plover, lapwing) = setup.gold.shortest(&word);
    println!("ours    {}", setup.candidates(&word).join("  "));
    println!(
        "deployed {}",
        dictionary
            .get(word.as_str())
            .map_or_else(|| "(not in the dictionary built here)".to_owned(), |o| render_outline(o))
    );
    for step in setup.rules.brevity() {
        let forms: Vec<String> = variants
            .iter()
            .flat_map(|sounds| setup.rules.shorten(*step, &word, sounds))
            .map(|o| render_outline(&o))
            .collect();
        if !forms.is_empty() {
            let vowel = if step.vowel { "" } else { " (no vowel)" };
            println!("  {}{vowel}: {}", step.step, forms.join("  "));
        }
    }
    println!(
        "plover  {}",
        setup
            .gold
            .plover
            .get(&word)
            .map_or(String::new(), |v| v.join("  "))
    );
    let _ = (plover, lapwing);
    println!(
        "lapwing {}",
        setup
            .gold
            .lapwing
            .get(&word)
            .map_or(String::new(), |v| v.join("  "))
    );
}

/// The most common words the rules get wrong, with what Plover and Lapwing write.
pub fn mismatches(
    setup: &Setup,
    syllables: usize,
    limit: usize,
    missing: bool,
    after: usize,
) {
    let mut shown = 0;
    for word in setup.words() {
        if setup.syllable_count(word) != syllables
            || setup.rank.get(word).copied().unwrap_or(usize::MAX) < after
        {
            continue;
        }
        let candidates = setup.candidates(word);
        let right = candidates.iter().any(|o| {
            let (p, l) = setup.gold.writes(word, o);
            p || l
        });
        if right || (missing && !candidates.is_empty()) {
            continue;
        }
        let (plover, lapwing) = setup.gold.shortest(word);
        let sounds = setup.pron[word]
            .first()
            .map(|s| s.join(" "))
            .unwrap_or_default();
        println!(
            "{word:<12} ours {:<18} plover {:<16} lapwing {:<16} {sounds}",
            candidates.first().map_or("-", String::as_str),
            plover.unwrap_or("-"),
            lapwing.unwrap_or("-"),
        );
        shown += 1;
        if shown == limit {
            break;
        }
    }
}

/// One group of sounds `learn` reports: how many words it covers, the sounds, and each way the
/// gold dictionaries write it with how many words do.
type Learned = (usize, String, Vec<(String, usize)>);

/// For one-syllable words that Plover and Lapwing write the same way in one stroke, which keys
/// each group of sounds went on. This is where the rules come from.
pub fn learn(setup: &Setup, min: usize, limit: usize) {
    type Counts = HashMap<String, HashMap<String, usize>>;
    let (mut onsets, mut rimes): (Counts, Counts) = (HashMap::new(), HashMap::new());
    for word in setup.words() {
        let (Some(plover), Some(lapwing)) = setup.gold.shortest(word) else {
            continue;
        };
        if plover != lapwing || plover.contains('/') {
            continue;
        }
        let Some(sounds) = setup.pron[word].first() else {
            continue;
        };
        let Some(syllables) = setup.rules.syllables(sounds) else {
            continue;
        };
        let [syllable] = syllables.as_slice() else {
            continue;
        };
        let Some(stroke) = Stroke::parse(plover) else {
            continue;
        };
        let left = Stroke(stroke.0 & Stroke::LEFT);
        let rest = Stroke(stroke.0 & !Stroke::LEFT);
        let base = syllable
            .vowel
            .trim_end_matches(|c: char| c.is_ascii_digit());
        let onset = syllable.onset.join(" ");
        let rime = std::iter::once(base)
            .chain(syllable.coda.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let shown = |s: Stroke| {
            if s.is_empty() {
                "(none)".to_owned()
            } else {
                s.to_string()
            }
        };
        *onsets
            .entry(onset)
            .or_default()
            .entry(shown(left))
            .or_default() += 1;
        *rimes
            .entry(rime)
            .or_default()
            .entry(shown(rest))
            .or_default() += 1;
    }
    for (title, counts) in [
        ("start of a syllable", onsets),
        ("vowel and the end of a syllable", rimes),
    ] {
        println!("== {title}: sounds -> keys (how many words)\n");
        let mut rows: Vec<Learned> = counts
            .into_iter()
            .map(|(sounds, keys)| {
                let mut keys: Vec<_> = keys.into_iter().collect();
                keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                (keys.iter().map(|k| k.1).sum(), sounds, keys)
            })
            .filter(|row| row.0 >= min)
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (total, sounds, keys) in rows.into_iter().take(limit) {
            let top: Vec<String> = keys
                .iter()
                .take(4)
                .map(|(k, n)| format!("{k} ({n})"))
                .collect();
            println!("{sounds:<14} {total:>5}  {}", top.join("  "));
        }
        println!();
    }
}
