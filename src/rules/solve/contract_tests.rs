//! The solver contract (`site/reference/solver-contract.json`): six guarantees that hold however
//! the solver's reading of a miss changes. Each test names its guarantee, and a failure says which
//! one broke and on what outline.
//!
//! Real strokes go through the real solver and the real built dictionary: the fixture builds the
//! whole dictionary as `build` does (every role, the name lane), reads the files back as keymux's
//! loader would (the reserved tables included, in keymux's priority order), and packs the solver
//! blob. `Layer` is keymux's lookup chain over them: each file in priority order, the solver last,
//! asked only about a window that is one word (every stroke after the first carries the join
//! key). Sample sizes are the constants below; `cargo test --release -- --ignored contract_full`
//! runs every entry and a far larger solver sample.

use super::core::{Model, Scratch};
use super::{Lexicon, pack, stems};
use crate::analysis::Setup;
use crate::generate::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// Every `N`th deployed entry is checked as an entry (guarantees 1 and 2).
const ENTRY_EVERY: usize = 25;
/// Every `N`th backup outline is read by the solver and compared with its entry (guarantees 1, 4).
const BACKUP_EVERY: usize = 400;
/// Stems used to build runs around a suffix chord (guarantee 3).
const STEMS: usize = 300;
/// Outlines read repeatedly for determinism (guarantee 4), and how often each is read.
const DETERMINISM_SAMPLE: usize = 600;
const REPEATS: usize = 10;
/// `--ignored` full run: every `N`th (1: all).
const FULL_ENTRY_EVERY: usize = 1;
const FULL_BACKUP_EVERY: usize = 20;
const FULL_STEMS: usize = 3_000;
const FULL_DETERMINISM_SAMPLE: usize = 6_000;

/// What `file` is called and what it holds, in keymux's loading priority: a numbered name before
/// any plain one, lowest number first, then alphabetical ignoring case (`keymux/src/steno/source.rs`).
fn priority(name: &str) -> (u32, String) {
    let number = name.split('-').next().and_then(|n| n.parse().ok()).unwrap_or(u32::MAX);
    (number, name.to_lowercase())
}

/// The dictionary files `deploy` ships (the backups are not: the solver reads them), as keymux
/// loads them: first file first, an outline answered by the earliest file that has it.
struct Files {
    in_order: Vec<(String, HashMap<String, String>)>,
    backups: BTreeMap<String, String>,
    longest: usize,
}

struct Fixture {
    files: Files,
    model: Model,
    blob: Vec<u8>,
    entries: &'static [Entry],
    setup: &'static Setup,
    names: Vec<crate::names::Name>,
}

fn read_map(text: &str) -> HashMap<String, String> {
    serde_json::from_str(text).expect("a dictionary file is a JSON object of outline to text")
}

static FIXTURE: LazyLock<Fixture> = LazyLock::new(|| {
    let setup = crate::dictionary_fixture::setup();
    let generated = crate::dictionary_fixture::generated();
    let dir = crate::test_dir::TestDir::new("lyk-solver-contract");
    crate::output::write_dictionary(generated, &dir).expect("the dictionary writes");
    let names = crate::names::collect(setup);
    let lane = crate::names::generate(setup, &generated.entries, &names).expect("the name lane builds");
    crate::names::write(&lane, &dir).expect("the name lane writes");

    let mut raw: Vec<(String, HashMap<String, String>)> = Vec::new();
    for role in crate::output::Role::ALL {
        let text = std::fs::read_to_string(dir.join(role.file_name())).expect("a role's file was written");
        raw.push((role.file_name().to_owned(), read_map(&text)));
    }
    for name in [crate::names::FILE, crate::names::BORROWED_FILE] {
        let text = std::fs::read_to_string(dir.join(name)).expect("a name file was written");
        raw.push((name.to_owned(), read_map(&text)));
    }
    drop(dir);
    for (name, text) in [
        ("01-lyk-commands.json", include_str!("../../../rules/reserved/commands.json")),
        ("03-lyk-punctuation.json", include_str!("../../../rules/reserved/punctuation.json")),
        ("04-lyk-emilys-modifiers.json", include_str!("../../../rules/reserved/emilys-modifiers.json")),
    ] {
        raw.push((name.to_owned(), read_map(text)));
    }
    // The backups are `out/lyk`'s test set, not shipped: held apart from the chain
    let at = raw.iter().position(|(n, _)| n == crate::output::Role::Backups.file_name()).expect("the backups file");
    let backups: BTreeMap<String, String> = raw.remove(at).1.into_iter().collect();
    raw.sort_by_key(|(name, _)| priority(name));
    let longest = raw.iter().flat_map(|(_, m)| m.keys()).map(|o| o.split('/').count()).max().unwrap_or(1);

    let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: 100_000 };
    let blob = pack(&setup.rules, &lexicon, &stems(&generated.entries), Some(&names));
    let model = Model::load(&blob).expect("the packed blob loads");
    Fixture { files: Files { in_order: raw, backups, longest }, model, blob, entries: &generated.entries, setup, names }
});

/// Where a stroke window's text came from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum From {
    File(String),
    Solver,
}

/// One translated window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Typed {
    outline: String,
    text: String,
    from: From,
}

/// Whether keymux would ask a plugin about these strokes: one stroke, or a run whose every stroke
/// after the first continues the word before it.
fn one_word(outline: &str) -> bool {
    outline.split('/').skip(1).all(|s| s.starts_with('+'))
}

impl Fixture {
    /// keymux's chain for one exact outline: the first file with it, else (a one-word window) the
    /// solver.
    fn layer(&self, outline: &str, sc: &mut Scratch) -> Option<Typed> {
        for (name, map) in &self.files.in_order {
            if let Some(text) = map.get(outline) {
                return Some(Typed { outline: outline.to_owned(), text: text.clone(), from: From::File(name.clone()) });
            }
        }
        if one_word(outline) {
            let text = self.model.answer_with(sc, outline)?;
            return Some(Typed { outline: outline.to_owned(), text, from: From::Solver });
        }
        None
    }

    /// A stream of strokes translated as keymux does: from the left, the longest window the chain
    /// answers (a window past the longest dictionary key only where it is one word), then on.
    fn stream(&self, strokes: &[&str], sc: &mut Scratch) -> Vec<Typed> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < strokes.len() {
            let run = 1 + strokes[at + 1..].iter().take_while(|s| s.starts_with('+')).count();
            let widest = self.files.longest.max(run).min(strokes.len() - at);
            let found = (1..=widest).rev().find_map(|len| self.layer(&strokes[at..at + len].join("/"), sc));
            if let Some(typed) = found {
                at += typed.outline.split('/').count();
                out.push(typed);
            } else {
                // Nothing reads it: keymux types the stroke's keys raw
                out.push(Typed { outline: strokes[at].to_owned(), text: strokes[at].to_owned(), from: From::Solver });
                at += 1;
            }
        }
        out
    }

    /// Every deployed entry as (outline, first file with it, its text), outlines sorted.
    fn entries_sorted(&self) -> Vec<(&str, &str, &str)> {
        let mut seen: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
        for (name, map) in &self.files.in_order {
            for (outline, text) in map {
                seen.entry(outline.as_str()).or_insert((name.as_str(), text.as_str()));
            }
        }
        seen.into_iter().map(|(o, (f, t))| (o, f, t)).collect()
    }
}

fn every<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    items.iter().step_by(n.max(1)).cloned().collect()
}

/// A fixed shuffle (no dependence on anything but the length and the seed).
fn shuffled<T>(items: &mut [T], seed: u64) {
    let mut state = seed | 1;
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = usize::try_from(state % (i as u64 + 1)).unwrap_or(0);
        items.swap(i, j);
    }
}

// --- Guarantee 1: a defined entry always wins ---

fn entries_win(every_nth: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let all = fx.entries_sorted();
    let sample = every(&all, every_nth);
    assert!(sample.len() > 100, "guarantee 1 (entry wins): the sample is only {} entries", sample.len());
    let mut bad = Vec::new();
    for (outline, file, text) in &sample {
        let strokes: Vec<&str> = outline.split('/').collect();
        let typed = fx.stream(&strokes, &mut sc);
        let ok = typed.len() == 1 && typed[0].text == *text && typed[0].from == From::File((*file).to_owned());
        if !ok {
            bad.push(format!("{outline}: the entry is {text:?} in {file}, typed {typed:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "guarantee 1 (a defined entry always wins) broken on {} of {} entries, e.g.\n{}",
        bad.len(),
        sample.len(),
        bad.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Two files hold one outline with different text: the earlier file's wins by priority, but a
/// defined outline then has two definitions, which the generator should never write.
fn one_definition_per_outline() {
    let fx = &*FIXTURE;
    let mut first: HashMap<&str, (&str, &str)> = HashMap::new();
    let mut clashes = Vec::new();
    for (name, map) in &fx.files.in_order {
        for (outline, text) in map {
            match first.get(outline.as_str()) {
                Some((file, held)) if held != text => clashes.push(format!("{outline}: {file} says {held:?}, {name} says {text:?}")),
                Some(_) => {}
                None => {
                    first.insert(outline, (name, text));
                }
            }
        }
    }
    clashes.sort();
    assert!(
        clashes.is_empty(),
        "guarantee 1 (a defined entry always wins): {} outlines have two different definitions in the deployed files, e.g.\n{}",
        clashes.len(),
        clashes.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// A starred suffix chord is a literal attach, so an entry that is a known outline plus one as
/// continuation strokes would type something other than what a learner builds from the pieces.
/// The generator never writes one: no deployed outline has a starred chord after its first stroke
/// unless the outline is that chord's own entry.
fn no_entry_continues_with_a_starred_chord() {
    let fx = &*FIXTURE;
    let chords: Vec<Vec<String>> =
        hard_suffixes(fx).into_iter().map(|(chord, _)| chord.split('/').map(str::to_owned).collect()).collect();
    assert!(chords.len() > 20, "guarantee 1: {} starred suffix chords is too few", chords.len());
    let mut bad = Vec::new();
    for (outline, file, text) in fx.entries_sorted() {
        if file.contains("suffixes") {
            continue;
        }
        let strokes: Vec<&str> = outline.split('/').collect();
        let rest = strokes.get(1..).unwrap_or_default();
        if chords.iter().any(|chord| rest.windows(chord.len()).any(|w| w.iter().zip(chord).all(|(a, b)| a == b))) {
            bad.push(format!("{outline} ({file}): {text:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "guarantee 1 (a defined entry always wins): {} entries use a starred suffix chord as continuation strokes, e.g.\n{}",
        bad.len(),
        bad.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn contract_1_a_defined_entry_always_wins() {
    entries_win(ENTRY_EVERY);
    one_definition_per_outline();
    no_entry_continues_with_a_starred_chord();
}

#[test]
#[ignore = "the full run: every deployed entry"]
fn contract_full_1_a_defined_entry_always_wins() {
    entries_win(FULL_ENTRY_EVERY);
}

/// Were the solver asked about a word's own outline (it never is: the dictionary answers first), it
/// would type that word. So a run that grows from an entry, or a window keymux tries, never
/// contradicts the dictionary.
///
/// `allowed` per 100,000 entries read otherwise: a rare word's own outline the beam drops, a capital
/// the solver lowercases (`I'd`); the sample has none.
fn solver_agrees_with_entries(every_nth: usize, allowed: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let chords: Vec<String> = hard_suffixes(fx).into_iter().map(|(chord, _)| chord).collect();
    let mut words: Vec<(&str, &str)> = Vec::new();
    for (name, map) in fx.files.in_order.iter().rev() {
        if ["words", "short", "alternates", "extras"].iter().any(|k| name.contains(k)) {
            // An outline with a lone `+` (the lock) or `+*` (the late star) stroke, or a starred suffix chord,
            // in it is read by the solver with that stroke as the lock, the star or the suffix: what the entry spells there is the
            // dictionary's alone (the entry wins: guarantee 1)
            words.extend(
                map.iter()
                    .filter(|(o, t)| {
                        let padded = format!("/{o}/");
                        !t.contains('{') && !t.contains(' ') && !padded.contains("/+*/") && !padded.contains("/+/") && !chords.iter().any(|c| padded.contains(&format!("/{c}/")))
                    })
                    .map(|(o, t)| (o.as_str(), t.as_str())),
            );
        }
    }
    words.sort_unstable();
    let sample = every(&words, every_nth);
    let mut bad = Vec::new();
    for (outline, text) in &sample {
        let got = fx.model.answer_with(&mut sc, outline);
        if got.as_deref() != Some(*text) {
            bad.push(format!("{outline}: the entry is {text:?}, the solver reads {got:?}"));
        }
    }
    assert!(
        bad.len() * 100_000 <= allowed * sample.len(),
        "guarantee 1 (the solver never contradicts an entry): {} of {} word entries read differently (at most {allowed} per 100,000), e.g.\n{}",
        bad.len(),
        sample.len(),
        bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn contract_1_the_solver_never_contradicts_an_entry() {
    solver_agrees_with_entries(ENTRY_EVERY * 4, 0);
}

#[test]
#[ignore = "the full run: every word entry"]
fn contract_full_1_the_solver_never_contradicts_an_entry() {
    solver_agrees_with_entries(FULL_ENTRY_EVERY, 20);
}

// --- Guarantee 2: the solver reads only what has no entry ---

fn solver_only_reads_misses(every_nth: usize, backup_every: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let held: std::collections::HashSet<&str> = fx.files.in_order.iter().flat_map(|(_, m)| m.keys().map(String::as_str)).collect();

    // A one-word window with no entry is the solver's, and nothing else answers it
    let misses: Vec<&String> = fx.files.backups.keys().filter(|o| !held.contains(o.as_str())).collect();
    for outline in every(&misses, backup_every) {
        let typed = fx.layer(outline, &mut sc).unwrap_or_else(|| panic!("guarantee 2: the solver gave no reading for the miss {outline}"));
        assert_eq!(typed.from, From::Solver, "guarantee 2 (the solver reads only misses): {outline} is no entry, so only the solver reads it");
    }

    // A backup that collides with an entry is the entry (the generator skips these, so none should be left)
    let both: Vec<(&String, &String)> = fx.files.backups.iter().filter(|(o, _)| held.contains(o.as_str())).collect();
    for (outline, _) in &both {
        let typed = fx.layer(outline, &mut sc).expect("an entry answers");
        assert!(matches!(typed.from, From::File(_)), "guarantee 2: backup outline {outline} is also an entry but {typed:?} answered");
    }

    // Strokes with no join key between them are separate words: never one window for the solver.
    // A window of two entries that is no entry is not read at all, and each stays its own entry.
    let singles: Vec<(&str, &str, &str)> = fx
        .entries_sorted()
        .into_iter()
        .filter(|(o, file, t)| !o.contains('/') && !o.starts_with('+') && file.starts_with("02-lyk-words") && t.chars().all(|c| c.is_ascii_lowercase()))
        .collect();
    let picked = every(&singles, (singles.len() / 400).max(1) * every_nth.div_ceil(ENTRY_EVERY));
    for pair in picked.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let joined = format!("{}/{}", a.0, b.0);
        if held.contains(joined.as_str()) {
            continue;
        }
        assert!(fx.layer(&joined, &mut sc).is_none(), "guarantee 2: the solver was asked about two separate words {joined}");
        let typed = fx.stream(&[a.0, b.0], &mut sc);
        let texts: Vec<&str> = typed.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec![a.2, b.2], "guarantee 2: two words struck one after the other are each their own entry ({joined})");
        assert!(typed.iter().all(|t| matches!(t.from, From::File(_))), "guarantee 2: {joined} was not read by the dictionary alone");
    }
}

#[test]
fn contract_2_the_solver_reads_only_what_has_no_entry() {
    solver_only_reads_misses(ENTRY_EVERY, BACKUP_EVERY);
}

#[test]
#[ignore = "the full run"]
fn contract_full_2_the_solver_reads_only_what_has_no_entry() {
    solver_only_reads_misses(FULL_ENTRY_EVERY, FULL_BACKUP_EVERY);
}

// --- Guarantee 3: exact means exact ---

/// Whole-word outlines (no join key) whose text is a plain lowercase word, from the files a word's
/// own outline lives in, spread over the sorted list.
fn stem_words(fx: &Fixture, count: usize) -> Vec<(String, String)> {
    let mut all: BTreeMap<&str, &str> = BTreeMap::new();
    for (name, map) in fx.files.in_order.iter().rev() {
        if ["words", "short", "alternates", "extras"].iter().any(|k| name.contains(k)) {
            for (outline, text) in map {
                if !outline.starts_with('+') && !outline.contains("/+") && text.len() > 2 && text.chars().all(|c| c.is_ascii_lowercase()) {
                    all.insert(outline, text);
                }
            }
        }
    }
    let all: Vec<(&str, &str)> = all.into_iter().collect();
    every(&all, (all.len() / count.max(1)).max(1)).into_iter().map(|(o, t)| (o.to_owned(), t.to_owned())).collect()
}

/// The defined starred suffix chords: outline (a `+*` run of one or two strokes) and its ending.
fn hard_suffixes(fx: &Fixture) -> Vec<(String, String)> {
    let (_, map) = fx.files.in_order.iter().find(|(n, _)| n.contains("suffixes")).expect("the suffix file");
    let mut all: Vec<(String, String)> = map
        .iter()
        .map(|(o, t)| (o.clone(), t.strip_prefix("{^}").unwrap_or_else(|| panic!("a suffix entry is an exact attach: {o} is {t}")).to_owned()))
        .collect();
    all.sort();
    all
}

/// `stem_misses_allowed` is a share (per 100,000 runs) of runs whose starred chords are literal but whose
/// stem was read other than as its entry: a rare stem the beam drops before the end (the full run
/// has a few; the sample has none).
fn exact_starred_chords(stem_count: usize, suffixes_each: usize, stem_misses_allowed: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let stems = stem_words(fx, stem_count);
    let suffixes = hard_suffixes(fx);
    assert!(stems.len() > 100 && suffixes.len() > 20, "guarantee 3: {} stems and {} suffix chords is too few", stems.len(), suffixes.len());
    let mut bad = Vec::new();
    let mut stem_misses = Vec::new();
    let mut checked = 0;
    for (n, (stem_outline, stem)) in stems.iter().enumerate() {
        for k in 0..suffixes_each.min(suffixes.len()) {
            let (chord, ending) = &suffixes[(n * 7 + k * 11) % suffixes.len()];
            let (second, ending2) = &suffixes[(n * 5 + k * 3 + 1) % suffixes.len()];
            // The chord right after the stem, then in the middle of a run (another hard chord
            // after it, a plain chord after that)
            let cases = [
                (format!("{stem_outline}/{chord}"), format!("{stem}{ending}"), ending.clone()),
                (format!("{stem_outline}/{chord}/{second}"), format!("{stem}{ending}{ending2}"), format!("{ending}{ending2}")),
                (format!("{stem_outline}/{chord}/+-L"), format!("{stem}{ending}"), String::new()),
            ];
            for (outline, expected, literal) in cases {
                // No entry is a known outline plus a starred chord (guarantee 1), so none of these runs is one
                checked += 1;
                let typed = fx.layer(&outline, &mut sc).expect("a one-word run gets a reading");
                // The starred chords' own text is always literal, last in the run (a plain chord after
                // the starred one is spelled at its seam: able + ly is ably)
                if !typed.text.ends_with(&literal) {
                    bad.push(format!("{outline}: the starred chords should type {literal:?} last, typed {:?}", typed.text));
                    continue;
                }
                let kept: String = expected.chars().take(expected.chars().count() - 1).collect();
                let exact = if outline.ends_with("+-L") { typed.text.starts_with(&kept) } else { typed.text == expected };
                if !exact {
                    stem_misses.push(format!("{outline}: {stem} + {ending} should type {expected:?}, typed {:?}", typed.text));
                }
            }
        }
    }
    assert!(
        stem_misses.len() * 100_000 <= stem_misses_allowed * checked,
        "guarantee 3 (a known outline then a starred chord types its entry and the ending) broken on {} of {checked} runs (at most {stem_misses_allowed} per 100,000), e.g.\n{}",
        stem_misses.len(),
        stem_misses.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(
        bad.is_empty(),
        "guarantee 3 (a starred suffix chord is exact) broken on {} of {checked} runs, e.g.\n{}",
        bad.len(),
        bad.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Every reserved or defined fixed point, struck between two runs: the runs are read as they are
/// alone, and the fixed point types exactly its entry.
fn fixed_points_between_runs(every_nth: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let stems = stem_words(fx, 40);
    let suffix = &hard_suffixes(fx)[0];
    let run_a = format!("{}/{}", stems[3].0, suffix.0);
    let run_b = format!("{}/+-G", stems[17].0);
    let alone_a = fx.layer(&run_a, &mut sc).expect("a reading").text;
    let alone_b = fx.layer(&run_b, &mut sc).expect("a reading").text;
    let mut checked = 0;
    for (name, map) in &fx.files.in_order {
        // Fingerspelled letters, literals, punctuation, controls, modifier chords; not words
        if !["commands", "punctuation", "emilys"].iter().any(|k| name.contains(k)) {
            continue;
        }
        let mut outlines: Vec<&String> = map.keys().filter(|o| !o.starts_with('+')).collect();
        outlines.sort();
        for outline in every(&outlines, every_nth.max(1)) {
            let mut strokes: Vec<&str> = run_a.split('/').collect();
            let fixed: Vec<&str> = outline.split('/').collect();
            strokes.extend(&fixed);
            strokes.extend(run_b.split('/'));
            let typed = fx.stream(&strokes, &mut sc);
            checked += 1;
            let texts: Vec<&str> = typed.iter().map(|t| t.text.as_str()).collect();
            let ok = texts.first() == Some(&alone_a.as_str())
                && texts.last() == Some(&alone_b.as_str())
                && typed[0].from == From::Solver
                && typed[1..typed.len() - 1].iter().all(|t| matches!(t.from, From::File(_)))
                && typed[1..typed.len() - 1].iter().map(|t| t.outline.as_str()).collect::<Vec<_>>().join("/") == **outline;
            assert!(
                ok,
                "guarantee 3 (fingerspelling, literals, controls and punctuation are fixed points): {run_a} {outline} {run_b} typed {texts:?}, \
                 the runs alone are {alone_a:?} and {alone_b:?} and the entry is {:?}",
                map[outline.as_str()]
            );
        }
    }
    assert!(checked > 50, "guarantee 3: only {checked} fixed points were tried");
}

#[test]
fn contract_3_exact_means_exact() {
    // A two-stroke starred chord is one chord, even where its first stroke is a chord of its own:
    // `+*R/+AOE` is *ary*, not `+*R` *er* and a stroke after it
    for (outline, typed) in [("PWREFRP/+*R/+AOE", "brewary"), ("PWREFRP/+*R", "brewer"), ("TAOEUP/+*G", "typeing"), ("A*R/+*LT", "arrlet")] {
        assert_eq!(FIXTURE.model.answer(outline).as_deref(), Some(typed), "guarantee 3: {outline}");
    }
    exact_starred_chords(STEMS, 4, 0);
    fixed_points_between_runs(7);
}

#[test]
#[ignore = "the full run: 3,000 stems against every starred chord, every reserved entry"]
fn contract_full_3_exact_means_exact() {
    exact_starred_chords(FULL_STEMS, 41, 10);
    fixed_points_between_runs(1);
}

// --- Guarantee 4: deterministic ---

/// Real runs of every kind the solver reads: backups (words, whole outlines with continuations),
/// whole entries, starred chords after stems, plain chords after stems.
fn mixed_sample(fx: &Fixture, count: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let backups: Vec<&String> = fx.files.backups.keys().collect();
    out.extend(every(&backups, (backups.len() / count.max(4) * 2).max(1)).into_iter().cloned());
    let entries = fx.entries_sorted();
    let multi: Vec<&str> = entries.iter().map(|e| e.0).filter(|o| o.contains("/+") && !o.starts_with('+')).collect();
    out.extend(every(&multi, (multi.len() / (count / 4).max(1)).max(1)).into_iter().map(str::to_owned));
    let suffixes = hard_suffixes(fx);
    for (n, (stem, _)) in stem_words(fx, count / 4).iter().enumerate() {
        out.push(format!("{stem}/{}", suffixes[n % suffixes.len()].0));
        out.push(format!("{stem}/+-G/+{}", ["-L", "-S", "-D", "-PBT"][n % 4]));
    }
    out
}

fn deterministic(count: usize) {
    let fx = &*FIXTURE;
    let sample = mixed_sample(fx, count);
    // The reference: each run read alone with working memory of its own
    let base: Vec<Option<String>> = sample.iter().map(|o| fx.model.answer_with(&mut fx.model.scratch(), o)).collect();
    let check = |how: String, got: Vec<(usize, Option<String>)>| {
        let wrong: Vec<String> =
            got.iter().filter(|(i, text)| *text != base[*i]).map(|(i, text)| format!("{}: {:?} then {text:?}", sample[*i], base[*i])).collect();
        assert!(
            wrong.is_empty(),
            "guarantee 4 (the same strokes always type the same text) broken, {how}: {} of {} runs differ, e.g.\n{}",
            wrong.len(),
            sample.len(),
            wrong.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
        );
    };
    for pass in 1..REPEATS {
        let mut order: Vec<usize> = (0..sample.len()).collect();
        shuffled(&mut order, 0x9e37_79b9 * pass as u64);
        // A pass in a new order through one working memory (what was read before is kept, as keymux's
        // stroke-by-stroke asking keeps it), and a pass through the plugin's own shared one
        let got = if pass % 2 == 0 {
            let mut sc = fx.model.scratch();
            order.iter().map(|&i| (i, fx.model.answer_with(&mut sc, &sample[i]))).collect()
        } else {
            order.iter().map(|&i| (i, fx.model.answer(&sample[i]))).collect()
        };
        check(format!("on repeat {pass}, in another order"), got);
    }
    // Each prefix of a run read in turn first (keymux asks again after every stroke), then the run
    let mut sc = fx.model.scratch();
    let got = (0..sample.len())
        .map(|i| {
            let strokes: Vec<&str> = sample[i].split('/').collect();
            for n in 1..strokes.len() {
                let _ = fx.model.answer_with(&mut sc, &strokes[..n].join("/"));
            }
            (i, fx.model.answer_with(&mut sc, &sample[i]))
        })
        .collect();
    check("after each of its prefixes was read".to_owned(), got);
    // Threads, each with its own working memory, each in its own order
    std::thread::scope(|scope| {
        // All four are started before any is joined
        #[allow(clippy::needless_collect)]
        let jobs: Vec<_> = (0..4)
            .map(|t| {
                let sample = &sample;
                scope.spawn(move || {
                    let mut order: Vec<usize> = (0..sample.len()).collect();
                    shuffled(&mut order, 31 + t);
                    let mut sc = fx.model.scratch();
                    order.into_iter().map(|i| (i, fx.model.answer_with(&mut sc, &sample[i]))).collect::<Vec<_>>()
                })
            })
            .collect();
        for (t, job) in jobs.into_iter().enumerate() {
            check(format!("on thread {t}"), job.join().expect("a thread reads"));
        }
    });
}

/// What is typed before a run only breaks ties between readings of one kind: never an entry, an
/// exact known outline or a starred chord. (The one documented dependence on earlier text.)
fn context_never_moves_what_is_fixed(count: usize) {
    let fx = &*FIXTURE;
    let mut sc = fx.model.scratch();
    let sample = mixed_sample(fx, count);
    let suffixes = hard_suffixes(fx);
    let stems = stem_words(fx, count / 8);
    let entries: std::collections::HashSet<&str> = fx.entries_sorted().into_iter().map(|e| e.0).collect();
    // The solver's known outlines: the stems it is packed with (the words, short, alternates and
    // extras entries), not every deployed outline. A phrase's (`UP`, *you up*), a name's or a
    // command's outline is no stem: a run headed by one is read from sounds, so the word before may
    // move it within its kind (`UP/+*L` is *apply* alone and *uply* after *the*, a tie at one cost)
    let known: std::collections::HashSet<String> = super::stems(fx.entries).iter().map(|(o, _)| crate::stroke::render_outline(o)).collect();
    let mut by_sounds: Vec<String> = Vec::new();
    let fixed: Vec<String> = stems
        .iter()
        .enumerate()
        .map(|(n, (stem, _))| format!("{stem}/{}", suffixes[n % suffixes.len()].0))
        // A known outline and a starred chord that together are not an entry: an entry never reaches
        // the solver, so only a run that is not one can show the context moving a fixed reading
        .chain(
            fx.entries_sorted()
                .into_iter()
                .map(|e| e.0)
                .filter(|o| o.contains("/+") && !o.starts_with('+'))
                .step_by(97)
                .filter_map(|outline| {
                    let head = outline.rsplit_once("/+")?.0;
                    let run = format!("{head}/{}", suffixes[outline.len() % suffixes.len()].0);
                    if entries.contains(run.as_str()) {
                        return None;
                    }
                    // A known outline: a head that is read from sounds is the context's to choose,
                    // within its kind (checked below)
                    if !known.contains(head) {
                        by_sounds.push(run);
                        return None;
                    }
                    Some(run)
                }),
        )
        .collect();
    for context in ["", "the", "to be or not to", "of the", "a"] {
        for outline in &sample {
            let again = fx.model.answer_in(&mut sc, outline, context, false);
            if context.is_empty() {
                assert_eq!(again, fx.model.answer_with(&mut sc, outline), "guarantee 4: an empty context changed {outline}");
            }
        }
        for outline in &fixed {
            let bare = fx.model.answer_with(&mut sc, outline);
            let after = fx.model.answer_in(&mut sc, outline, context, false);
            assert_eq!(after, bare, "guarantee 4 (only a tie is ever moved by the word before): {outline} after {context:?}");
        }
        // A run read from sounds may change, but never across a kind: the word before never makes
        // a reading by sounds a fixed one, nor a fixed one a reading by sounds
        for outline in by_sounds.iter().chain(&sample) {
            let Some(run) = outline.split('/').map(super::core::parse_stroke).collect::<Option<Vec<u32>>>() else { continue };
            let mut kind = |context: &str| fx.model.solve_in(&mut sc, &run, 1, context, false).first().map(|r| r.exact);
            let (bare, after) = (kind(""), kind(context));
            assert_eq!(after, bare, "guarantee 4 (the word before only reorders within a kind): {outline} after {context:?}, exact {bare:?} then {after:?}");
        }
    }
}

/// The blob is built the same from the same dictionary however its inputs are ordered: the same
/// bytes, so the same plugin on every machine and every build.
fn blob_is_reproducible() {
    let fx = &*FIXTURE;
    let setup = fx.setup;
    let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: 100_000 };
    let mut stems = stems(fx.entries);
    shuffled(&mut stems, 7);
    // (The names stay in their order: it is their rank.)
    let again = pack(&setup.rules, &lexicon, &stems, Some(&fx.names));
    assert!(
        again == fx.blob,
        "guarantee 4 (determinism): packing the solver blob from the same dictionary with its inputs in another order gave different bytes ({} then {})",
        fx.blob.len(),
        again.len()
    );
}

#[test]
fn contract_4_the_same_strokes_always_type_the_same_text() {
    deterministic(DETERMINISM_SAMPLE);
    context_never_moves_what_is_fixed(DETERMINISM_SAMPLE / 6);
    blob_is_reproducible();
}

#[test]
#[ignore = "the full run"]
fn contract_full_4_the_same_strokes_always_type_the_same_text() {
    deterministic(FULL_DETERMINISM_SAMPLE);
    context_never_moves_what_is_fixed(FULL_DETERMINISM_SAMPLE / 6);
}

