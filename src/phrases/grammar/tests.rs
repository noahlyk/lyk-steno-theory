use super::*;
use crate::dictionary_fixture;
use std::collections::HashMap;

fn grammar() -> Grammar {
    Grammar::built_in().expect("rules/grammar.toml parses")
}

fn codes() -> Codes {
    grammar().derive_codes(dictionary_fixture::setup()).expect("every verb gets a code")
}

fn text_of(codes: &Codes, grammar: &Grammar, stroke: &str) -> Option<String> {
    let stroke = stroke_of(stroke);
    grammar.phrases(codes).into_iter().find(|p| p.stroke == stroke).map(|p| p.text)
}

/// The codes the plan's table lists (`plans/08-phrasing-grammar.md`), which the rule must give.
#[test]
fn the_rule_gives_the_codes_of_the_plan() {
    let grammar = grammar();
    let expected = [
        ("be", "-B"), ("have", "-F"), ("have been", "-FB"), ("say", "-S"), ("go", "-G"), ("get", "-T"),
        ("make", "-BG"), ("know", "-PB"), ("think", "-PBG"), ("take", "-BGT"), ("see", "-SZ"), ("come", "-PL"),
        ("want", "-PBT"), ("look", "-BLG"), ("tell", "-L"), ("feel", "-FL"), ("mean", "-PBL"), ("need", "-D"),
        ("like", "-BLGZ"), ("hear", "-R"), ("seem", "-PLS"), ("find", "-PBD"), ("give", "-FG"),
    ];
    let got: Vec<(String, String)> = codes().named(&grammar).into_iter().map(|(v, c)| (v, render_outline(&[c]))).collect();
    let expected: Vec<(String, String)> = expected.iter().map(|(v, c)| ((*v).to_owned(), (*c).to_owned())).collect();
    assert_eq!(got, expected);
}

#[test]
fn every_phrase_has_its_own_typable_stroke_and_text() {
    let grammar = grammar();
    let phrases = grammar.phrases(&codes());
    assert_eq!(phrases.len(), 3_758);
    let mut strokes: HashMap<Stroke, &str> = HashMap::new();
    let mut texts: HashMap<&str, Stroke> = HashMap::new();
    for phrase in &phrases {
        assert!(phrase.stroke.typable(), "{} is not typable", phrase.text);
        assert!(strokes.insert(phrase.stroke, &phrase.text).is_none(), "{} shares a stroke", phrase.text);
        assert!(texts.insert(&phrase.text, phrase.stroke).is_none(), "{} is written twice", phrase.text);
        // the parts together are the stroke, and no part is empty
        let bits = phrase.parts.iter().fold(0, |bits, part| bits | stroke_of(&part.keys).0);
        assert_eq!(bits, phrase.stroke.0, "{}", phrase.text);
        assert!(phrase.parts.iter().all(|p| !p.keys.is_empty() && !p.says.is_empty()), "{}", phrase.text);
    }
}

#[test]
fn the_examples_of_the_plan() {
    let grammar = grammar();
    let codes = codes();
    for (stroke, text) in [
        ("KPWHEB", "it was"),
        ("KPWH-B", "it is"),
        ("SWR-B", "I am"),
        ("SWR*B", "I am not"),
        ("SWR-PB", "I know"),
        ("SWR*PB", "I don't know"),
        ("SWR*EPB", "I didn't know"),
        ("KPWHOEB", "it would be"),
        ("STWHEFB", "he had been"),
        ("SWRA*EFB", "I couldn't have been"),
        ("STWHR*EB", "they weren't"),
        ("KPWR-SZ", "you see"),
        ("SWR", "I do"),
    ] {
        assert_eq!(text_of(&codes, &grammar, stroke).as_deref(), Some(text.to_lowercase().as_str()), "{stroke}");
    }
}

#[test]
fn there_takes_be_have_been_and_the_modals_only() {
    let grammar = grammar();
    let there: Vec<String> = grammar.phrases(&codes()).into_iter().map(|p| p.text).filter(|t| t.starts_with("there ")).collect();
    for text in ["there was", "there would be", "there must have been", "there can", "there isn't"] {
        assert!(there.iter().any(|t| t == text), "{text}");
    }
    assert!(!there.iter().any(|t| t == "there do" || t == "there go" || t == "there have"), "{there:?}");
}

#[test]
fn a_stroke_is_explained_part_by_part() {
    let grammar = grammar();
    let codes = codes();
    let phrase = grammar.explain(&codes, stroke_of("SWRAB")).expect("SWRAB is I can be");
    assert_eq!(phrase.text, "i can be");
    let parts: Vec<(PartKind, &str, &str)> = phrase.parts.iter().map(|p| (p.kind, p.keys.as_str(), p.says.as_str())).collect();
    assert_eq!(parts, [(PartKind::Starter, "SWR", "I"), (PartKind::Auxiliary, "A", "can"), (PartKind::Verb, "-B", "be")]);
    let phrase = grammar.explain(&codes, stroke_of("SWRA*EFB")).expect("I couldn't have been");
    let kinds: Vec<(&str, &str)> = phrase.parts.iter().map(|p| (p.keys.as_str(), p.says.as_str())).collect();
    assert_eq!(kinds, [("SWR", "I"), ("AE", "could"), ("*", "not"), ("-FB", "have been")]);
}

#[test]
fn the_dictionary_has_every_phrase_and_the_codes_read_back() {
    let grammar = grammar();
    let generated = dictionary_fixture::generated();
    let own: HashMap<&str, Stroke> =
        generated.entries.iter().filter(|e| e.origin == Origin::Grammar).map(|e| (e.word.as_str(), e.outline[0])).collect();
    let codes = codes();
    let phrases = grammar.phrases(&codes);
    let questions = grammar.questions(&codes);
    // every statement and every question is there, except the questions a merged pair already writes
    assert_eq!(own.len(), phrases.len() + questions.len() - generated.questions.kept.len(), "left out: {:?}", generated.grammar.unplaced);
    assert!(generated.grammar.unplaced.is_empty(), "{:?}", generated.grammar.unplaced);
    assert!(generated.questions.unplaced.is_empty(), "{:?}", generated.questions.unplaced);
    for phrase in &phrases {
        assert_eq!(own.get(phrase.text.as_str()), Some(&phrase.stroke), "{}", phrase.text);
    }
    // a dictionary read back gives the same codes (what the lookup commands use)
    assert_eq!(grammar.codes_from(|text| own.get(text).copied()), Some(codes));
}

#[test]
fn every_question_has_its_own_typable_stroke_and_text() {
    let grammar = grammar();
    let codes = codes();
    let statements = grammar.phrases(&codes);
    let questions = grammar.questions(&codes);
    assert_eq!(questions.len(), statements.len());
    let bar = stroke_of("#").0;
    let mut strokes: HashMap<Stroke, &str> = statements.iter().map(|p| (p.stroke, p.text.as_str())).collect();
    let mut texts: HashMap<&str, Stroke> = statements.iter().map(|p| (p.text.as_str(), p.stroke)).collect();
    for (statement, question) in statements.iter().zip(&questions) {
        // the same stroke with the number bar down, in no statement's keys
        assert_eq!(question.stroke.0, statement.stroke.0 | bar, "{}", question.text);
        assert_eq!(statement.stroke.0 & bar, 0, "{}", statement.text);
        assert!(question.stroke.typable(), "{} is not typable", question.text);
        assert!(strokes.insert(question.stroke, &question.text).is_none(), "{} shares a stroke", question.text);
        assert!(texts.insert(&question.text, question.stroke).is_none(), "{} is also another phrase's text", question.text);
        assert_eq!(question.parts[0].kind, PartKind::Question);
        let bits = question.parts.iter().fold(0, |bits, part| bits | stroke_of(&part.keys).0);
        assert_eq!(bits, question.stroke.0, "{}", question.text);
    }
}

#[test]
fn the_questions_read_in_question_order() {
    let grammar = grammar();
    let codes = codes();
    for (stroke, text) in [
        ("#KPWRA", "can you"),
        ("#KPWRAE", "could you"),
        ("#KPWR", "do you"),
        ("#KPWR*E", "didn't you"),
        ("#KPWR-PBG", "do you think"),
        ("#KPWR*PBG", "don't you think"),
        ("#KPWH-B", "is it"),
        ("#KPWH*B", "isn't it"),
        ("#SWR-B", "am I"),
        ("#SWR*B", "am I not"),
        ("#SWR-F", "do I have"),
        ("#SWR-FB", "have I been"),
        ("#SWRA*EFB", "couldn't I have been"),
        ("#SWR*U", "shall I not"),
        ("#STWH-F", "does he have"),
        ("#STHR-B", "is there"),
        ("#STHR-FB", "has there been"),
        ("#TWR-S", "do we say"),
        ("#STWHR*EG", "didn't they go"),
    ] {
        let found = grammar.questions(&codes).into_iter().find(|p| p.stroke == stroke_of(stroke)).map(|p| p.text);
        assert_eq!(found.as_deref(), Some(text.to_lowercase().as_str()), "{stroke}");
    }
}

#[test]
fn a_question_never_changes_an_entry_the_dictionary_already_has() {
    let generated = dictionary_fixture::generated();
    let questions = grammar().questions(&codes());
    let others: HashMap<&str, Stroke> =
        generated.entries.iter().filter(|e| e.origin != Origin::Grammar).filter_map(|e| Some((e.word.as_str(), *e.outline.first()?))).collect();
    // a question the dictionary already writes keeps its entry (`do you`, `are you`) and is listed
    let kept: Vec<&str> = generated.questions.kept.iter().map(|(t, _)| t.as_str()).collect();
    assert!(kept.len() >= 20, "{kept:?}");
    for text in &kept {
        assert!(others.contains_key(text), "{text} is kept but is not an entry");
        assert!(!generated.entries.iter().any(|e| e.origin == Origin::Grammar && e.word == *text), "{text} is written twice");
    }
    // every other question is a grammar entry, and its stroke is held by no other entry
    let mut held: HashMap<Stroke, usize> = HashMap::new();
    for e in &generated.entries {
        if let [stroke] = e.outline[..] {
            *held.entry(stroke).or_default() += 1;
        }
    }
    for q in questions.iter().filter(|q| !kept.contains(&q.text.as_str())) {
        assert_eq!(held.get(&q.stroke), Some(&1), "{} {}", q.text, render_outline(&[q.stroke]));
    }
}

#[test]
fn a_merged_pair_never_writes_a_text_the_grammar_writes() {
    let generated = dictionary_fixture::generated();
    let written = texts(&generated.entries);
    let twice: Vec<&str> =
        generated.entries.iter().filter(|e| e.origin == Origin::Phrase && written.contains(e.word.as_str())).map(|e| e.word.as_str()).collect();
    assert!(twice.is_empty(), "{twice:?}");
}

#[test]
fn a_starter_chord_alone_is_never_a_word_the_grammar_displaced_without_a_new_outline() {
    // every word keeps an outline: nothing the grammar fixed leaves a common word without one
    let generated = dictionary_fixture::generated();
    let setup = dictionary_fixture::setup();
    let has: std::collections::HashSet<&str> =
        generated.entries.iter().filter(|e| !matches!(e.origin, Origin::Grammar | Origin::Phrase | Origin::Suffix)).map(|e| e.word.as_str()).collect();
    let missing: Vec<&String> = setup
        .count
        .iter()
        .filter(|(w, c)| **c > 5_000 && setup.pron.contains_key(w.as_str()) && w.len() > 1 && w.bytes().all(|b| b.is_ascii_lowercase()) && !has.contains(w.as_str()))
        .map(|(w, _)| w)
        .take(10)
        .collect();
    assert!(missing.is_empty(), "words with no outline: {missing:?}");
}
