//! Where the units of a feed come from. Each loader returns plain units, never outlines.

use super::feed::is_plain_word;
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::Path;

/// The lowercase form of every word Plover and Lapwing only ever write capitalized (names such as
/// `ben`), which the subtitle counts rank among ordinary words. Empty if their files are missing.
#[must_use]
pub fn names_in(data: &Path) -> HashSet<String> {
    let mut lower: HashSet<String> = HashSet::new();
    let mut capitalized: HashSet<String> = HashSet::new();
    for file in ["plover-main.json", "lapwing-base.json"] {
        let Ok(text) = std::fs::read_to_string(data.join("dictionaries").join(file)) else { continue };
        let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&text) else { continue };
        for word in map.into_values() {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) if c.is_ascii_uppercase() && chars.clone().next().is_some() && chars.all(|c| c.is_ascii_lowercase()) => {
                    capitalized.insert(word.to_ascii_lowercase());
                }
                Some(_) if word.chars().all(|c| c.is_ascii_lowercase()) => {
                    lower.insert(word);
                }
                _ => {}
            }
        }
    }
    capitalized.retain(|w| !lower.contains(w));
    capitalized
}

/// The commonest words, in rank order, out of the subtitle counts: the first `reach` plain words
/// that are not in `skip`. Reads only as much of the file as that takes.
pub fn common(path: &Path, reach: usize, names: bool, skip: &HashSet<String>) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).with_context(|| {
        format!("{}: no word list; run `lyk fetch` to download the data (`lyk where` shows the folder)", path.display())
    })?;
    let mut ranked: Vec<String> = Vec::with_capacity(reach);
    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        let Some(word) = line.split_whitespace().next() else { continue };
        if is_plain_word(word, names) && !skip.contains(word) {
            ranked.push(word.to_owned());
            if ranked.len() >= reach {
                break;
            }
        }
    }
    Ok(ranked)
}

/// The words of `lyk mistakes --drill`: every item's prompt, a word once per copy (the copies are
/// its weight), and the confusable pairs' words.
pub fn mistakes(path: &Path, names: bool) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!("{}: no drill set yet; run `lyk mistakes --drill` (it reads the keymux steno tape)", path.display())
    })?;
    mistakes_from(&text, names).with_context(|| path.display().to_string())
}

pub fn mistakes_from(text: &str, names: bool) -> Result<Vec<String>> {
    let json: serde_json::Value = serde_json::from_str(text)?;
    let prompt = |item: &serde_json::Value| item.get("p").and_then(|p| p.as_str()).map(str::to_owned);
    let mut words: Vec<String> = json.get("items").and_then(|i| i.as_array()).into_iter().flatten().filter_map(prompt).collect();
    for pair in json.get("pairs").and_then(|p| p.as_array()).into_iter().flatten() {
        words.extend(pair.get("items").and_then(|i| i.as_array()).into_iter().flatten().filter_map(prompt));
    }
    words.retain(|w| is_plain_word(w, names));
    if words.is_empty() {
        bail!("the drill set has no ordinary words in it");
    }
    Ok(words)
}

/// Words of a plain text file, split on whitespace with the punctuation around each trimmed.
pub fn file(path: &Path, names: bool) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').replace('\u{2019}', "'"))
        .filter(|w| is_plain_word(w, names))
        .collect();
    if words.is_empty() {
        bail!("{}: no plain words in it", path.display());
    }
    Ok(words)
}

/// The phrases of the phrasing grammar (`02-lyk-phrasing.json`), as the text they type, in the
/// file's order.
pub fn phrases(dir: &Path) -> Result<Vec<String>> {
    let path = dir.join("02-lyk-phrasing.json");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("{}: no phrasing grammar built; run `lyk build`", path.display()))?;
    let map: std::collections::BTreeMap<String, String> = serde_json::from_str(&text).with_context(|| path.display().to_string())?;
    let mut seen = std::collections::HashSet::new();
    let phrases: Vec<String> = map
        .into_values()
        .filter(|p| p.split(' ').all(|w| is_plain_word(w, false)) && seen.insert(p.clone()))
        .collect();
    if phrases.is_empty() {
        bail!("{}: no phrases in it", path.display());
    }
    Ok(phrases)
}

const SENTENCE_WORDS: std::ops::RangeInclusive<usize> = 5..=22;

/// Sentences out of the books `lyk fetch` downloads (`data/corpus/*.txt`): five to 22 words, plain
/// letters and ordinary punctuation only.
pub fn quotes(dir: &Path) -> Result<Vec<String>> {
    let mut books: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("{}: no text corpus; run `lyk fetch`", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    books.sort();
    let mut out = Vec::new();
    for book in books {
        if let Ok(text) = std::fs::read_to_string(&book) {
            out.extend(sentences(&text));
        }
    }
    if out.is_empty() {
        bail!("{}: no usable sentences", dir.display());
    }
    Ok(out)
}

/// The usable sentences of one text.
#[must_use]
pub fn sentences(text: &str) -> Vec<String> {
    let flat: String = text
        .replace(['\u{2019}', '\u{2018}'], "'")
        .split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = flat.as_bytes();
    for i in 0..bytes.len() {
        let end = matches!(bytes[i], b'.' | b'!' | b'?') && bytes.get(i + 1).is_none_or(|b| *b == b' ');
        if end {
            let sentence = flat[start..=i].trim();
            start = i + 1;
            if usable_sentence(sentence) {
                out.push(sentence.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    out
}

fn usable_sentence(s: &str) -> bool {
    let n = s.split_whitespace().count();
    SENTENCE_WORDS.contains(&n)
        && s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '\'' | ',' | '.' | '!' | '?' | ';' | ':' | '-'))
        && !s.contains("--")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistakes_keep_their_copies_and_drop_non_words() {
        let json = r#"{"v":1,"items":[{"p":"to"},{"p":"to"},{"p":"Matt"},{"p":"a.b"}],
            "pairs":[{"title":"t","items":[{"p":"the"},{"p":"them"}]}]}"#;
        assert_eq!(mistakes_from(json, false).unwrap(), ["to", "to", "the", "them"]);
        assert!(mistakes_from(json, true).unwrap().contains(&"Matt".to_owned()));
        assert!(mistakes_from(r#"{"items":[]}"#, false).is_err());
    }

    #[test]
    fn sentences_are_split_cleaned_and_limited() {
        let text = "Call me Ishmael. Some years ago, never mind how long precisely, I thought\nI would sail about.\nHe said \"no\" to the idea of leaving the house today. Too short here.";
        let got = sentences(text);
        assert_eq!(got, ["Some years ago, never mind how long precisely, I thought I would sail about."]);
    }

    #[test]
    fn names_are_what_the_dictionaries_only_write_capitalized() {
        let dir = std::env::temp_dir().join(format!("lyk-practice-names-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dictionaries")).unwrap();
        std::fs::write(dir.join("dictionaries/plover-main.json"), r#"{"PWEPB":"Ben","PWAOEPB":"bean","TKOPB":"Don","TKOPB/1":"don","A":"A","TKP":"NASA"}"#).unwrap();
        std::fs::write(dir.join("dictionaries/lapwing-base.json"), r#"{"MAT":"Matt"}"#).unwrap();
        let mut names: Vec<String> = names_in(&dir).into_iter().collect();
        names.sort();
        assert_eq!(names, ["ben", "matt"]);
        assert!(names_in(&dir.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_says_what_to_run() {
        let err = common(Path::new("/no/such/en_full.txt"), 5, false, &HashSet::new()).unwrap_err().to_string();
        assert!(err.contains("lyk fetch"), "{err}");
        let err = mistakes(Path::new("/no/such/drills.json"), false).unwrap_err().to_string();
        assert!(err.contains("lyk mistakes --drill"), "{err}");
    }

    #[test]
    fn common_reads_only_plain_words_in_rank_order() {
        let dir = std::env::temp_dir().join(format!("lyk-practice-common-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("en_full.txt");
        std::fs::write(&path, "you 9\ni 8\nll 7\nthe 6\nt 5\nof 4\n").unwrap();
        assert_eq!(common(&path, 3, false, &HashSet::new()).unwrap(), ["you", "i", "the"]);
        assert_eq!(common(&path, 3, false, &HashSet::from(["you".to_owned()])).unwrap(), ["i", "the", "of"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
