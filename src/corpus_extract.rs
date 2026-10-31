//! Hand-rolled extraction of short, plain-prose sentences from a downloaded Project Gutenberg
//! plain-text book, for `quote_corpus`'s larger, non-hand-picked benchmark corpus (see
//! `src/fetch.rs` for where the books come from and `docs/decisions.md` for why). Deliberately
//! simple - paragraph-level junk filtering plus an abbreviation-aware sentence splitter, not a
//! real NLP pipeline. A handful of odd sentences slipping through, or a handful of clean ones
//! getting dropped, doesn't matter here: this corpus is a wide statistical check over hundreds of
//! passages nobody hand-picked, not an individually curated list the way
//! `quote_corpus::PASSAGES` is.

/// Trailing-period abbreviations that must never be read as a sentence boundary, lowercased for
/// a case-insensitive match.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "st", "jr", "sr", "mt", "vs", "etc", "co", "capt", "col", "gen",
    "prof", "rev", "hon", "messrs", "mme", "mlle", "no",
];

/// Closing punctuation that can trail a sentence-ending mark (a closing quote after the period,
/// for instance) without itself mattering for where the sentence actually ends.
const TRAILING_CLOSERS: &[char] = &['"', '\'', ')', '\u{201d}', '\u{2019}'];

/// Keeps only the text between Project Gutenberg's standard `*** START OF ... ***` and
/// `*** END OF ... ***` marker lines - every modern Gutenberg release uses them verbatim, and
/// everything outside them is boilerplate (license text, table of contents, transcriber notes)
/// rather than the book itself.
pub fn strip_boilerplate(raw: &str) -> &str {
    let after_start = raw
        .find("*** START OF")
        .and_then(|i| raw[i..].find('\n').map(|n| i + n + 1))
        .unwrap_or(0);
    let before_end = raw.find("*** END OF").unwrap_or(raw.len());
    if after_start < before_end {
        &raw[after_start..before_end]
    } else {
        raw
    }
}

/// Joins each blank-line-delimited paragraph's wrapped lines into one string, dropping anything
/// that reads as structure rather than prose. A real paragraph of prose always has a lowercase
/// letter in it; an all-caps chapter heading, a roman-numeral section marker, a decorative rule,
/// or an `[Illustration: ...]` caption does not (or carries a `[`/`_` marker instead).
fn paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.retain(|p| p.chars().any(|c| c.is_ascii_lowercase()) && !p.contains('[') && !p.contains('_'));
    out
}

/// Splits one paragraph into sentences on `.`/`!`/`?` followed by whitespace (or the end of the
/// paragraph), skipping a `.` that follows a known abbreviation or a single capital-letter
/// initial, so "Mr. Darcy walked in." and "J. Austen wrote this." don't get cut in half.
fn split_sentences(paragraph: &str) -> Vec<String> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            let mut j = i + 1;
            while j < chars.len() && TRAILING_CLOSERS.contains(&chars[j]) {
                current.push(chars[j]);
                j += 1;
            }
            let at_boundary = j >= chars.len() || chars[j].is_whitespace();
            let last_word = current
                .trim_end_matches(|ch: char| !ch.is_alphabetic())
                .rsplit(|ch: char| ch.is_whitespace())
                .next()
                .unwrap_or("");
            let is_abbreviation = c == '.'
                && (last_word.chars().count() == 1
                    && last_word.chars().next().is_some_and(char::is_uppercase)
                    || ABBREVIATIONS.iter().any(|a| a.eq_ignore_ascii_case(last_word)));
            if at_boundary && !is_abbreviation {
                let done = current.trim();
                if !done.is_empty() {
                    sentences.push(done.to_string());
                }
                current.clear();
            }
            i = j;
            continue;
        }
        i += 1;
    }
    let tail = current.trim();
    if !tail.is_empty() {
        sentences.push(tail.to_string());
    }
    sentences
}

/// Both of 19th-century prose's own hyphen conventions - an em dash written as two bare hyphens
/// with no surrounding space (`word--word`), and hyphenated multi-word phrases this era writes far
/// more freely than modern English (`four-and-twenty`, `bread-and-cheese`, `printing-office`) -
/// read as a single whitespace-delimited token if left alone. The scoring pipeline's own word
/// cleanup (`quote::clean`) strips the punctuation but keeps the letters either way, fusing two or
/// more unrelated real words into one nonsense word that costs a theory an expensive fallback
/// outline for a "word" that doesn't exist, while the other theories simply have no entry for it
/// either and are silently excluded from that comparison - unfairly inflating one side's aggregate
/// key total over a formatting quirk, not a real difference in terseness. Steno handles a
/// hyphenated phrase as separate strokes for its separate words anyway (this project's whole join-
/// key design is built around explicit word boundaries, see `docs/join-key.md`), so splitting every
/// hyphen and dash into its own space-separated word is the fair, representative choice here, not
/// a hack: it's what typing this phrase actually looks like.
fn normalize_dashes(text: &str) -> String {
    text.chars()
        .map(|c| if matches!(c, '-' | '\u{2014}' | '\u{2013}') { ' ' } else { c })
        .collect()
}

/// Collapses internal whitespace (line-wrap joins can leave doubled spaces) and keeps a sentence
/// only if it's 8-20 words of ordinary prose: no digits, brackets, or underscores left over from
/// chapter numbers, footnote markers, or italics formatting.
fn clean_sentence(raw: &str) -> Option<String> {
    let text = normalize_dashes(raw).split_whitespace().collect::<Vec<_>>().join(" ");
    let word_count = text.split_whitespace().count();
    if !(8..=20).contains(&word_count) {
        return None;
    }
    let allowed_punctuation: &[char] = &[
        '.', ',', '!', '?', ';', ':', '\'', '"', '-', '\u{2014}', '\u{2018}', '\u{2019}',
        '\u{201c}', '\u{201d}',
    ];
    let clean = text
        .chars()
        .all(|c| c.is_alphabetic() || c.is_whitespace() || allowed_punctuation.contains(&c));
    clean.then_some(text)
}

/// Every 8-20 word prose sentence this book yields, in reading order.
pub fn extract(raw: &str) -> Vec<String> {
    paragraphs(strip_boilerplate(raw))
        .iter()
        .flat_map(|p| split_sentences(p))
        .filter_map(|s| clean_sentence(&s))
        .collect()
}

/// Deterministically thins a combined list of extracted sentences down to roughly `target` by
/// taking every Nth one (a fixed stride, not a random sample) - so the exact passages a
/// `cargo test` run sees never depend on process-specific randomness, and a re-run against the
/// same downloaded books always reproduces the same corpus.
pub fn sample(sentences: &[String], target: usize) -> Vec<String> {
    if target == 0 || sentences.len() <= target {
        return sentences.to_vec();
    }
    let stride = (sentences.len() / target).max(1);
    sentences.iter().step_by(stride).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_gutenberg_boilerplate() {
        let raw = "preamble\n*** START OF THE PROJECT GUTENBERG EBOOK FOO ***\nbody text here\n*** END OF THE PROJECT GUTENBERG EBOOK FOO ***\nlicense junk";
        assert_eq!(strip_boilerplate(raw).trim(), "body text here");
    }

    #[test]
    fn keeps_abbreviation_periods_inside_one_sentence() {
        let sentences = split_sentences(
            "Mr. Darcy walked into the quiet room and said nothing at all to anyone there.",
        );
        assert_eq!(sentences.len(), 1);
    }

    #[test]
    fn splits_on_real_sentence_boundaries() {
        let sentences = split_sentences("This is one sentence. This is another one here.");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn drops_headings_and_illustration_captions() {
        let text = "CHAPTER ONE\n\n[Illustration: a house]\n\nShe walked quietly down the long, quiet hallway toward the door.";
        let paras = paragraphs(text);
        assert_eq!(paras, vec![
            "She walked quietly down the long, quiet hallway toward the door.".to_string()
        ]);
    }

    #[test]
    fn filters_by_word_count() {
        assert!(clean_sentence("Too short.").is_none());
        assert!(
            clean_sentence(
                "This sentence has exactly enough words in it to pass the filter cleanly today."
            )
            .is_some()
        );
    }

    #[test]
    fn sample_is_deterministic_and_respects_target() {
        let sentences: Vec<String> = (0..1000).map(|i| i.to_string()).collect();
        let a = sample(&sentences, 400);
        let b = sample(&sentences, 400);
        assert_eq!(a, b);
        assert!(a.len() <= 500 && a.len() >= 300);
    }
}
