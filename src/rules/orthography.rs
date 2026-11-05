//! Every way to write a word as joined strokes, from how it is *spelled*.
//!
//! The lattice over a word's pronunciation (`lattice.rs`) can only find strokes for sounds the
//! dictionary says the word has. A typist who spells a word out, a syllable at a time, as it looks
//! (`con` `tem` `pla` `tion`), says the letters, not the phonemes, and the two disagree wherever
//! English does: the *o* of *con* that `CMUdict` hears as "ah", the *e* of *tem* it hears as a
//! schwa. So the letters get a lattice of their own. The word is read as a pseudo-pronunciation
//! (each letter or letter pair a sound, a vowel letter offered short, long and as a weak
//! droppable vowel, the letters that can be read two ways, like *c* and *g*, read both) and the
//! same lattice runs over it, with the same stroke building, pricing and quotas. See
//! `docs/syllables.md`.

use super::Rules;
use crate::stroke::Stroke;

/// One position of the pseudo-pronunciation: the sound as a short reading, a long reading, and a
/// weak one.
#[derive(Clone)]
struct Phone {
    heard: String,
    long: String,
    weak: String,
}

impl Phone {
    fn consonant(sound: &str) -> Self {
        Self { heard: sound.into(), long: sound.into(), weak: sound.into() }
    }

    fn vowel(short: &str, long: &str) -> Self {
        Self { heard: format!("{short}1"), long: format!("{long}1"), weak: "AH0".into() }
    }

    /// A vowel pair is one sound: *ea*, *oo*, *ou*
    fn fixed(sound: &str) -> Self {
        Self { heard: format!("{sound}1"), long: format!("{sound}1"), weak: format!("{sound}1") }
    }
}

/// How many readings of one word are tried, at most.
const MAX_READINGS: usize = 24;

const VOWEL_PAIRS: [(&str, &str); 20] = [
    ("ea", "IY"),
    ("ee", "IY"),
    ("ie", "IY"),
    ("ai", "EY"),
    ("ay", "EY"),
    ("ei", "EY"),
    ("ey", "EY"),
    ("oa", "OW"),
    ("ow", "OW"),
    ("oe", "OW"),
    ("oo", "UW"),
    ("ew", "UW"),
    ("eu", "UW"),
    ("ue", "UW"),
    ("ui", "UW"),
    ("ou", "AW"),
    ("oi", "OY"),
    ("oy", "OY"),
    ("au", "AO"),
    ("aw", "AO"),
];

fn is_vowel_letter(c: char) -> bool {
    "aeiou".contains(c)
}

const fn short_long(c: char) -> (&'static str, &'static str) {
    match c {
        'a' => ("AE", "EY"),
        'e' => ("EH", "IY"),
        'o' => ("AA", "OW"),
        'u' => ("AH", "UW"),
        _ => ("IH", "AY"),
    }
}

impl Rules {
    /// The ways to write `word` as joined strokes from its spelling: the cheapest paths of each
    /// stroke count over every reading of its letters, at most `quotas[k - 2]` of those with `k`
    /// strokes, cheapest first.
    pub fn orthographic_paths(&self, word: &str, quotas: &[usize]) -> Vec<(u32, Vec<Stroke>)> {
        self.spelled_paths(word, quotas, 2)
    }

    /// The same for the end of a word whose beginning is struck as a word of its own (`part` then
    /// `icular`): from one stroke up, and a single vowel is enough.
    pub fn spelled_remainder(&self, rest: &str, quotas: &[usize]) -> Vec<(u32, Vec<Stroke>)> {
        self.spelled_paths(rest, quotas, 1)
    }

    fn spelled_paths(&self, word: &str, quotas: &[usize], first: usize) -> Vec<(u32, Vec<Stroke>)> {
        let mut all: Vec<(u32, Vec<Stroke>)> = Vec::new();
        for reading in readings(word, first) {
            let heard: Vec<String> = reading.iter().map(|p| p.heard.clone()).collect();
            let long: Vec<String> = reading.iter().map(|p| p.long.clone()).collect();
            let weak: Vec<String> = reading.iter().map(|p| p.weak.clone()).collect();
            for (cost, path) in self.lattice_paths_from(&heard, &long, &weak, quotas, first) {
                match all.iter_mut().find(|(_, p)| *p == path) {
                    Some(seen) => seen.0 = seen.0.min(cost),
                    None => all.push((cost, path)),
                }
            }
        }
        all.sort_by_key(|(cost, path)| (*cost, path.len()));
        let mut taken = vec![0usize; quotas.len()];
        all.retain(|(_, path)| {
            let k = path.len().saturating_sub(first);
            if k >= quotas.len() || taken[k] >= quotas[k] {
                return false;
            }
            taken[k] += 1;
            true
        });
        all
    }
}

/// The pseudo-pronunciations of a word's letters.
fn readings(word: &str, min_vowels: usize) -> Vec<Vec<Phone>> {
    let letters: Vec<char> = word.chars().collect();
    if !letters.iter().all(char::is_ascii_lowercase) {
        return Vec::new();
    }
    let mut out = Vec::new();
    read(&letters, 0, &mut Vec::new(), &mut out);
    out.retain(|r| r.iter().filter(|p| p.heard.ends_with(|c: char| c.is_ascii_digit())).count() >= min_vowels);
    out
}

fn read(w: &[char], at: usize, so_far: &mut Vec<Phone>, out: &mut Vec<Vec<Phone>>) {
    if out.len() >= MAX_READINGS {
        return;
    }
    if at >= w.len() {
        out.push(so_far.clone());
        return;
    }
    let rest: String = w[at..].iter().collect();
    let c = w[at];
    // every way to read what starts here: the sounds, and how many letters they use
    let mut options: Vec<(Vec<Phone>, usize)> = Vec::new();
    let cons = |s: &[&str]| s.iter().map(|s| Phone::consonant(s)).collect::<Vec<_>>();
    let has_vowel_before = w[..at].iter().any(|&l| is_vowel_letter(l));
    if (rest.starts_with("tio") || rest.starts_with("sio") || rest.starts_with("cio")) && has_vowel_before {
        options.push((vec![Phone::consonant("SH"), Phone::fixed("AH")], 3));
    }
    if let Some(&(pair, sound)) = VOWEL_PAIRS.iter().find(|(p, _)| rest.starts_with(p)) {
        let _ = pair;
        options.push((vec![Phone::fixed(sound)], 2));
    }
    if is_vowel_letter(c) {
        let (short, long) = short_long(c);
        // a silent e after a consonant is not a syllable: at the end of the word (`table`), or
        // before the next consonant when a vowel and a consonant come right before it, as in
        // `move` `ment`, `hope` `ful`, `care` `less`
        let silent_e = c == 'e'
            && at >= 2
            && !is_vowel_letter(w[at - 1])
            && has_vowel_before
            && match w.get(at + 1) {
                None => true,
                Some(&n) => !is_vowel_letter(n) && is_vowel_letter(w[at - 2]),
            };
        if silent_e {
            options.push((Vec::new(), 1));
        }
        options.push((vec![Phone::vowel(short, long)], 1));
    } else {
        let next = w.get(at + 1).copied();
        match (c, next) {
            ('c', Some('h')) => {
                options.push((cons(&["CH"]), 2));
                options.push((cons(&["K"]), 2));
                options.push((cons(&["SH"]), 2));
            }
            ('c', Some('k')) => options.push((cons(&["K"]), 2)),
            ('s', Some('h')) => options.push((cons(&["SH"]), 2)),
            ('t', Some('h')) => {
                options.push((cons(&["TH"]), 2));
                options.push((cons(&["DH"]), 2));
            }
            ('p', Some('h')) => options.push((cons(&["F"]), 2)),
            ('w', Some('h')) => options.push((cons(&["W"]), 2)),
            ('g', Some('h')) => {
                options.push((Vec::new(), 2));
                options.push((cons(&["G"]), 2));
                options.push((cons(&["F"]), 2));
            }
            ('n', Some('g')) => {
                options.push((cons(&["NG"]), 2));
                options.push((cons(&["N", "G"]), 2));
            }
            ('n', Some('k')) => options.push((cons(&["NG", "K"]), 2)),
            ('q', Some('u')) => options.push((cons(&["K", "W"]), 2)),
            ('k', Some('n')) if at == 0 => options.push((cons(&["N"]), 2)),
            ('w', Some('r')) if at == 0 => options.push((cons(&["R"]), 2)),
            (a, Some(b)) if a == b && !is_vowel_letter(a) => options.push((one(a), 2)),
            _ => {}
        }
        options.extend(single(c, at, w).into_iter().map(|p| (p, 1)));
    }
    for (phones, used) in options {
        let before = so_far.len();
        so_far.extend(phones);
        read(w, at + used, so_far, out);
        so_far.truncate(before);
    }
}

/// A doubled consonant is one sound.
fn one(c: char) -> Vec<Phone> {
    single(c, 1, &[]).into_iter().next().unwrap_or_default()
}

/// A single consonant letter's readings.
fn single(c: char, at: usize, w: &[char]) -> Vec<Vec<Phone>> {
    let cons = |s: &[&str]| s.iter().map(|s| Phone::consonant(s)).collect::<Vec<_>>();
    let soft = w.get(at + 1).is_some_and(|n| "eiy".contains(*n));
    match c {
        'c' if soft => vec![cons(&["S"]), cons(&["K"])],
        'c' => vec![cons(&["K"]), cons(&["S"])],
        'g' if soft => vec![cons(&["JH"]), cons(&["G"])],
        'g' => vec![cons(&["G"])],
        's' => vec![cons(&["S"]), cons(&["Z"])],
        'x' => vec![cons(&["K", "S"])],
        'y' if at == 0 || w.get(at + 1).is_some_and(|n| is_vowel_letter(*n)) => vec![cons(&["Y"])],
        'y' => vec![vec![Phone::vowel("IH", "AY")]],
        'b' => vec![cons(&["B"])],
        'd' => vec![cons(&["D"])],
        'f' => vec![cons(&["F"])],
        'h' => vec![cons(&["HH"])],
        'j' => vec![cons(&["JH"])],
        'k' | 'q' => vec![cons(&["K"])],
        'l' => vec![cons(&["L"])],
        'm' => vec![cons(&["M"])],
        'n' => vec![cons(&["N"])],
        'p' => vec![cons(&["P"])],
        'r' => vec![cons(&["R"])],
        't' => vec![cons(&["T"])],
        'v' => vec![cons(&["V"])],
        'w' => vec![cons(&["W"])],
        'z' => vec![cons(&["Z"])],
        _ => Vec::new(),
    }
}
