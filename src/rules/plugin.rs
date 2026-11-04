//! The keymux WASM plugin that reads a stroke back exactly the way [`Rules::resolve`] does,
//! generated from this theory's own tables instead of hand-copied, so the two can never drift
//! apart. `plugin_template.rs.in` is the plugin's source with a `__NAME__` placeholder for each
//! table; the ABI boilerplate (`alloc`/`fallback`/`joins`) is fixed there, since every plugin
//! needs it whatever the theory.

use super::Rules;
use crate::stroke::Stroke;
use std::collections::HashMap;
use std::fmt::Write;

const TEMPLATE: &str = include_str!("plugin_template.rs.in");

impl Rules {
    /// The plugin's source. `stems` is every single-stroke word in the generated dictionary, by its
    /// own bare stroke (see [`crate::generate::single_stroke_stems`]), so the plugin reads a known
    /// word's real text as the stem of an otherwise-unmatched continuation run (`SKP/+-R`, "and"
    /// plus "-er", reads "ander", not "sper"). It is a snapshot of the dictionary, not a fixed
    /// table: regenerate and redeploy whenever the dictionary changes.
    #[must_use]
    pub fn generate_plugin(&self, stems: &HashMap<Stroke, String>) -> String {
        let tokens = |tokens: &[String]| {
            let inner: Vec<String> = tokens.iter().map(|t| format!("{t:?}")).collect();
            format!("&[{}]", inner.join(", "))
        };
        let onset_rev = rows(self.onset_rev.iter(), |(stroke, t)| format!("({}, {})", stroke.0, tokens(t)));
        let coda_rev = rows(self.coda_rev.iter(), |(stroke, t)| format!("({}, {})", stroke.0, tokens(t)));
        let vowel_rev = rows(sorted(&self.vowel_rev), |(stroke, label)| format!("({}, {label:?})", stroke.0));
        let vowel_coda_rev = rows(self.vowel_coda_rev.iter(), |(combo, vowel, coda)| {
            format!("({}, {vowel:?}, {coda:?})", combo.0)
        });
        let spelling = rows(sorted(&self.spelling), |(stroke, letter)| format!("({}, {letter:?})", stroke.0));
        let suffixes = rows(sorted(&self.suffixes), |(stroke, text)| format!("({}, {text:?})", stroke.0));
        let two_stroke_suffixes = rows(self.two_stroke_suffixes.iter(), |(text, strokes)| {
            format!("({}, {}, {text:?})", strokes[0].0, strokes[1].0)
        });
        let stems = rows(sorted(stems), |(stroke, word)| format!("({}, {word:?})", stroke.0));
        [
            ("__LEFT__", Stroke::LEFT.to_string()),
            ("__VOWELS__", Stroke::VOWELS.to_string()),
            ("__STAR__", Stroke::STAR.to_string()),
            ("__RIGHT__", Stroke::RIGHT.to_string()),
            ("__JOIN__", Stroke::JOIN.to_string()),
            ("__SPELL__", Stroke::SPELL.to_string()),
            ("__ONSET_REV__", onset_rev),
            ("__CODA_REV__", coda_rev),
            ("__VOWEL_REV__", vowel_rev),
            ("__VOWEL_CODA_REV__", vowel_coda_rev),
            ("__SPELLING__", spelling),
            ("__SUFFIXES__", suffixes),
            ("__TWO_STROKE_SUFFIXES__", two_stroke_suffixes),
            ("__STEMS__", stems),
        ]
        .into_iter()
        .fold(TEMPLATE.to_owned(), |source, (name, value)| source.replace(name, &value))
    }
}

/// A map's entries in stroke order, so the generated tables are the same on every run.
fn sorted<V>(map: &HashMap<Stroke, V>) -> impl Iterator<Item = (&Stroke, &V)> {
    let mut entries: Vec<(&Stroke, &V)> = map.iter().collect();
    entries.sort_by_key(|(stroke, _)| stroke.0);
    entries.into_iter()
}

/// One `    row,` line per item: the body of a generated `const` table.
fn rows<T>(items: impl Iterator<Item = T>, row: impl Fn(T) -> String) -> String {
    items.fold(String::new(), |mut out, item| {
        let _ = writeln!(out, "    {},", row(item));
        out
    })
}

#[cfg(test)]
mod tests {
    use crate::rules::Rules;
    use crate::stroke::parse_outline;
    use std::collections::HashMap;
    use std::process::Command;

    /// The generated plugin, compiled natively with a `main` that reads each argument back, gives
    /// exactly what `Rules::resolve` gives for the same outlines: the guarantee the plugin exists
    /// for, checked end to end instead of trusted.
    #[test]
    fn the_generated_plugin_reads_every_outline_exactly_as_the_rules_do() {
        let rules = Rules::built_in().unwrap();
        let stems: HashMap<_, _> = [("SKP", "and"), ("PWEULD", "build"), ("KUB", "cube"), ("HAF", "have")]
            .into_iter()
            .map(|(o, w)| (parse_outline(o).unwrap()[0], w.to_owned()))
            .collect();
        let outlines = [
            "KAT", "SKP/+-R", "PWEULD/+-G", "KUB/+EUZ/+-PL", "KHRAEUR/+U/+TEU", "HAF/+-G", "TPAOL/+-RB",
            "EUPB/+TPOR/+PHAEUGS", "STKPW/+-D", "~K/+A/+T", "TAEU/+-BL", "PRAF/+U/+TU/+PWUL",
        ];
        let dir = std::env::temp_dir().join(format!("lyk-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("plugin.rs");
        let main = "\nfn main() { for a in std::env::args().skip(1) { println!(\"{}\", resolve(&a).unwrap_or_default()); } }\n";
        std::fs::write(&source, rules.generate_plugin(&stems) + main).unwrap();
        let binary = dir.join("plugin");
        let compiled = Command::new("rustc")
            .args(["--edition", "2021", "-O", "-A", "warnings", "-o"])
            .arg(&binary)
            .arg(&source)
            .output()
            .expect("rustc runs");
        assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
        let run = Command::new(&binary).args(outlines).output().unwrap();
        let plugin: Vec<String> = String::from_utf8(run.stdout).unwrap().lines().map(str::to_owned).collect();
        let ours: Vec<String> = outlines
            .iter()
            .map(|o| rules.resolve(&parse_outline(o).unwrap(), Some(&stems)).unwrap_or_default())
            .collect();
        assert_eq!(plugin, ours);
        // The two-stroke suffixes read as themselves after a known stem
        let read = |o: &str| rules.resolve(&parse_outline(o).unwrap(), Some(&stems)).unwrap();
        assert_eq!(read("KUB/+EUZ/+-PL"), "cubism");
        assert!(read("KHRAEUR/+U/+TEU").ends_with("ity"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
