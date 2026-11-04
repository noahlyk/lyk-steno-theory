//! `build`: generate the dictionary, write it out, and report what it costs to type.

use super::{Briefs, Entry, Generated, Origin, Setup, cheapest, generate, keys, shortest};
use crate::num::ToF64;
use crate::output::Written;
use crate::stroke::{parse_outline, render_outline};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// The frequency bands the report breaks keys down by: the rank a band runs up to, and its name.
const BANDS: [(usize, &str); 8] = [
    (100, "top 100"),
    (1_000, "100-1k"),
    (5_000, "1k-5k"),
    (10_000, "5k-10k"),
    (20_000, "10k-20k"),
    (50_000, "20k-50k"),
    (100_000, "50k-100k"),
    (usize::MAX, "100k+"),
];

/// A dictionary to compare with besides Plover and Lapwing: its name and its outline per word.
type Against = (String, HashMap<String, String>);

/// Build the dictionary, write it as one file per role into the folder `out` (see
/// [`crate::output`]), and say what it costs to type next to Plover and Lapwing.
pub fn build(setup: &Setup, briefs: &Briefs, common: usize, out: &Path, against: Option<&Against>) -> Result<()> {
    let generated = generate(setup, briefs, common)?;
    let files = crate::output::write_dictionary(&generated, out)?;
    print_summary(setup, &generated, &files, out);
    let (total, bands, worse) = tally(setup, &generated, against);
    print_costs(&total, &bands, against.map(|(name, _)| name.as_str()));
    print_lost(setup, &generated);
    print_worse(setup, &generated, worse);
    Ok(())
}

/// What was written and how each pass went.
fn print_summary(setup: &Setup, generated: &Generated, files: &[Written], out: &Path) {
    let mut by_origin: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &generated.entries {
        *by_origin.entry(format!("{:?}", entry.origin)).or_default() += 1;
    }
    println!("wrote {} entries to {} ({} files)", generated.entries.len(), out.display(), files.len());
    for file in files {
        let unchanged = if file.changed { "" } else { "  (unchanged)" };
        println!("  {:<24} {:>7} entries{unchanged}", file.role.file_name(), file.entries);
    }
    for (origin, n) in &by_origin {
        println!("  {origin:<12} {n:>7}");
    }
    println!("  words the rules could not place: {}", generated.unresolved.len());
    println!("  backup entries skipped as collisions: {}", generated.backup_skipped);
    println!("  words still contested after fixed-point peeling (the real solver's own input): {}", generated.contested);
    println!(
        "  phrase entries added: {} ({} took the stroke from a rarer word)",
        generated.phrases_added, generated.phrases_displaced
    );
    println!("  extra single-stroke entries added last, in strokes nobody else needed: {}", generated.extras_added);
    println!("  phrase entries skipped as collisions: {}", generated.phrases_skipped);
    println!("  words stranded on a worse outline than their own, unclaimed plain reading: {}", generated.stranded.len());
    if !generated.stranded.is_empty() {
        println!("    {}", generated.stranded.join(", "));
    }
    let sample: Vec<String> =
        generated.unresolved.iter().take(40).map(|w| format!("{w} ({})", setup.pron[w.as_str()][0].join(" "))).collect();
    println!("    the commonest: {}", sample.join(", "));
}

/// Strokes and keys per word, each weighted by how often the word is said.
#[derive(Default)]
struct Total {
    weight: f64,
    ours: (f64, f64),
    plover: (f64, f64),
    lapwing: (f64, f64),
    other: (f64, f64),
}

/// One frequency band: weighted keys per dictionary, then the band's words counted once and how
/// many of them are one stroke here, in Plover and in Lapwing.
#[derive(Default)]
struct Band {
    weight: f64,
    keys: [f64; 4],
    words: f64,
    one_stroke: [f64; 3],
}

/// A word that costs more keys here than in Plover: the weighted excess, the word, its keys here
/// and in Plover.
type Worse<'a> = (f64, &'a str, u32, u32);

/// Every word every dictionary writes, tallied overall and by band, and the words that cost more
/// keys than Plover's.
fn tally<'a>(setup: &Setup, generated: &'a Generated, against: Option<&Against>) -> (Total, [Band; BANDS.len()], Vec<Worse<'a>>) {
    let cost_of = |outlines: Option<&Vec<String>>| {
        let outline = shortest(outlines?)?;
        Some((outline.len(), keys(&outline)))
    };
    let other_cost = |word: &str| {
        let outline = parse_outline(against?.1.get(word)?)?;
        Some((outline.len(), keys(&outline)))
    };
    let mut total = Total::default();
    let mut bands: [Band; BANDS.len()] = Default::default();
    let mut worse = Vec::new();
    for (word, outline) in cheapest(&generated.entries) {
        let (strokes, key_count) = (outline.len(), keys(outline));
        let (Some(plover), Some(lapwing)) = (cost_of(setup.gold.plover.get(word)), cost_of(setup.gold.lapwing.get(word)))
        else {
            continue;
        };
        let other = other_cost(word);
        if against.is_some() && other.is_none() {
            continue;
        }
        let other = other.unwrap_or_default();
        let weight = setup.count.get(word).copied().unwrap_or(0).to_f64();
        let add = |sum: &mut (f64, f64), (s, k): (usize, u32)| {
            sum.0 += weight * s.to_f64();
            sum.1 += weight * f64::from(k);
        };
        total.weight += weight;
        add(&mut total.ours, (strokes, key_count));
        add(&mut total.plover, plover);
        add(&mut total.lapwing, lapwing);
        add(&mut total.other, other);
        let rank = setup.rank.get(word).copied().unwrap_or(usize::MAX);
        let band = &mut bands[BANDS.iter().position(|(to, _)| rank < *to).unwrap_or(0)];
        band.weight += weight;
        for (sum, k) in band.keys.iter_mut().zip([key_count, plover.1, lapwing.1, other.1]) {
            *sum += weight * f64::from(k);
        }
        band.words += 1.0;
        for (count, s) in band.one_stroke.iter_mut().zip([strokes, plover.0, lapwing.0]) {
            *count += f64::from(u8::from(s == 1));
        }
        if key_count > plover.1 {
            worse.push((weight * f64::from(key_count - plover.1), word, key_count, plover.1));
        }
    }
    (total, bands, worse)
}

fn print_costs(total: &Total, bands: &[Band], other: Option<&str>) {
    let other_name = other.unwrap_or("");
    println!("\nwhat a word costs on average, weighted by how often it is said");
    println!("(words all the dictionaries write; strokes and keys pressed per word)");
    println!("            strokes     keys");
    let mut rows = vec![("this", total.ours), ("plover", total.plover), ("lapwing", total.lapwing)];
    if other.is_some() {
        rows.push((other_name, total.other));
    }
    for (name, (strokes, key_count)) in rows {
        println!("  {name:<9} {:>7.3} {:>8.3}", strokes / total.weight, key_count / total.weight);
    }
    println!("\nkeys per word by how common the word is, and the share of all words said;");
    println!("then, counting each word once, how many of the band's words are one stroke");
    println!("            share     this   plover  lapwing  {other_name:<8}   words  1-stroke  plover  lapwing");
    for ((_, name), band) in BANDS.iter().zip(bands).filter(|(_, band)| band.weight != 0.0) {
        let per = |k: f64| k / band.weight;
        let other = if other.is_some() { format!("  {:>7.3}", per(band.keys[3])) } else { String::from("         ") };
        let share = |n: f64| 100.0 * n / band.words;
        println!(
            "  {name:<9} {:>5.1}%  {:>7.3}  {:>7.3}  {:>7.3}{other}  {:>6}  {:>6.1}%  {:>5.1}%  {:>5.1}%",
            100.0 * band.weight / total.weight,
            per(band.keys[0]),
            per(band.keys[1]),
            per(band.keys[2]),
            band.words,
            share(band.one_stroke[0]),
            share(band.one_stroke[1]),
            share(band.one_stroke[2]),
        );
    }
}

/// The commonest words that lost their outline to a word that sounds the same.
fn print_lost(setup: &Setup, generated: &Generated) {
    println!("\nthe commonest words that lost their outline to a word that sounds the same");
    let mut lost: Vec<&Entry> =
        generated.entries.iter().filter(|e| e.lost_to.is_some() && matches!(e.origin, Origin::Ladder(_))).collect();
    lost.sort_by_key(|e| setup.rank.get(&e.word).copied().unwrap_or(usize::MAX));
    for entry in lost.iter().take(30) {
        let holder = entry.lost_to.as_deref().unwrap_or("");
        let theirs =
            generated.entries.iter().find(|e| e.word == holder).map_or_else(String::new, |e| render_outline(&e.outline));
        println!("  {:<12} {:<18} (the outline of {holder}: {theirs})", entry.word, render_outline(&entry.outline));
    }
}

/// The words that cost the most extra keys over Plover, weighted by how often they are said.
fn print_worse(setup: &Setup, generated: &Generated, mut worse: Vec<Worse>) {
    worse.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("\nthe words that cost the most extra keys over plover");
    for (_, word, ours_keys, plover_keys) in worse.iter().take(40) {
        let outline =
            generated.entries.iter().filter(|e| e.word == *word).map(|e| render_outline(&e.outline)).collect::<Vec<_>>().join(" ");
        let plover = setup.gold.plover.get(*word).and_then(|o| o.first()).map_or("", String::as_str);
        println!("  {word:<12} {ours_keys} vs {plover_keys} keys   ours {outline:<24} plover {plover}");
    }
}
