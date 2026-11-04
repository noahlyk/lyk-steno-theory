//! The phonetic rules: from sounds to strokes, read from a TOML file.

use crate::stroke::Stroke;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The rules that ship with the program.
mod brevity;
mod fragments;
mod lattice;
mod plugin;
mod resolve;


const BUILT_IN: &str = include_str!("../../rules/plover.toml");
/// Spelling-mode's letter table (see `docs/spelling-mode.md`): always this file, independent of
/// whichever rules file `Rules::load` was given, the same way `rules/suffixes.toml` and
/// `rules/briefs.toml` are always their own built-in file regardless of the theory in use —
/// every letter gets one unambiguous chord no matter which phonetic theory is loaded.
const BUILT_IN_SPELLING: &str = include_str!("../../rules/spelling.toml");
/// `rules/suffixes.toml`'s own table, loaded the same unconditional way as `BUILT_IN_SPELLING`
/// above: see [`Rules::suffixes`].
const BUILT_IN_SUFFIXES: &str = include_str!("../../rules/suffixes.toml");

/// The right-hand `R`, the one key a vowel (`ER`, `AR`) and a cluster (`-FRB`) may share.
const R_KEY: u32 = 1 << 14;
/// How many sounds of one stroke may fall out of steno order (see `consonants_shared`).
const MAX_OUT_OF_ORDER: u32 = 2;

#[derive(Deserialize, Default)]
struct Raw {
    #[serde(default)]
    onset: HashMap<String, String>,
    #[serde(default)]
    coda: HashMap<String, String>,
    #[serde(default)]
    vowel: HashMap<String, String>,
    #[serde(default)]
    vowel_coda: HashMap<String, String>,
    #[serde(default)]
    onset_cluster: HashMap<String, String>,
    #[serde(default)]
    coda_cluster: HashMap<String, String>,
    #[serde(default)]
    syllables: RawSyllables,
    #[serde(default)]
    spelled: Vec<Spelled>,
    #[serde(default)]
    reduce: RawReduce,
    #[serde(default)]
    theory: RawTheory,
    #[serde(default)]
    brevity: Vec<RawBrevity>,
    #[serde(default)]
    prefix: Vec<Prefix>,
}

/// A prefix written with fixed keys that join the first stroke of the rest of the word: `ex-` is
/// `KP`, so `expect` is `KPEBGT`.
#[derive(Deserialize, Debug, Clone)]
pub struct Prefix {
    /// The word starts with these letters
    pub spelling: String,
    /// And its sounds start with one of these (with stress digits)
    pub sounds: Vec<String>,
    /// The keys that replace those sounds
    pub keys: String,
}

#[derive(Deserialize)]
struct RawBrevity {
    step: Step,
    /// Words up to this frequency rank take this step
    rank: usize,
    /// Whether the stressed vowel is kept (one-stroke steps)
    #[serde(default = "yes")]
    vowel: bool,
    /// Only words with at least this many syllables
    #[serde(default = "two")]
    min_syllables: usize,
    /// Whether the folded consonants may land out of steno order (`any`) or not (`strict`)
    #[serde(default)]
    order: Option<String>,
}

const fn yes() -> bool {
    true
}

const fn two() -> usize {
    2
}

/// A way to write a word of several syllables shorter than a stroke per syllable. Each is a
/// transformation of the full outline a learner can do by hand.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// One stroke: the consonants that start the word, then every consonant after its first vowel
    /// folded onto the right hand.
    Skeleton,
    /// One stroke: the consonants that start the word and those of the stressed and last
    /// syllables.
    Shell,
    /// One stroke: the consonants that start the word and those of the last syllable.
    Ends,
    /// An unstressed first syllable that is only a vowel is dropped: `about` PWOUT.
    Lead,
    /// The consonants of an unstressed first syllable go on the left hand of the next stroke:
    /// `believe` PWHRAOEF.
    Lean,
    /// The outline with reduced syllables folded out of steno order where they would not fit in
    /// order, as Plover does: `student` STAOUPBTD, `goodness` TKPWAOPBSD.
    Fold,
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Skeleton => "skeleton",
            Self::Shell => "shell",
            Self::Ends => "ends",
            Self::Lead => "lead",
            Self::Lean => "lean",
            Self::Fold => "fold",
        })
    }
}

/// One shortening step and how common a word has to be to take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortening {
    pub step: Step,
    pub rank: usize,
    pub vowel: bool,
    pub min_syllables: usize,
    pub any: bool,
}

#[derive(Deserialize, Default)]
struct RawReduce {
    /// Unstressed vowels that are not written: the syllable's consonants join the stroke before
    #[serde(default)]
    vowels: Vec<String>,
    /// Whether the joined consonants may land out of steno order (`any`) or only in order
    /// (`strict`), writing a continuation stroke otherwise
    #[serde(default)]
    order: Option<String>,
    /// Variants of those vowels that are written after all: a final `a` (`china` KHAOEU/TPHA)
    #[serde(default)]
    except: Vec<String>,
    /// Variants only reduced when a single consonant starts the syllable: `-ly` folds as -L in
    /// `lately`, but `assembly` ends in a stroke of its own, PWHREU
    #[serde(default)]
    lone: Vec<String>,
    /// Consonants that may land out of steno order when a syllable of only them is folded:
    /// Plover's -RT for `-ter` (`better` PWERT) and -LT for `-tly` (`lately` HRAEULT)
    #[serde(default)]
    loose: Vec<String>,
}

#[derive(Deserialize, Default)]
struct RawTheory {
    /// Strokes after the first of a word carry a join key
    #[serde(default)]
    join_key: bool,
}

#[derive(Deserialize, Default)]
struct RawSyllables {
    #[serde(default)]
    onsets: Vec<String>,
    #[serde(default)]
    lax: Vec<String>,
}

/// A sound that is written differently depending on how the word is spelled. It becomes a
/// variant of the sound (`UW.oo`), which has its own entry in the tables.
#[derive(Deserialize, Debug, Clone)]
pub struct Spelled {
    pub sound: String,
    pub label: String,
    /// The word starts with this, and the sound is its first
    pub starts: Option<String>,
    /// The word ends with this, and the sound is its last
    pub ends: Option<String>,
    /// The word has this in it
    pub contains: Option<String>,
    /// The word starts with this, and the sound is its first vowel and unstressed: `be-` in
    /// `believe` but not in `bee`
    pub prefix: Option<String>,
    /// The sound before this one is this (`R` also matches a vowel with an R in it, `ER`)
    pub after: Option<String>,
    /// The sound after this one is this, and the last: the `Z` of `babies`
    pub before: Option<String>,
    /// The word has at least this many syllables
    pub min_syllables: Option<usize>,
    /// The word has none of these in it
    #[serde(default)]
    pub unless: Vec<String>,
    /// Only used to tell apart words that sound the same: the word is written this way when
    /// another word would otherwise have the same outline
    #[serde(default)]
    pub clash: bool,
}

impl Spelled {
    fn applies(
        &self,
        word: &str,
        sounds: &[String],
        at: usize,
        syllables: usize,
        first_vowel: usize,
    ) -> bool {
        let count = sounds.len();
        let before = at.checked_sub(1).map(|i| plain(&sounds[i]));
        self.min_syllables.is_none_or(|n| syllables >= n)
            && self
                .after
                .as_ref()
                .is_none_or(|s| before.is_some_and(|b| b == s || (s == "R" && b == "ER")))
            && self
                .before
                .as_ref()
                .is_none_or(|s| at + 2 == count && plain(&sounds[at + 1]) == s)
            && !self.unless.iter().any(|s| word.contains(s.as_str()))
            && self
                .starts
                .as_ref()
                .is_none_or(|s| at == 0 && word.starts_with(s.as_str()))
            && self.prefix.as_ref().is_none_or(|s| {
                at == first_vowel && sounds[at].ends_with('0') && word.starts_with(s.as_str())
            })
            && self
                .ends
                .as_ref()
                .is_none_or(|s| at + 1 == count && word.ends_with(s.as_str()))
            && self
                .contains
                .as_ref()
                .is_none_or(|s| word.contains(s.as_str()))
    }
}

/// One syllable's sounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    pub onset: Vec<String>,
    /// With its stress digit: `AH0`
    pub vowel: String,
    pub coda: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Rules {
    onset: HashMap<String, Stroke>,
    coda: HashMap<String, Stroke>,
    vowel: HashMap<String, Stroke>,
    vowel_coda: HashMap<String, Stroke>,
    onset_cluster: HashMap<String, Stroke>,
    coda_cluster: HashMap<String, Stroke>,
    onsets: HashSet<String>,
    lax: HashSet<String>,
    spelled: Vec<Spelled>,
    reduce: HashSet<String>,
    keep: HashSet<String>,
    lone: HashSet<String>,
    loose: HashSet<String>,
    /// Whether consonants joining the stroke before may land out of steno order
    fold_any: bool,
    join_key: bool,
    brevity: Vec<Shortening>,
    prefixes: Vec<(Prefix, Stroke)>,
    // Read the keys of a stroke the rules did not write, back into the sounds that would have
    // written them: the tables above, inverted once here instead of on every call.
    onset_rev: Vec<(Stroke, Vec<String>)>,
    coda_rev: Vec<(Stroke, Vec<String>)>,
    vowel_rev: HashMap<Stroke, String>,
    vowel_coda_rev: Vec<(Stroke, String, String)>,
    /// Spelling-mode's chord -> letter table (see `docs/spelling-mode.md`), keyed by each
    /// letter's bare chord (no join or spell bit).
    spelling: HashMap<Stroke, String>,
    /// `rules/suffixes.toml`'s own chord -> text table, keyed exactly as that file writes each
    /// chord (join bit included, e.g. `+-D`) — consulted by [`Self::resolve`] for a continuation
    /// stroke that misses the dictionary entirely, so a suffix reads as its own real spelling
    /// ("ed") rather than a generic phonetic guess at its bare consonants ("d") whenever it rides
    /// along after some other, unrelated stroke with no dictionary entry of its own.
    suffixes: HashMap<Stroke, String>,
    /// `rules/suffixes.toml`'s two-stroke entries (`-ism`, `-ary`/`-ery`/`-ory`, `-ical`, `-logy`,
    /// `-cy`/`-sy`, `-ify`/`-fy`) - each one's own spelling and its two-stroke chord, exactly as
    /// written there. Consulted by [`Self::two_stroke_suffix`] for the backup-generation pass in
    /// `generate/backup.rs`, by spelling rather than re-derived sound rules (see that method's own doc).
    two_stroke_suffixes: Vec<(String, Vec<Stroke>)>,
}

/// A table inverted: each distinct right-hand side keeps every left-hand side that produced it,
/// split into its sound tokens (`"P Y"` is `["P", "Y"]`). Sorted by its tokens, so that when two
/// entries share the exact same keys (`Z` and the glide cluster `Z Y` are both `STKPW`, since
/// the glide is silent on the keys) the tie always breaks the same way between runs.
fn invert(table: &HashMap<String, Stroke>) -> Vec<(Stroke, Vec<String>)> {
    let mut rev: Vec<(Stroke, Vec<String>)> = table
        .iter()
        .map(|(sounds, &stroke)| {
            (
                stroke,
                sounds.split_whitespace().map(str::to_owned).collect(),
            )
        })
        .collect();
    rev.sort_by(|a, b| a.1.cmp(&b.1));
    rev
}

fn strokes(table: HashMap<String, String>, what: &str) -> Result<HashMap<String, Stroke>> {
    table
        .into_iter()
        .map(|(sound, text)| {
            let stroke = Stroke::parse(&text)
                .with_context(|| format!("[{what}] {sound} = {text:?} is not a stroke"))?;
            Ok((sound, stroke))
        })
        .collect()
}

/// `rules/spelling.toml`'s chord-notation keys, parsed into the bare chords (no join or spell
/// bit) spelling mode looks a stroke's keys up by.
fn spelling_table(raw: &str) -> Result<HashMap<Stroke, String>> {
    let table: HashMap<String, String> = toml::from_str(raw)?;
    table
        .into_iter()
        .map(|(chord, letter)| {
            let stroke = Stroke::parse(&chord)
                .with_context(|| format!("[spelling] {chord:?} is not a stroke"))?;
            Ok((stroke, letter))
        })
        .collect()
}

/// `rules/suffixes.toml`'s single-stroke entries only, keyed by their own chord exactly as
/// written there — its handful of two-stroke suffixes (`-ism`, `-ary`/`-ery`/`-ory`, `-ical`,
/// `-logy`, `-ity`/`-ety`, `-cy`/`-sy`, `-ify`/`-fy`) don't parse as a single [`Stroke`] at all
/// (their own chord has a `/` in it) and are skipped rather than erroring the whole table: a
/// continuation stroke [`Rules::resolve`] reads is always exactly one stroke at a time, so a
/// suffix that takes two has nothing here to match against yet (see `docs/suffixes.md`).
fn single_stroke_suffixes(raw: &str) -> Result<HashMap<Stroke, String>> {
    let table: HashMap<String, String> = toml::from_str(raw)?;
    Ok(table
        .into_iter()
        .filter_map(|(chord, text)| Stroke::parse(&chord).map(|stroke| (stroke, text)))
        .collect())
}

/// `rules/suffixes.toml`'s two-stroke entries only - the ones [`single_stroke_suffixes`] skips
/// because their own chord has a `/` in it. Each chord is parsed whole, in order, keeping its own
/// join bits exactly as written (both strokes already carry `+`).
fn two_stroke_suffixes(raw: &str) -> Result<Vec<(String, Vec<Stroke>)>> {
    let table: HashMap<String, String> = toml::from_str(raw)?;
    table
        .into_iter()
        .filter(|(chord, _)| chord.contains('/'))
        .map(|(chord, text)| {
            let strokes: Option<Vec<Stroke>> = chord.split('/').map(Stroke::parse).collect();
            let strokes = strokes
                .filter(|s| s.len() == 2)
                .with_context(|| format!("[suffixes] {chord:?} is not a valid two-stroke chord"))?;
            Ok((text, strokes))
        })
        .collect()
}

/// A sound without its stress digit or spelling variant: `UW.oo1` is `UW`.
fn plain(sound: &str) -> &str {
    sound
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .split('.')
        .next()
        .unwrap_or(sound)
}

/// A syllable written as a continuation stroke: if that stroke has no vowel keys, the syllable
/// has no vowel either when more consonants fold onto it (`confidence` KOPB/-FD/-PBS).
fn vowelless(mut syllable: Syllable, written: Stroke) -> Syllable {
    if written.0 & (Stroke::VOWELS | Stroke::STAR) == 0 {
        syllable.vowel = String::new();
    }
    // Written on the right hand, its consonants stay there
    if written.0 & Stroke::LEFT == 0 {
        let mut coda = std::mem::take(&mut syllable.onset);
        coda.append(&mut syllable.coda);
        syllable.coda = coda;
    }
    syllable
}

/// How a vowel label sorts when two share a physical stroke: lowest wins the stroke's reverse
/// reading. A plain sound normally beats a spelling variant, except `UH` ("book", "put"), which
/// is rarer in practice than the `UW.oo` ("too", "-oo" endings like "kangaroo") and `OW.oa`
/// ("boat") variants it happens to share bare `AO` with — ranking it first like any other plain
/// sound would make the fallback's best guess for that stroke the least likely reading.
fn vowel_rank(s: &str) -> (bool, &str) {
    (!s.contains('.') && s != "UH", s)
}

/// A sound without its spelling variant but with its stress: `UW.oo1` is `UW1`.
fn unlabelled(sound: &str) -> String {
    let stress: String = sound.chars().filter(char::is_ascii_digit).collect();
    format!("{}{stress}", plain(sound))
}

impl Rules {
    pub fn built_in() -> Result<Self> {
        Self::parse(BUILT_IN)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self> {
        let raw: Raw = toml::from_str(text)?;
        if raw.onset.is_empty() || raw.vowel.is_empty() {
            bail!("the rules need at least [onset] and [vowel]");
        }
        let onset = strokes(raw.onset, "onset")?;
        let coda = strokes(raw.coda, "coda")?;
        let vowel = strokes(raw.vowel, "vowel")?;
        let vowel_coda = strokes(raw.vowel_coda, "vowel_coda")?;
        let onset_cluster = strokes(raw.onset_cluster, "onset_cluster")?;
        let coda_cluster = strokes(raw.coda_cluster, "coda_cluster")?;

        let mut onset_rev = invert(&onset);
        onset_rev.extend(invert(&onset_cluster));
        let mut coda_rev = invert(&coda);
        coda_rev.extend(invert(&coda_cluster));
        // When two labels share the same vowel keys, deterministically pick which one wins the
        // reverse reading: see `vowel_rank`.
        let mut vowel_pairs: Vec<(Stroke, String)> =
            vowel.iter().map(|(s, &k)| (k, s.clone())).collect();
        vowel_pairs.sort_by(|a, b| vowel_rank(&a.1).cmp(&vowel_rank(&b.1)));
        let vowel_rev: HashMap<Stroke, String> = vowel_pairs.into_iter().collect();
        let mut vowel_coda_rev: Vec<(Stroke, String, String)> = vowel_coda
            .iter()
            .filter_map(|(sounds, &stroke)| {
                let (vowel, coda) = sounds.split_once(' ')?;
                Some((stroke, vowel.to_owned(), coda.to_owned()))
            })
            .collect();
        vowel_coda_rev.sort_by(|a, b| (&a.1, &a.2).cmp(&(&b.1, &b.2)));
        let spelling = spelling_table(BUILT_IN_SPELLING)?;
        let suffixes = single_stroke_suffixes(BUILT_IN_SUFFIXES)?;
        let two_stroke_suffixes = two_stroke_suffixes(BUILT_IN_SUFFIXES)?;

        Ok(Self {
            onset,
            coda,
            vowel,
            vowel_coda,
            onset_cluster,
            coda_cluster,
            onset_rev,
            coda_rev,
            vowel_rev,
            vowel_coda_rev,
            spelling,
            suffixes,
            two_stroke_suffixes,
            onsets: raw.syllables.onsets.into_iter().collect(),
            lax: raw.syllables.lax.into_iter().collect(),
            spelled: raw.spelled,
            reduce: raw.reduce.vowels.into_iter().collect(),
            keep: raw.reduce.except.into_iter().collect(),
            lone: raw.reduce.lone.into_iter().collect(),
            loose: raw.reduce.loose.into_iter().collect(),
            fold_any: match raw.reduce.order.as_deref() {
                None | Some("any") => true,
                Some("strict") => false,
                Some(other) => bail!("[reduce] order = {other:?}: use \"any\" or \"strict\""),
            },
            prefixes: raw
                .prefix
                .into_iter()
                .map(|p| {
                    let keys = Stroke::parse(&p.keys)
                        .with_context(|| format!("[[prefix]] {} keys {:?}", p.spelling, p.keys))?;
                    Ok((p, keys))
                })
                .collect::<Result<_>>()?,
            join_key: raw.theory.join_key,
            brevity: raw
                .brevity
                .into_iter()
                .map(|b| {
                    Ok(Shortening {
                        step: b.step,
                        rank: b.rank,
                        vowel: b.vowel,
                        min_syllables: b.min_syllables,
                        any: match b.order.as_deref() {
                            None | Some("strict") => false,
                            Some("any") => true,
                            Some(other) => {
                                bail!("[[brevity]] order = {other:?}: use \"any\" or \"strict\"")
                            }
                        },
                    })
                })
                .collect::<Result<_>>()?,
        })
    }

    /// The keys for a run of consonants, one sound after another (or a cluster rule for a run of
    /// them). The keys have to come in steno order, with no overlap: otherwise it can't be
    /// written in one stroke by these rules and this gives `None`.
    ///
    /// With `fold`, the keys may come in any order as long as none is used twice: that is how a
    /// reduced syllable's consonants are added to the stroke before it.
    fn consonants(
        sounds: &[String],
        single: &HashMap<String, Stroke>,
        clusters: &HashMap<String, Stroke>,
        fold: bool,
    ) -> Option<Stroke> {
        let mut bits = 0u32;
        let mut at = 0;
        while at < sounds.len() {
            let (stroke, used) = Self::next_run(sounds, at, single, clusters)?;
            let highest = Stroke(bits).highest();
            // Only a star may go anywhere: it belongs to the vowels
            let keys = Stroke(stroke.0 & !Stroke::STAR);
            if fold {
                if bits & keys.0 != 0 {
                    return None;
                }
            } else if let (Some(highest), Some(lowest)) = (highest, keys.lowest())
                && lowest <= highest
            {
                return None;
            }
            bits |= stroke.0;
            at += used;
        }
        Some(Stroke(bits))
    }

    /// [`Self::consonants`] where a sound may share keys already down, and may come out of steno
    /// order: a sound whose keys are all down already adds nothing (*m-p-r* is `PHR`, the *p* is
    /// inside the `PH` of the *m*), and a sound whose new keys fall before the ones down is out of
    /// order, which is allowed up to [`MAX_OUT_OF_ORDER`] times and counted, for the caller to price.
    /// A chord is a set of keys, so order never stops a stroke being struck, only from reading as
    /// the sounds in the order they were said.
    fn consonants_shared(
        sounds: &[String],
        single: &HashMap<String, Stroke>,
        clusters: &HashMap<String, Stroke>,
    ) -> Option<(Stroke, u32)> {
        let mut bits = 0u32;
        let mut out_of_order = 0;
        let mut at = 0;
        while at < sounds.len() {
            let (stroke, used) = Self::next_run(sounds, at, single, clusters)?;
            let keys = Stroke(stroke.0 & !Stroke::STAR);
            let new = Stroke(keys.0 & !bits);
            if let (Some(highest), Some(lowest)) = (Stroke(bits).highest(), new.lowest())
                && lowest <= highest
            {
                out_of_order += 1;
                if out_of_order > MAX_OUT_OF_ORDER {
                    return None;
                }
            }
            bits |= stroke.0;
            at += used;
        }
        Some((Stroke(bits), out_of_order))
    }

    /// The keys of the cluster (longest first) or the single sound at `at`, and how many sounds
    /// they cover.
    fn next_run(
        sounds: &[String],
        at: usize,
        single: &HashMap<String, Stroke>,
        clusters: &HashMap<String, Stroke>,
    ) -> Option<(Stroke, usize)> {
        (2..=sounds.len() - at)
            .rev()
            .find_map(|n| {
                let run = &sounds[at..at + n];
                let labelled = run.join(" ");
                let bare = run.iter().map(|s| plain(s)).collect::<Vec<_>>().join(" ");
                clusters.get(&labelled).or_else(|| clusters.get(&bare)).map(|&s| (s, n))
            })
            .or_else(|| single.get(&sounds[at]).or_else(|| single.get(plain(&sounds[at]))).map(|&s| (s, 1)))
    }

    /// The keys that write one syllable, if one stroke can.
    pub fn syllable_stroke(&self, syllable: &Syllable) -> Option<Stroke> {
        self.stroke_for(syllable, false)
    }

    fn stroke_for(&self, syllable: &Syllable, fold: bool) -> Option<Stroke> {
        self.stroke_with(syllable, fold, false).map(|(stroke, _)| stroke)
    }

    /// [`Self::stroke_for`], with consonants that may share keys and fall out of order (see
    /// [`Self::consonants_shared`]) when `share`; also the number of sounds out of order.
    fn stroke_with(&self, syllable: &Syllable, fold: bool, share: bool) -> Option<(Stroke, u32)> {
        let run = |sounds: &[String], single, clusters, fold| {
            if share {
                Self::consonants_shared(sounds, single, clusters)
            } else {
                Self::consonants(sounds, single, clusters, fold).map(|stroke| (stroke, 0))
            }
        };
        let (onset, onset_out) = run(&syllable.onset, &self.onset, &self.onset_cluster, false)?;
        let base = syllable
            .vowel
            .trim_end_matches(|c: char| c.is_ascii_digit());
        // A continuation stroke that became the stem of the next fold has no vowel
        let plain_vowel = if syllable.vowel.is_empty() {
            Some(Stroke(0))
        } else {
            self.vowel
                .get(&syllable.vowel)
                .or_else(|| self.vowel.get(base))
                .or_else(|| self.vowel.get(plain(base)))
                .copied()
        };
        // The vowel and the consonant after it together (`AR`, `OPBG`) when the rules have that,
        // and the rest of the consonants fit after it; otherwise the vowel on its own
        let joined = syllable.coda.first().and_then(|first| {
            self.vowel_coda
                .get(&format!("{base} {first}"))
                .or_else(|| {
                    self.vowel_coda
                        .get(&format!("{} {}", plain(base), plain(first)))
                })
                .map(|&v| (v, &syllable.coda[1..]))
        });
        let rest = &syllable.coda[..];
        let with = |vowel: Stroke, coda: &[String]| -> Option<(Stroke, u32)> {
            // A vowel with an `R` of its own (`ER`, `AR`) and a cluster that has the `R` in it
            // too, as in `serve` -FRB, `march` -FRPB or `harsh` -RB, share the key; any other
            // key used twice does not fit, and nor does a second `R` sound (`harder`)
            let shared = if coda.iter().any(|s| plain(s) == "R") {
                0
            } else {
                R_KEY
            };
            let (coda, coda_out) = run(coda, &self.coda, &self.coda_cluster, fold)?;
            if vowel.0 & coda.0 & Stroke::RIGHT & !shared != 0 {
                return None;
            }
            if vowel.0 & coda.0 & Stroke::STAR != 0 {
                return None;
            }
            Some((Stroke(onset.0 | vowel.0 | coda.0), onset_out + coda_out))
        };
        joined
            .and_then(|(vowel, coda)| with(vowel, coda))
            .or_else(|| with(plain_vowel?, rest))
    }

    /// Give the sounds of a word their spelling variants: how the word is spelled can decide
    /// which keys a sound goes on.
    pub fn label(&self, word: &str, sounds: &[String]) -> Vec<String> {
        self.label_with(word, sounds, false)
    }

    /// [`Self::label`], also using the rules that only tell apart words that sound the same.
    fn label_with(&self, word: &str, sounds: &[String], clash: bool) -> Vec<String> {
        let is_vowel = |s: &String| s.ends_with(|c: char| c.is_ascii_digit());
        let syllables = sounds.iter().filter(|s| is_vowel(s)).count();
        let first_vowel = sounds.iter().position(is_vowel).unwrap_or(usize::MAX);
        sounds
            .iter()
            .enumerate()
            .map(|(at, sound)| {
                let stress: String = sound.chars().filter(char::is_ascii_digit).collect();
                let rule = self.spelled.iter().find(|r| {
                    (clash || !r.clash)
                        && r.sound == plain(sound)
                        && r.applies(word, sounds, at, syllables, first_vowel)
                });
                rule.map_or_else(|| sound.clone(), |r| format!("{}{stress}", r.label))
            })
            .collect()
    }

    /// Split a word's sounds into syllables. Between vowels, the longest run of consonants that
    /// can start a syllable goes to the next one, except that a stressed short vowel keeps a single
    /// consonant after it.
    pub fn syllables(&self, sounds: &[String]) -> Option<Vec<Syllable>> {
        self.syllables_split(sounds, true, None)
    }

    /// Every other place the consonants between two vowels can be broken, one boundary at a time:
    /// the boundary's number (counting vowels from the first) and how many of its consonants start
    /// the next syllable. Any run is a valid break, and none at all (`co-mpro`, `com-pro`, `comp-ro`,
    /// `compr-o`): the join key means no break has to be the only one. Whether a break gives strokes
    /// that can be struck is decided later, when they are built.
    fn boundary_breaks(sounds: &[String]) -> Vec<(usize, usize)> {
        let vowels: Vec<usize> = (0..sounds.len()).filter(|&i| sounds[i].ends_with(|c: char| c.is_ascii_digit())).collect();
        let mut breaks = Vec::new();
        for (n, pair) in vowels.windows(2).enumerate() {
            let cluster = &sounds[pair[0] + 1..pair[1]];
            if sounds[pair[1]].starts_with("IY.ly") {
                continue;
            }
            // Any run at all: whether the strokes can be built decides, not English
            breaks.extend((0..=cluster.len()).map(|take| (n, take)));
        }
        breaks
    }

    /// [`Self::syllables`], with the stressed short vowel keeping its consonant (`lem-on`) or not
    /// (`le-mon`): both are how somebody might hear the word, so both are offered as join paths.
    /// `shift` overrides one boundary (see [`Self::boundary_breaks`]).
    fn syllables_split(&self, sounds: &[String], keep_lax: bool, shift: Option<(usize, usize)>) -> Option<Vec<Syllable>> {
        let is_vowel = |s: &str| s.ends_with(|c: char| c.is_ascii_digit());
        let vowels: Vec<usize> = (0..sounds.len())
            .filter(|&i| is_vowel(&sounds[i]))
            .collect();
        let (&first, &last) = (vowels.first()?, vowels.last()?);
        let mut result: Vec<Syllable> = Vec::new();
        let mut onset: Vec<String> = sounds[..first].to_vec();
        for (n, &v) in vowels.iter().enumerate() {
            let mut syllable = Syllable {
                onset: std::mem::take(&mut onset),
                vowel: sounds[v].clone(),
                coda: Vec::new(),
            };
            if v == last {
                syllable.coda = sounds[v + 1..].to_vec();
            } else {
                let cluster = &sounds[v + 1..vowels[n + 1]];
                let next = sounds[vowels[n + 1]].trim_end_matches(|c: char| c.is_ascii_digit());
                // The `-ly` ending takes its L and a stop before it (`sim-ply`, `as-sem-bly`),
                // never an S: `ous-ly`, not `ou-sly`
                let take = if next == "IY.ly" {
                    let ends_in_l = cluster.last().is_some_and(|s| plain(s) == "L");
                    let stressed = syllable.vowel.ends_with(['1', '2']);
                    let keeps = stressed && self.lax.contains(plain(&syllable.vowel));
                    // `ug-ly`: a stressed short vowel keeps its consonant here too
                    let two = cluster.len() >= 2 && !(keeps && cluster.len() == 2) && {
                        let pair = &cluster[cluster.len() - 2..];
                        plain(&pair[0]) != "S"
                            && self.onsets.contains(&format!("{} L", plain(&pair[0])))
                    };
                    match (ends_in_l, two) {
                        (true, true) => 2,
                        (true, false) => 1,
                        _ => 0,
                    }
                } else {
                    self.onset_length(&syllable.vowel, cluster, keep_lax)
                };
                let take = match shift {
                    Some((boundary, takes)) if boundary == n => takes,
                    _ => take,
                };
                let split = cluster.len() - take;
                syllable.coda = cluster[..split].to_vec();
                onset = cluster[split..].to_vec();
            }
            result.push(syllable);
        }
        Some(result)
    }

    /// How many of the consonants between two vowels start the second syllable.
    fn onset_length(&self, vowel: &str, cluster: &[String], keep_lax: bool) -> usize {
        // A stressed short vowel keeps a consonant after it: `bet-ter`, `ev-ry`, not `e-vry`
        let stressed = vowel.ends_with(['1', '2']);
        let keep = usize::from(keep_lax && stressed && self.lax.contains(plain(vowel)));
        (1..=cluster.len().saturating_sub(keep))
            .rev()
            .find(|&n| {
                let run: Vec<&str> = cluster[cluster.len() - n..]
                    .iter()
                    .map(|s| plain(s))
                    .collect();
                self.onsets.contains(&run.join(" "))
            })
            .unwrap_or(0)
    }

    /// Whether strokes after the first of a word carry the join key.
    pub const fn join_key(&self) -> bool {
        self.join_key
    }

    /// Whether a syllable is one whose vowel is not written.
    fn is_reduced(&self, syllable: &Syllable) -> bool {
        let label = syllable
            .vowel
            .trim_end_matches(|c: char| c.is_ascii_digit());
        syllable.vowel.ends_with('0')
            && !self.keep.contains(label)
            && (syllable.onset.len() == 1 || !self.lone.contains(label))
            && (self.reduce.contains(label) || self.reduce.contains(plain(label)))
    }

    /// A reduced syllable that did not fit on the stroke before it: a continuation stroke of just
    /// its consonants (`-G` for `-ing`, `-D` for `-ed`, `-PB` for `-en`). A reduced `ER` is written
    /// as its `R` (`-R`), and a syllable with no consonants keeps its vowel.
    fn continuation(&self, syllable: &Syllable) -> Option<Stroke> {
        let is_er = plain(&syllable.vowel) == "ER";
        let consonants = syllable.onset.len() + syllable.coda.len();
        if consonants == 0 && !is_er {
            return self.syllable_stroke(syllable);
        }
        // A suffix is written on the right hand with no vowel (`-ness` -PBS, `-ment` -PLT, `-er`
        // -R); when its consonants don't fit there in order, the onset goes on the left and the
        // rest stays on the right (`-nial` TPH-L, `-ner` TPH-R). The `R` of a reduced `ER` is part
        // of that rest: `examiner`'s "-ner" is `TPH-R`, never a bare `TPH` that reads as "n".
        let mut rest: Vec<String> = Vec::new();
        if is_er {
            rest.push("R".into());
        }
        rest.extend(syllable.coda.iter().cloned());
        let mut all = syllable.onset.clone();
        all.extend(rest.iter().cloned());
        if let Some(coda) = Self::consonants(&all, &self.coda, &self.coda_cluster, false) {
            return Some(coda);
        }
        let onset = Self::consonants(&syllable.onset, &self.onset, &self.onset_cluster, false)?;
        let coda = Self::consonants(&rest, &self.coda, &self.coda_cluster, false)?;
        Some(Stroke(onset.0 | coda.0))
    }

    /// Whether a word's own last syllable is a reduced syllable that could be struck as a
    /// standalone continuation stroke on its own — the same split [`Self::strokes_for`] already
    /// falls back to when folding it onto one stroke doesn't fit (`ended` EPBD/+-D, `breaking`
    /// PWRAEUBG/+-G), computed here unconditionally instead of only when that fold fails. `None`
    /// for a one-syllable word (nothing to split), when the last syllable isn't reduced, or when
    /// it has no bare continuation stroke of its own.
    ///
    /// Returns the index into `sounds` where the stem ends (the last syllable's own onset, if it
    /// has one, counts as part of the stem: a standalone suffix never has an onset of its own,
    /// the same convention `rules/suffixes.toml` already uses, so those sounds belong to
    /// whichever word they'd actually be pronounced with) and the bare (no join key) stroke the
    /// trailing syllable's vowel and coda alone would be struck as.
    pub fn suffix_split(&self, word: &str, sounds: &[String]) -> Option<(usize, Stroke)> {
        let labelled = self.label(word, sounds);
        let syllables = self.syllables(&labelled)?;
        if syllables.len() < 2 {
            return None;
        }
        let last = syllables.last()?;
        if !self.is_reduced(last) {
            return None;
        }
        let suffix = Syllable {
            onset: Vec::new(),
            vowel: last.vowel.clone(),
            coda: last.coda.clone(),
        };
        let stroke = self.continuation(&suffix)?;
        Some((sounds.len() - 1 - last.coda.len(), stroke))
    }

    /// Whether `word` ends in one of `rules/suffixes.toml`'s two-stroke suffixes (`-ism`,
    /// `-ary`/`-ery`/`-ory`, `-ical`, `-logy`, `-cy`/`-sy`, `-ify`/`-fy`) - found by spelling
    /// (`word` ends with the suffix's own letters), unlike [`Self::suffix_split`]'s sound-driven
    /// single-stroke version. That difference is deliberate, not a shortcut: a two-syllable
    /// suffix's second syllable sometimes has its onset consonant migrate backward onto the
    /// *first* stroke rather than staying on the second the way plain syllable rules would place
    /// it (`-ism`'s own `+EUZ/+-PL` ends its first stroke in the `z` that ordinary onset
    /// assignment would otherwise give the second syllable) - `rules/suffixes.toml`'s own chord is
    /// already correct for this, hand-verified against real words, so it is reused as-is rather
    /// than re-derived and risking getting that migration wrong. The longest matching suffix wins
    /// when spelling matches more than one (so a word ending in both a shorter and a longer
    /// registered suffix is read by whichever one actually describes it). `None` if nothing
    /// matches, or a match would consume the whole word (leaving no stem).
    #[must_use]
    pub fn two_stroke_suffix(&self, word: &str) -> Option<(&str, &[Stroke])> {
        self.two_stroke_suffixes
            .iter()
            .filter(|(text, _)| word.len() > text.len() && word.ends_with(text.as_str()))
            .max_by_key(|(text, _)| text.len())
            .map(|(text, strokes)| (text.as_str(), strokes.as_slice()))
    }

    /// The keys one or more leading consonant sounds take through the onset tables, for a prefix
    /// found by an exact match against another already-placed word's own trailing pronunciation
    /// (`never`'s leading "N" in front of `ever`'s own exact sounds) rather than through syllable
    /// analysis the way [`Self::suffix_split`] finds a trailing one. The caller already knows
    /// where the split falls; this only answers what the leading sounds themselves would be
    /// struck as. `None` for no leading sounds at all, or when they don't fit together in one
    /// stroke's own steno order.
    pub fn prefix_keys(&self, word: &str, sounds: &[String], split: usize) -> Option<Stroke> {
        if split == 0 {
            return None;
        }
        let labelled = self.label(word, sounds);
        Self::consonants(&labelled[..split], &self.onset, &self.onset_cluster, false)
    }

    /// The prefix a word starts with, if the rules write one with fixed keys: the keys and how
    /// many sounds they stand for.
    fn prefix(&self, word: &str, sounds: &[String]) -> Option<(Stroke, usize)> {
        self.prefixes.iter().find_map(|(prefix, keys)| {
            if !word.starts_with(prefix.spelling.as_str()) {
                return None;
            }
            prefix.sounds.iter().find_map(|run| {
                let run: Vec<&str> = run.split_whitespace().collect();
                // With a vowel left after it: `disk` is not `dis-` and a `K`
                let matches = run.len() < sounds.len()
                    && run.iter().zip(sounds).all(|(a, b)| *a == unlabelled(b))
                    && sounds[run.len()..]
                        .iter()
                        .any(|s| s.ends_with(|c: char| c.is_ascii_digit()));
                matches.then_some((*keys, run.len()))
            })
        })
    }

    /// The strokes for a word. A reduced syllable's consonants join the stroke before it when
    /// they fit; every other syllable is a stroke of its own. `None` if some syllable can't be
    /// written in a stroke by these rules.
    pub fn outline(&self, word: &str, sounds: &[String]) -> Option<Vec<Stroke>> {
        self.outline_with(word, sounds, false)
    }

    /// How the spelling tells this word from others that sound the same: the outline it takes
    /// when it has to, if that differs from its usual one.
    pub fn variant(&self, word: &str, sounds: &[String]) -> Option<Vec<Stroke>> {
        let alternative = self.outline_with(word, sounds, true)?;
        (Some(&alternative) != self.outline(word, sounds).as_ref()).then_some(alternative)
    }

    fn outline_with(&self, word: &str, sounds: &[String], clash: bool) -> Option<Vec<Stroke>> {
        self.outline_folded(word, sounds, clash, self.fold_any)
    }

    /// The outline with the folded consonants allowed out of steno order (`any`) or not.
    fn outline_folded(
        &self,
        word: &str,
        sounds: &[String],
        clash: bool,
        any: bool,
    ) -> Option<Vec<Stroke>> {
        let labelled = self.label_with(word, sounds, clash);
        let (prefix, skip) = self.prefix(word, &labelled).unzip();
        let syllables = self.syllables(&labelled[skip.unwrap_or(0)..])?;
        let mut strokes = self.strokes_for(&syllables, any)?;
        if let Some(prefix) = prefix {
            // The prefix keys join the first stroke's own left-hand keys, sharing any they both
            // use: `KP` + `PEBGT` is `KPEBGT`, `STK` + `TAPBS` is `STKAPBS`
            strokes.first_mut()?.0 |= prefix.0;
        }
        Some(strokes)
    }

    /// Every way to write a word one syllable per stroke, for the join paths every word of two
    /// syllables or more gets beside its primary (see `docs/syllables.md`). First the word's own
    /// folded outline ([`Self::outline`]); then, for both syllable splits (`lem-on`, `le-mon`) and
    /// with the fixed prefix both folded onto the first stroke and struck as a syllable of its own,
    /// two writings: `suffixed`, where a reduced *last* syllable is its continuation stroke with the
    /// vowel dropped, exactly as a suffix is (*table* `TAEU/+-BL`), and `voiced`, where every
    /// syllable keeps its vowel (*table* `TAEU/+PWUL`). An unstressed schwa in a syllable that
    /// keeps its vowel is `U`, and an unstressed short i `EU`, whatever letter spells it: the sound is
    /// what a learner hears; and again with the vowel the spelling gives it (*compromise* `KOPL/PRO/PHAOEUZ`,
    /// the way Plover writes it). Only outlines of two strokes or more, without
    /// repeats; the join key is not yet set.
    pub fn syllable_paths(&self, word: &str, sounds: &[String]) -> Vec<Vec<Stroke>> {
        let spelled = self.label(word, sounds);
        // The same sounds with an unstressed schwa or short i stripped of the letter spelling it
        let heard: Vec<String> = spelled
            .iter()
            .map(|s| if matches!(unlabelled(s).as_str(), "AH0" | "IH0") { unlabelled(s) } else { s.clone() })
            .collect();
        let mut out: Vec<Vec<Stroke>> = self.outline(word, sounds).into_iter().filter(|o| o.len() >= 2).collect();
        let mut starts: Vec<(Option<Stroke>, usize)> = vec![(None, 0)];
        if let Some((keys, skip)) = self.prefix(word, &heard) {
            starts.push((Some(keys), skip));
        }
        for labelled in [&heard, &spelled] {
            for &(prefix, skip) in &starts {
                let rest = &labelled[skip..];
                let splits = [(true, None), (false, None)]
                    .into_iter()
                    .chain(Self::boundary_breaks(rest).into_iter().map(|b| (true, Some(b))));
                for (keep_lax, shift) in splits {
                    let Some(syllables) = self.syllables_split(rest, keep_lax, shift) else { continue };
                    for mut strokes in self.syllable_writings(&syllables) {
                        if let Some(keys) = prefix {
                            // Shared keys are shared, as in `outline_folded`: `STK` + `TAPBS`
                            strokes.first_mut().expect("a syllable split is never empty").0 |= keys.0;
                        }
                        // A syllable with nothing to strike (a dropped vowel and no consonant) is no stroke
                        if strokes.len() >= 2 && strokes.iter().all(|s| s.0 != 0) && !out.contains(&strokes) {
                            out.push(strokes);
                        }
                    }
                }
            }
        }
        out
    }

    /// The two ways to write a syllable split one stroke each: `suffixed`, where a reduced *last*
    /// syllable is its continuation stroke with the vowel dropped, exactly as a suffix is, and
    /// `voiced`, where every syllable keeps its vowel.
    fn syllable_writings(&self, syllables: &[Syllable]) -> Vec<Vec<Stroke>> {
        let last = syllables.len() - 1;
        let suffixed: Option<Vec<Stroke>> = syllables
            .iter()
            .enumerate()
            .map(|(i, s)| if i == last && i > 0 && self.is_reduced(s) { self.continuation(s) } else { self.syllable_stroke(s) })
            .collect();
        let voiced: Option<Vec<Stroke>> =
            syllables.iter().map(|s| self.syllable_stroke(s).or_else(|| self.continuation(s))).collect();
        [suffixed, voiced].into_iter().flatten().collect()
    }

    /// The strokes for a run of syllables, folding reduced ones onto the stroke before.
    fn strokes_for(&self, syllables: &[Syllable], any: bool) -> Option<Vec<Stroke>> {
        let mut strokes: Vec<Stroke> = Vec::new();
        let mut previous: Option<Syllable> = None;
        for syllable in syllables.iter().cloned() {
            if let Some(before) = &previous
                && self.is_reduced(&syllable)
            {
                let mut merged = before.clone();
                let mut added: Vec<String> = syllable.onset.clone();
                if plain(&syllable.vowel) == "ER" {
                    added.push("R".into());
                }
                added.extend(syllable.coda.iter().cloned());
                // Some consonants may land out of order (Plover's -RT for `-ter`, -LT for `-tly`)
                let loose = added.iter().all(|s| self.loose.contains(plain(s)));
                merged.coda.extend(added);
                if let Some(stroke) = self.stroke_for(&merged, any || loose) {
                    *strokes.last_mut()? = stroke;
                    previous = Some(merged);
                    continue;
                }
                // The syllable's first consonants belong to the stem and the rest is a suffix
                // stroke: `ended` EPBD/-D, `breaking` PWRAEUBG/-G, `artist` ART/-FT
                let mut stem = before.clone();
                stem.coda.extend(syllable.onset.iter().cloned());
                let suffix = Syllable {
                    onset: Vec::new(),
                    vowel: syllable.vowel.clone(),
                    coda: syllable.coda.clone(),
                };
                if !syllable.onset.is_empty()
                    && let Some(stroke) = self.stroke_for(&stem, any)
                    && let Some(rest) = self.continuation(&suffix)
                {
                    *strokes.last_mut()? = stroke;
                    strokes.push(rest);
                    previous = Some(vowelless(suffix, rest));
                    continue;
                }
                let rest = self.continuation(&syllable)?;
                strokes.push(rest);
                previous = Some(vowelless(syllable, rest));
                continue;
            }
            strokes.push(self.syllable_stroke(&syllable)?);
            previous = Some(syllable);
        }
        Some(strokes)
    }

    /// Mark the strokes after the first of a word with the join key, if the theory has one.
    pub fn joined(&self, outline: &mut [Stroke]) {
        if self.join_key {
            for stroke in outline.iter_mut().skip(1) {
                stroke.0 |= Stroke::JOIN;
            }
        }
    }

}

#[cfg(test)]
mod tests;
