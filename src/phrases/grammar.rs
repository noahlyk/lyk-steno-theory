//! The phrasing grammar: a starter (left bank) + an auxiliary (vowels; the star is *not*) + a verb
//! (right bank) in one stroke, so `I was`, `they wouldn't` and `she has been` are one stroke each.
//!
//! The parts are data (`rules/grammar.toml`); a verb's right-bank code is worked out here, from the
//! verb's own sounds, by the rule on `site/reference/phrases.json` ("The codes"). [`Grammar::phrases`]
//! writes every phrase with the part of the stroke that says each piece, [`place`] commits them
//! ahead of the merged-pair phrases, and the lookup commands read the same tables back
//! ([`Grammar::explain`]) to say what a stroke is made of.

use crate::analysis::Setup;
use crate::generate::{Book, Entry, Origin, typable};
use crate::rules::Syllable;
use crate::stroke::{Stroke, parse_outline, render_outline};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// The grammar's tables, `rules/grammar.toml`.
const BUILT_IN: &str = include_str!("../../rules/grammar.toml");

/// Which part of the stroke says a piece of the phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    /// The number bar: the words are in question order
    Question,
    /// The left bank: who
    Starter,
    /// The vowels: which modal, and `E` for the past
    Auxiliary,
    /// The star: the negative of the rest
    Not,
    /// The right bank: *be*, *have*, *have been* or one of the common verbs
    Verb,
}

impl PartKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Starter => "starter",
            Self::Auxiliary => "auxiliary",
            Self::Not => "not",
            Self::Verb => "verb",
        }
    }
}

/// One group of keys of a phrase's stroke, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub kind: PartKind,
    /// The group's keys as notation: `SWR`, `AE`, `*`, `-B`
    pub keys: String,
    /// What it says: `I`, `could`, `past`, `not`, `be`
    pub says: String,
}

/// A phrase the grammar writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    /// The words, lowercase (`i was`), the generator's own key; `written` capitalizes the pronoun
    pub text: String,
    pub stroke: Stroke,
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
struct Starter {
    word: String,
    chord: String,
    person: String,
    #[serde(default)]
    only: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Modal {
    keys: String,
    present: String,
    past: Option<String>,
    negative: String,
    past_negative: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Be {
    code: String,
    present: HashMap<String, String>,
    present_negative: HashMap<String, String>,
    past: HashMap<String, String>,
    past_negative: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Auxiliary {
    code: Option<String>,
    been_code: Option<String>,
    base: String,
    third: String,
    past: String,
    negative: String,
}

#[derive(Debug, Deserialize)]
struct Verb {
    base: String,
    third: String,
    past: String,
}

#[derive(Debug, Deserialize)]
struct Tables {
    past: String,
    not: String,
    question: String,
    extra: String,
    starter: Vec<Starter>,
    modal: Vec<Modal>,
    be: Be,
    have: Auxiliary,
    #[serde(rename = "do")]
    do_: Auxiliary,
    verb: Vec<Verb>,
}

/// What the verb slot holds: nothing, *be*, *have*, *have been*, or a lexical verb (its place in
/// the list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    None,
    Be,
    Have,
    HaveBeen,
    Lexical(usize),
}

/// The right-bank code of every verb slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Codes {
    be: Stroke,
    have: Stroke,
    have_been: Stroke,
    /// In the order of `rules/grammar.toml`'s verbs
    verbs: Vec<Stroke>,
}

impl Codes {
    /// Each verb with its code, `be`, `have` and `have been` first, then the lexical verbs in the
    /// order of the file.
    pub fn named(&self, grammar: &Grammar) -> Vec<(String, Stroke)> {
        let mut all = vec![("be".to_owned(), self.be), ("have".to_owned(), self.have), ("have been".to_owned(), self.have_been)];
        all.extend(grammar.t.verb.iter().zip(&self.verbs).map(|(v, &c)| (v.base.clone(), c)));
        all
    }
}

/// The grammar's tables.
#[derive(Debug)]
pub struct Grammar {
    t: Tables,
}

fn stroke_of(text: &str) -> Stroke {
    let outline = parse_outline(text).unwrap_or_else(|| panic!("{text:?} in rules/grammar.toml is not a stroke"));
    outline[0]
}

fn keys_text(bits: u32) -> String {
    render_outline(&[Stroke(bits)])
}

/// The grammar's phrases in one go: what each part is called, and the stroke they make together.
struct Build<'a> {
    g: &'a Grammar,
    codes: &'a Codes,
    /// Write each phrase as a question (the number bar down, the words in question order)
    question: bool,
    out: Vec<Phrase>,
}

impl Grammar {
    pub fn built_in() -> anyhow::Result<Self> {
        Ok(Self { t: toml::from_str(BUILT_IN)? })
    }

    /// Each verb's right-bank code, worked out from its sounds. In the order of the file (commonest
    /// first), the code is the verb's own final consonants as the rules write them, or its first
    /// consonant when it ends in a vowel; if an earlier verb (or *be*, *have*, *have been*) has that
    /// code, the first consonant is added; if that is taken as well, `-Z`.
    pub fn derive_codes(&self, setup: &Setup) -> Result<Codes, String> {
        let be = stroke_of(&self.t.be.code);
        let have = stroke_of(self.t.have.code.as_deref().ok_or("have has no code")?);
        let have_been = stroke_of(self.t.have.been_code.as_deref().ok_or("have been has no code")?);
        let extra = stroke_of(&self.t.extra);
        let mut taken: HashSet<u32> = [be.0, have.0, have_been.0].into();
        let mut verbs = Vec::new();
        for verb in &self.t.verb {
            let word = verb.base.as_str();
            let sounds = setup.pron.get(word).and_then(|p| p.first()).ok_or_else(|| format!("{word} has no pronunciation"))?;
            let labelled = setup.rules.label(word, sounds);
            let syllables = setup.rules.syllables(&labelled).ok_or_else(|| format!("{word} has no syllables"))?;
            let coda_of = |sounds: &[String]| -> Result<Stroke, String> {
                let syllable = Syllable { onset: Vec::new(), vowel: String::new(), coda: sounds.to_vec() };
                setup.rules.syllable_stroke(&syllable).ok_or_else(|| format!("{word}: {sounds:?} is not a coda"))
            };
            let last = syllables.last().ok_or("no syllable")?;
            let first = syllables.first().ok_or("no syllable")?;
            // a consonant with no right-bank key (the `W` of *want*) adds nothing
            let initial = first.onset.first().and_then(|s| coda_of(std::slice::from_ref(s)).ok());
            let own = if last.coda.is_empty() { initial.ok_or_else(|| format!("{word} has no consonant"))? } else { coda_of(&last.coda)? };
            // a code is right-bank keys only: the star some coda clusters carry (`NG K`) is *not*
            let mut code = Stroke(own.0 & !Stroke::STAR);
            if taken.contains(&code.0) {
                code = Stroke(code.0 | initial.map_or(0, |s| s.0));
            }
            if taken.contains(&code.0) {
                code = Stroke(code.0 | extra.0);
            }
            if taken.contains(&code.0) {
                return Err(format!("{word}: every code {} is taken", render_outline(&[code])));
            }
            taken.insert(code.0);
            verbs.push(code);
        }
        Ok(Codes { be, have, have_been, verbs })
    }

    /// The verb codes as the built dictionary has them: the right bank of `I be`, `I have`, ...
    /// found by the text of a phrase. `None` when the dictionary has no grammar.
    pub fn codes_from(&self, stroke_of_text: impl Fn(&str) -> Option<Stroke>) -> Option<Codes> {
        let first = self.t.starter.first()?;
        let code = |text: String| stroke_of_text(&text).map(|s| Stroke(s.0 & Stroke::RIGHT));
        let subject = first.word.to_lowercase();
        let third = first.person == "3s";
        let be = code(format!("{subject} {}", self.t.be.present.get(&first.person)?))?;
        let have = code(format!("{subject} {}", if third { &self.t.have.third } else { &self.t.have.base }))?;
        let have_been = code(format!("{subject} {} been", if third { &self.t.have.third } else { &self.t.have.base }))?;
        let verbs = self
            .t
            .verb
            .iter()
            .map(|v| code(format!("{subject} {}", if third { &v.third } else { &v.base })))
            .collect::<Option<Vec<_>>>()?;
        Some(Codes { be, have, have_been, verbs })
    }

    /// Every phrase the grammar writes, in the order of the tables.
    pub fn phrases(&self, codes: &Codes) -> Vec<Phrase> {
        let mut build = Build { g: self, codes, question: false, out: Vec::new() };
        for starter in &self.t.starter {
            build.starter(starter);
        }
        build.out
    }

    /// The question form of every phrase: the same stroke with the number bar down, the words in
    /// question order (`can you`, `isn't it`, `do I think`). One for each of [`Self::phrases`], in
    /// the same order.
    pub fn questions(&self, codes: &Codes) -> Vec<Phrase> {
        let mut build = Build { g: self, codes, question: true, out: Vec::new() };
        for starter in &self.t.starter {
            build.starter(starter);
        }
        build.out
    }

    /// The parts of the phrase `stroke` writes, if the grammar writes one.
    #[cfg(test)]
    pub fn explain(&self, codes: &Codes, stroke: Stroke) -> Option<Phrase> {
        self.phrases(codes).into_iter().find(|p| p.stroke == stroke)
    }
}

/// The grammar as a built dictionary has it: the phrases it holds, by their text and by their
/// stroke, for the lookup commands to say what a phrase or a stroke is made of.
pub struct Phrasing {
    phrases: Vec<Phrase>,
    by_text: HashMap<String, usize>,
    by_stroke: HashMap<Stroke, usize>,
}

impl Phrasing {
    /// The grammar read back from `entries`: the verb codes come from the phrases the dictionary
    /// has (`I am`, `I say`, ...), and only a phrase the dictionary holds with its own stroke
    /// counts. `None` when the dictionary has no grammar (one built before it existed).
    pub fn read(entries: &[Entry]) -> Option<Self> {
        let own: HashMap<&str, Stroke> =
            entries.iter().filter(|e| e.origin == Origin::Grammar).filter_map(|e| Some((e.word.as_str(), *e.outline.first()?))).collect();
        if own.is_empty() {
            return None;
        }
        let grammar = Grammar::built_in().ok()?;
        let codes = grammar.codes_from(|text| own.get(text).copied())?;
        let phrases: Vec<Phrase> = grammar
            .phrases(&codes)
            .into_iter()
            .chain(grammar.questions(&codes))
            .filter(|p| own.get(p.text.as_str()) == Some(&p.stroke))
            .collect();
        let by_text = phrases.iter().enumerate().map(|(i, p)| (p.text.clone(), i)).collect();
        let by_stroke = phrases.iter().enumerate().map(|(i, p)| (p.stroke, i)).collect();
        Some(Self { phrases, by_text, by_stroke })
    }

    /// The phrase written `text` (lowercase words), if the grammar writes it.
    pub fn text(&self, text: &str) -> Option<&Phrase> {
        self.by_text.get(text).map(|&i| &self.phrases[i])
    }

    /// The phrase `stroke` writes, if it is one of the grammar's.
    pub fn stroke(&self, stroke: Stroke) -> Option<&Phrase> {
        self.by_stroke.get(&stroke).map(|&i| &self.phrases[i])
    }

    /// The longest start of `words` (lowercase, two words at least) that the grammar writes, and
    /// how many words it covers.
    pub fn leading(&self, words: &[String]) -> Option<(usize, &Phrase)> {
        (2..=words.len()).rev().find_map(|n| self.text(&words[..n].join(" ")).map(|p| (n, p)))
    }
}

impl Build<'_> {
    /// The words a verb slot adds after the auxiliary: the verb as the modal takes it (`be`, `have
    /// been`, `go`).
    fn name(&self, slot: Slot) -> Option<String> {
        match slot {
            Slot::None => None,
            Slot::Be => Some("be".to_owned()),
            Slot::Have => Some(self.g.t.have.base.clone()),
            Slot::HaveBeen => Some(format!("{} been", self.g.t.have.base)),
            Slot::Lexical(i) => Some(self.g.t.verb[i].base.clone()),
        }
    }

    fn code(&self, slot: Slot) -> Option<Stroke> {
        match slot {
            Slot::None => None,
            Slot::Be => Some(self.codes.be),
            Slot::Have => Some(self.codes.have),
            Slot::HaveBeen => Some(self.codes.have_been),
            Slot::Lexical(i) => Some(self.codes.verbs[i]),
        }
    }

    fn starter(&mut self, starter: &Starter) {
        let third = starter.person == "3s";
        let slots: Vec<Slot> = [Slot::None, Slot::Be, Slot::Have, Slot::HaveBeen]
            .into_iter()
            .chain((0..self.g.t.verb.len()).map(Slot::Lexical))
            .collect();
        // `None` is no modal: the present or past tense of the verb on its own
        let modals: Vec<Option<&Modal>> = std::iter::once(None).chain(self.g.t.modal.iter().map(Some)).collect();
        for modal in modals {
            for past in [false, true] {
                if past && modal.is_some_and(|m| m.past.is_none()) {
                    continue;
                }
                for negative in [false, true] {
                    for &slot in &slots {
                        let verb_name = self.name(slot);
                        if !starter.only.is_empty() && slot != Slot::None && !verb_name.as_ref().is_some_and(|n| starter.only.contains(n)) {
                            continue;
                        }
                        if !starter.only.is_empty() && slot == Slot::None && modal.is_none() {
                            continue;
                        }
                        let Some(words) = (if self.question {
                            self.question_words(starter, third, modal, past, negative, slot, verb_name)
                        } else {
                            self.words(starter, third, modal, past, negative, slot, verb_name)
                        }) else {
                            continue;
                        };
                        self.push(starter, modal, past, negative, slot, &words);
                    }
                }
            }
        }
    }

    /// The words after the starter.
    #[allow(clippy::too_many_arguments)]
    fn words(&self, starter: &Starter, third: bool, modal: Option<&Modal>, past: bool, negative: bool, slot: Slot, verb_name: Option<String>) -> Option<String> {
        let t = &self.g.t;
        if let Some(m) = modal {
            let aux = match (negative, past) {
                (false, false) => Some(&m.present),
                (false, true) => m.past.as_ref(),
                (true, false) => Some(&m.negative),
                (true, true) => m.past_negative.as_ref(),
            }?;
            return Some(verb_name.map_or_else(|| aux.clone(), |v| format!("{aux} {v}")));
        }
        let neg = |word: &str, suffix: &str| if negative { format!("{word}{suffix}") } else { word.to_owned() };
        let do_ = &t.do_;
        let have = &t.have;
        let person = starter.person.as_str();
        Some(match slot {
            Slot::None => neg(if past { &do_.past } else if third { &do_.third } else { &do_.base }, &do_.negative),
            Slot::Be => {
                let table = match (past, negative) {
                    (false, false) => &t.be.present,
                    (false, true) => &t.be.present_negative,
                    (true, false) => &t.be.past,
                    (true, true) => &t.be.past_negative,
                };
                table.get(person)?.clone()
            }
            Slot::Have | Slot::HaveBeen => {
                let form = neg(if past { &have.past } else if third { &have.third } else { &have.base }, &have.negative);
                if slot == Slot::HaveBeen { format!("{form} been") } else { form }
            }
            Slot::Lexical(i) => {
                let verb = &t.verb[i];
                if negative {
                    format!("{} {}", neg(if past { &do_.past } else if third { &do_.third } else { &do_.base }, &do_.negative), verb.base)
                } else if past {
                    verb.past.clone()
                } else if third {
                    verb.third.clone()
                } else {
                    verb.base.clone()
                }
            }
        })
    }

    /// The whole phrase as a question: the auxiliary, the starter, then the rest (`can you go`,
    /// `isn't it`, `do you think`, `am I not`). Only an auxiliary opens a question, so the verb
    /// alone takes *do* (`do you have`, `did he say`) and *have been* keeps its own *have*.
    #[allow(clippy::too_many_arguments)]
    fn question_words(&self, starter: &Starter, third: bool, modal: Option<&Modal>, past: bool, negative: bool, slot: Slot, verb_name: Option<String>) -> Option<String> {
        let t = &self.g.t;
        let who = starter.word.to_lowercase();
        // an auxiliary with its `not` spelt out (`shall not`, `am not`) splits around the starter
        let around = |aux: &str| aux.strip_suffix(" not").map_or_else(|| format!("{aux} {who}"), |head| format!("{head} {who} not"));
        if let Some(m) = modal {
            let aux = match (negative, past) {
                (false, false) => Some(&m.present),
                (false, true) => m.past.as_ref(),
                (true, false) => Some(&m.negative),
                (true, true) => m.past_negative.as_ref(),
            }?;
            let head = around(aux);
            return Some(verb_name.map_or_else(|| head.clone(), |v| format!("{head} {v}")));
        }
        let neg = |word: &str, suffix: &str| if negative { format!("{word}{suffix}") } else { word.to_owned() };
        let do_ = &t.do_;
        let have = &t.have;
        let does = around(&neg(if past { &do_.past } else if third { &do_.third } else { &do_.base }, &do_.negative));
        Some(match slot {
            Slot::None => does,
            Slot::Be => {
                let table = match (past, negative) {
                    (false, false) => &t.be.present,
                    (false, true) => &t.be.present_negative,
                    (true, false) => &t.be.past,
                    (true, true) => &t.be.past_negative,
                };
                around(table.get(starter.person.as_str())?)
            }
            Slot::Have => format!("{does} {}", have.base),
            Slot::HaveBeen => format!("{} been", around(&neg(if past { &have.past } else if third { &have.third } else { &have.base }, &have.negative))),
            Slot::Lexical(i) => format!("{does} {}", t.verb[i].base),
        })
    }

    fn push(&mut self, starter: &Starter, modal: Option<&Modal>, past: bool, negative: bool, slot: Slot, words: &str) {
        let t = &self.g.t;
        let mut parts = Vec::new();
        let mut bits = 0;
        if self.question {
            let bar = stroke_of(&t.question).0;
            parts.push(Part { kind: PartKind::Question, keys: keys_text(bar), says: "question".to_owned() });
            bits |= bar;
        }
        parts.push(Part { kind: PartKind::Starter, keys: starter.chord.clone(), says: starter.word.clone() });
        bits |= stroke_of(&starter.chord).0;
        let mut vowels = modal.map_or(0, |m| stroke_of(&m.keys).0);
        if past {
            vowels |= stroke_of(&t.past).0;
        }
        if vowels != 0 {
            let says = match (modal, past) {
                (Some(m), true) => m.past.clone().unwrap_or_default(),
                (Some(m), false) => m.present.clone(),
                (None, _) => "past".to_owned(),
            };
            parts.push(Part { kind: PartKind::Auxiliary, keys: keys_text(vowels), says });
            bits |= vowels;
        }
        if negative {
            let star = stroke_of(&t.not).0;
            parts.push(Part { kind: PartKind::Not, keys: keys_text(star), says: "not".to_owned() });
            bits |= star;
        }
        if let Some(code) = self.code(slot) {
            parts.push(Part { kind: PartKind::Verb, keys: keys_text(code.0), says: self.name(slot).unwrap_or_default() });
            bits |= code.0;
        }
        let text = if self.question { words.to_owned() } else { format!("{} {words}", starter.word.to_lowercase()) };
        self.out.push(Phrase { text, stroke: Stroke(bits), parts });
    }
}

/// What the placement did.
#[derive(Debug, Default)]
pub struct Placed {
    /// Phrases committed
    pub added: usize,
    /// Phrases left out: the stroke, the text and why
    pub unplaced: Vec<(String, String, String)>,
    /// Each verb with the code the rule gave it
    pub codes: Vec<(String, String)>,
    /// Questions the dictionary already writes as an entry (a merged pair): the text and the
    /// outline it keeps
    pub kept: Vec<(String, String)>,
}

/// Fix the grammar's phrases in the book before any word settles, as the standalone suffixes are:
/// a starter is a chord no word is meant to start with, so the few words whose own candidates
/// land on one settle on their next outline instead. A stroke that is already held (a reserved
/// stroke, a suffix, a word's own brief in `rules/briefs.toml`) is not taken: the phrase is left
/// out and reported with its holder.
pub fn fix(setup: &Setup, book: &mut Book) -> Result<Placed, String> {
    let grammar = Grammar::built_in().map_err(|e| e.to_string())?;
    let codes = grammar.derive_codes(setup)?;
    let mut placed = Placed {
        codes: codes.named(&grammar).into_iter().map(|(v, c)| (v, render_outline(&[c]))).collect(),
        ..Placed::default()
    };
    for phrase in grammar.phrases(&codes) {
        let text = render_outline(&[phrase.stroke]);
        if !typable(&[phrase.stroke]) {
            placed.unplaced.push((text, phrase.text, "cannot be pressed (the right pinky)".to_owned()));
        } else if let Some(holder) = book.taken.get(&text) {
            let why = format!("{holder:?} has the stroke");
            placed.unplaced.push((text, phrase.text, why));
        } else {
            book.place(&phrase.text, vec![phrase.stroke], Origin::Grammar, text);
            placed.added += 1;
        }
    }
    Ok(placed)
}

/// Fix the question forms in the book, after the merged pairs and every word have settled. A
/// question stroke is a statement's with the number bar down, which nothing else uses, so it never
/// takes a stroke from anything; a question whose text the dictionary already has (a merged pair
/// such as `do you`, `are you`) is left alone and keeps its outline.
pub fn place_questions(setup: &Setup, book: &mut Book) -> Result<Placed, String> {
    let grammar = Grammar::built_in().map_err(|e| e.to_string())?;
    let codes = grammar.derive_codes(setup)?;
    let have: HashMap<String, String> = book.entries.iter().map(|e| (e.word.clone(), render_outline(&e.outline))).collect();
    let mut placed = Placed::default();
    for phrase in grammar.questions(&codes) {
        let text = render_outline(&[phrase.stroke]);
        if let Some(outline) = have.get(&phrase.text) {
            placed.kept.push((phrase.text, outline.clone()));
        } else if !typable(&[phrase.stroke]) {
            placed.unplaced.push((text, phrase.text, "cannot be pressed (the right pinky)".to_owned()));
        } else if let Some(holder) = book.taken.get(&text) {
            let why = format!("{holder:?} has the stroke");
            placed.unplaced.push((text, phrase.text, why));
        } else {
            book.place(&phrase.text, vec![phrase.stroke], Origin::Grammar, text);
            placed.added += 1;
        }
    }
    Ok(placed)
}

/// Every text the grammar writes, for the merged-pair pass to skip.
pub fn texts(entries: &[Entry]) -> HashSet<&str> {
    entries.iter().filter(|e| e.origin == Origin::Grammar).map(|e| e.word.as_str()).collect()
}

#[cfg(test)]
mod tests;
