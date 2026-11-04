//! `build`: generate the dictionary, write it out, and report what it costs to type.

use super::*;

/// Build the dictionary, write it as one file per role into the folder `out` (see
/// [`crate::output`]), and say what it costs to type next to Plover and Lapwing.
pub fn build(
    setup: &Setup,
    briefs: &Briefs,
    common: usize,
    out: &Path,
    against: Option<&(String, HashMap<String, String>)>,
) -> Result<()> {
    let generated = generate(setup, briefs, common)?;
    let files = crate::output::write_dictionary(&generated, out)?;

    let mut by_origin: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &generated.entries {
        *by_origin.entry(format!("{:?}", entry.origin)).or_default() += 1;
    }
    println!(
        "wrote {} entries to {} ({} files)",
        generated.entries.len(),
        out.display(),
        files.len()
    );
    for file in &files {
        println!(
            "  {:<24} {:>7} entries{}",
            file.role.file_name(),
            file.entries,
            if file.changed { "" } else { "  (unchanged)" }
        );
    }
    for (origin, n) in &by_origin {
        println!("  {origin:<12} {n:>7}");
    }
    println!(
        "  words the rules could not place: {}",
        generated.unresolved.len()
    );
    println!(
        "  backup entries skipped as collisions: {}",
        generated.backup_skipped
    );
    println!(
        "  words still contested after fixed-point peeling (the real solver's own input): {}",
        generated.contested
    );
    println!(
        "  phrase entries added: {} ({} took the stroke from a rarer word)",
        generated.phrases_added, generated.phrases_displaced
    );
    println!("  extra single-stroke entries added last, in strokes nobody else needed: {}", generated.extras_added);
    println!(
        "  phrase entries skipped as collisions: {}",
        generated.phrases_skipped
    );
    println!(
        "  words stranded on a worse outline than their own, unclaimed plain reading: {}",
        generated.stranded.len()
    );
    if !generated.stranded.is_empty() {
        println!("    {}", generated.stranded.join(", "));
    }
    let sample: Vec<String> = generated
        .unresolved
        .iter()
        .take(40)
        .map(|w| format!("{w} ({})", setup.pron[w.as_str()][0].join(" ")))
        .collect();
    println!("    the commonest: {}", sample.join(", "));

    // Each word's cheapest outline here, and what Plover and Lapwing spend on the same word
    let ours: HashMap<&str, (usize, u32)> = cheapest(&generated.entries)
        .into_iter()
        .map(|(word, outline)| (word, (outline.len(), keys(outline))))
        .collect();
    let cost_of = |outlines: Option<&Vec<String>>| {
        let outline = shortest(outlines?)?;
        Some((outline.len(), keys(&outline)))
    };
    // Another dictionary to compare with, by its own shortest outline for each word
    let other_cost = |word: &str| -> Option<(usize, u32)> {
        let outline = parse_outline(against?.1.get(word)?)?;
        Some((outline.len(), keys(&outline)))
    };
    let mut total = [0f64; 9];
    // The same, by how common the word is: where the keys go
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
    // Per band: weight, weighted keys (ours, Plover, Lapwing, other), then unweighted: words
    // counted once, and how many of them are one stroke here, in Plover, and in Lapwing.
    let mut bands = [[0f64; 9]; BANDS.len()];
    let mut worse: Vec<(f64, &str, u32, u32)> = Vec::new();
    for (word, &(strokes, key_count)) in &ours {
        let (Some(plover), Some(lapwing)) = (
            cost_of(setup.gold.plover.get(*word)),
            cost_of(setup.gold.lapwing.get(*word)),
        ) else {
            continue;
        };
        let other = other_cost(word);
        if against.is_some() && other.is_none() {
            continue;
        }
        let other = other.unwrap_or_default();
        let weight = setup.count.get(*word).copied().unwrap_or(0) as f64;
        total[0] += weight;
        total[1] += weight * strokes as f64;
        total[2] += weight * f64::from(key_count);
        total[3] += weight * plover.0 as f64;
        total[4] += weight * f64::from(plover.1);
        total[5] += weight * lapwing.0 as f64;
        total[6] += weight * f64::from(lapwing.1);
        total[7] += weight * other.0 as f64;
        total[8] += weight * f64::from(other.1);
        let rank = setup.rank.get(*word).copied().unwrap_or(usize::MAX);
        let band = BANDS.iter().position(|(to, _)| rank < *to).unwrap_or(0);
        bands[band][0] += weight;
        bands[band][1] += weight * f64::from(key_count);
        bands[band][2] += weight * f64::from(plover.1);
        bands[band][3] += weight * f64::from(lapwing.1);
        bands[band][4] += weight * f64::from(other.1);
        bands[band][5] += 1.0;
        bands[band][6] += f64::from(u8::from(strokes == 1));
        bands[band][7] += f64::from(u8::from(plover.0 == 1));
        bands[band][8] += f64::from(u8::from(lapwing.0 == 1));
        if key_count > plover.1 {
            worse.push((
                weight * f64::from(key_count - plover.1),
                word,
                key_count,
                plover.1,
            ));
        }
    }
    let other_name = against.map_or("", |(name, _)| name.as_str());
    println!("\nwhat a word costs on average, weighted by how often it is said");
    println!("(words all the dictionaries write; strokes and keys pressed per word)");
    println!("            strokes     keys");
    let mut rows = vec![
        ("this", total[1], total[2]),
        ("plover", total[3], total[4]),
        ("lapwing", total[5], total[6]),
    ];
    if against.is_some() {
        rows.push((other_name, total[7], total[8]));
    }
    for (name, strokes, key_count) in rows {
        println!(
            "  {name:<9} {:>7.3} {:>8.3}",
            strokes / total[0],
            key_count / total[0]
        );
    }
    println!("\nkeys per word by how common the word is, and the share of all words said;");
    println!("then, counting each word once, how many of the band's words are one stroke");
    println!("            share     this   plover  lapwing  {other_name:<8}   words  1-stroke  plover  lapwing");
    for ((_, name), band) in BANDS.iter().zip(&bands) {
        if band[0] == 0.0 {
            continue;
        }
        let other = if against.is_some() {
            format!("  {:>7.3}", band[4] / band[0])
        } else {
            String::from("         ")
        };
        println!(
            "  {name:<9} {:>5.1}%  {:>7.3}  {:>7.3}  {:>7.3}{other}  {:>6}  {:>6.1}%  {:>5.1}%  {:>5.1}%",
            100.0 * band[0] / total[0],
            band[1] / band[0],
            band[2] / band[0],
            band[3] / band[0],
            band[5],
            100.0 * band[6] / band[5],
            100.0 * band[7] / band[5],
            100.0 * band[8] / band[5],
        );
    }
    println!("\nthe commonest words that lost their outline to a word that sounds the same");
    let mut lost: Vec<&Entry> = generated
        .entries
        .iter()
        .filter(|e| e.lost_to.is_some() && matches!(e.origin, Origin::Ladder(_)))
        .collect();
    lost.sort_by_key(|e| setup.rank.get(&e.word).copied().unwrap_or(usize::MAX));
    for entry in lost.iter().take(30) {
        let holder = entry.lost_to.as_deref().unwrap_or("");
        let theirs = generated
            .entries
            .iter()
            .find(|e| e.word == holder)
            .map_or_else(String::new, |e| render_outline(&e.outline));
        println!(
            "  {:<12} {:<18} (the outline of {holder}: {theirs})",
            entry.word,
            render_outline(&entry.outline)
        );
    }
    worse.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("\nthe words that cost the most extra keys over plover");
    for (_, word, ours_keys, plover_keys) in worse.iter().take(40) {
        let outline = generated
            .entries
            .iter()
            .filter(|e| e.word == *word)
            .map(|e| render_outline(&e.outline))
            .collect::<Vec<_>>()
            .join(" ");
        let plover = setup
            .gold
            .plover
            .get(*word)
            .and_then(|o| o.first())
            .map_or("", String::as_str);
        println!(
            "  {word:<12} {ours_keys} vs {plover_keys} keys   ours {outline:<24} plover {plover}"
        );
    }
    Ok(())
}
