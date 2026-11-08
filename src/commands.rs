//! What each `lyk-steno-theory` subcommand does beyond calling one library function: deploying,
//! looking words up, family reports, the plugin, and the ergonomics query. `main` only parses
//! the command line and dispatches here.

use crate::{analysis, built, ergonomics, family, generate, output, reverse, rules, stroke};
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Every file lyk's keymux config is made of, besides the two this binary generates fresh each
/// time (the words dictionary and the fallback plugin): the reserved stroke tables this repo
/// already tracks as its own source of truth (`rules/reserved/`), verbatim, under the numbered
/// name keymux loads them by, plus the one piece of generic keymux scaffolding (the theory-name
/// marker) every theory folder needs to be recognized and reloadable at all. Static, not derived
/// from the rules, so `deploy` can rebuild the whole folder from nothing rather than silently
/// dropping anything it doesn't happen to find already there.
///
/// `rules/reserved/emilys-modifiers.py` and its `regenerate.py` tool are deliberately left out of
/// this list: keymux only ever reads the `.json` a `.py` dictionary already has to be converted
/// to, and never runs the `.py` itself (see keymux's own `setup::LAPWING_FILES` comment — a `.py`
/// "needs Plover's plugin runtime, so it isn't used here"). They stay in `rules/reserved/` as this
/// repo's own source and dev tool for hand-editing that one file, not as something to ship into a
/// live config: run `python3 rules/reserved/regenerate.py` there and commit the result if that
/// file ever changes.
const STATIC_FILES: &[(&str, &str)] = &[
    ("01-lyk-commands.json", include_str!("../rules/reserved/commands.json")),
    ("03-lyk-punctuation.json", include_str!("../rules/reserved/punctuation.json")),
    ("04-lyk-emilys-modifiers.json", include_str!("../rules/reserved/emilys-modifiers.json")),
    (".keymux-theory", "lyk\n"),
];

/// `build` and `plugin`, plus every reserved stroke table and the generic scaffolding keymux
/// needs, straight into keymux's live config: the whole folder from one command, not a partial
/// update that assumes the rest is already sitting there correctly.
pub fn deploy(
    setup: &analysis::Setup,
    briefs: &generate::Briefs,
    common: usize,
    rules_path: Option<&Path>,
    to: &Path,
) -> Result<()> {
    let to = expand_home(to)?;
    std::fs::create_dir_all(&to).with_context(|| to.display().to_string())?;

    let words_out = PathBuf::from("out/lyk");
    generate::build(setup, briefs, common, &words_out, None)?;
    let plugin_out = PathBuf::from("out/lyk-orthographic-fallback.rs");
    write_plugin(setup, briefs, common, rules_path, &plugin_out)?;

    // Plain overwrite of every generated file; files an earlier layout left behind are removed.
    let mut copied = Vec::new();
    for role in output::Role::ALL {
        copied.push((words_out.join(role.file_name()), to.join(role.file_name())));
    }
    copied.push((plugin_out, to.join("05-lyk-orthographic-fallback.rs")));
    for (from, dest) in &copied {
        std::fs::copy(from, dest).with_context(|| dest.display().to_string())?;
    }
    output::remove_stale(&to)?;

    for (name, content) in STATIC_FILES {
        let dest = to.join(name);
        std::fs::write(&dest, content).with_context(|| dest.display().to_string())?;
    }

    // An earlier `deploy` shipped these two here by mistake (see `STATIC_FILES`'s own comment on
    // why they never belonged in a live config); remove them if a past run already left them
    // behind, rather than leaving dead files for `deploy` to never again mention or explain.
    for stale in ["04-lyk-emilys-modifiers.py", "regenerate.py"] {
        let path = to.join(stale);
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| path.display().to_string())?;
        }
    }

    println!(
        "deployed to {} ({} generated, {} static)",
        to.display(),
        copied.len(),
        STATIC_FILES.len()
    );
    Ok(())
}

/// A leading `~/` or bare `~`, expanded against `$HOME`; any other path is returned as-is.
pub fn expand_home(path: &Path) -> Result<PathBuf> {
    let Ok(rest) = path.strip_prefix("~") else {
        return Ok(path.to_path_buf());
    };
    let home = std::env::var("HOME").context("$HOME is not set, and the path starts with ~")?;
    Ok(PathBuf::from(home).join(rest))
}

/// Where `build` writes the dictionary, and so where the query commands read it back from.
pub const BUILT_DIR: &str = "out/lyk";

/// Every entry of lyk's dictionary. Read back from what `build` already wrote to [`BUILT_DIR`]
/// (milliseconds) unless `rebuild` is set or nothing is built yet, then generated from scratch
/// (tens of seconds): the answers are the same, so a query never needs the slow path just to know
/// a word's outline. A note on stderr says when the rules changed after the files were written.
pub fn dictionary_entries(
    setup: &analysis::Setup,
    briefs_path: Option<&Path>,
    common: usize,
    rebuild: bool,
) -> Result<Vec<generate::Entry>> {
    let dir = Path::new(BUILT_DIR);
    if !rebuild && built::exists(dir) {
        if let Some(note) = built::stale_note(dir) {
            eprintln!("{note}");
        }
        return built::load(dir);
    }
    let briefs = generate::Briefs::from_path(briefs_path)?;
    Ok(generate::generate(setup, &briefs, common)?.entries)
}

/// `resolve`: what a stroke outline types, from the dictionary or the rules' best reading.
pub fn run_resolve(
    setup: &impl Fn(Option<usize>) -> Result<analysis::Setup>,
    outline: &str,
    common: usize,
    briefs_path: Option<&Path>,
    rebuild: bool,
) -> Result<()> {
    let setup = setup(Some(common))?;
    let entries = dictionary_entries(&setup, briefs_path, common, rebuild)?;
    let strokes = stroke::parse_outline(outline).with_context(|| format!("{outline:?} is not a stroke outline"))?;
    print_resolve(&setup, &entries, &strokes);
    Ok(())
}

/// The one lookup, forward or backward: see `reverse::classify`.
pub fn run_lookup(
    setup: &impl Fn(Option<usize>) -> Result<analysis::Setup>,
    query: &str,
    common: usize,
    briefs_path: Option<&Path>,
    rebuild: bool,
) -> Result<()> {
    match reverse::classify(query) {
        reverse::Query::Stroke(strokes) => {
            let setup = setup(Some(common))?;
            let entries = dictionary_entries(&setup, briefs_path, common, rebuild)?;
            print_resolve(&setup, &entries, &strokes);
            Ok(())
        }
        reverse::Query::Text(phrase) => {
            if phrase.is_empty() {
                println!("Nothing to look up");
                return Ok(());
            }
            let setup = setup(Some(common))?;
            let entries = dictionary_entries(&setup, briefs_path, common, rebuild)?;
            let dictionary = generate::cheapest(&entries);
            for found in reverse::reverse_phrase(&setup, &dictionary, &phrase) {
                let lyk_label = match (&found.composed_of, found.lyk_deployed) {
                    (_, true) => "lyk".to_owned(),
                    (Some(pieces), false) => format!("lyk (composed: {})", pieces.join(" + ")),
                    (None, false) => "lyk (not deployed - rules' own reading)".to_owned(),
                };
                let label = |name: &str, pieces: &Option<Vec<String>>| {
                    pieces
                        .as_ref()
                        .map_or_else(|| name.to_owned(), |p| format!("{name} (composed: {})", p.join(" + ")))
                };
                let plover_label = label("plover", &found.plover_composed_of);
                let lapwing_label = label("lapwing", &found.lapwing_composed_of);
                let width = lyk_label.len().max(plover_label.len()).max(lapwing_label.len());
                println!(
                    "{}\n  {lyk_label:<width$} {}\n  {plover_label:<width$} {}\n  {lapwing_label:<width$} {}",
                    found.word,
                    found.lyk.as_deref().unwrap_or("?"),
                    found.plover.as_deref().unwrap_or("?"),
                    found.lapwing.as_deref().unwrap_or("?"),
                );
            }
            Ok(())
        }
    }
}

/// `ergonomics`: a stroke outline is scored directly, with no dictionary needed - the fast path
/// for just trying a chord shape. A word is looked up against the generated dictionary's own
/// chosen outline first, the same lookup `lookup`/`resolve` use, so what's explained is the real,
/// deployed stroke for that word, not a guess at one.
pub fn run_ergonomics(
    setup: &impl Fn(Option<usize>) -> Result<analysis::Setup>,
    query: &str,
    common: usize,
    briefs_path: Option<&Path>,
) -> Result<()> {
    if let Some(strokes) = stroke::parse_outline(query) {
        println!("{query} (a stroke outline):");
        println!("{}", ergonomics::explain_outline(&strokes));
        return Ok(());
    }
    let setup = setup(Some(common))?;
    let briefs = match briefs_path {
        Some(path) => generate::Briefs::load(path)?,
        None => generate::Briefs::built_in()?,
    };
    let generated = generate::generate(&setup, &briefs, common)?;
    let dictionary = generate::cheapest(&generated.entries);
    let Some(outline) = dictionary.get(query) else {
        println!("{query:?} is not a stroke outline and not a word in the generated dictionary");
        return Ok(());
    };
    println!("{query}  ->  {}:", stroke::render_outline(outline));
    println!("{}", ergonomics::explain_outline(outline));
    Ok(())
}

/// What a stroke outline resolves to: a real dictionary entry, the rules' best phonetic reading
/// of a miss, or a true miss nothing can read. Shared by `resolve` and `lookup`'s stroke mode.
pub fn print_resolve(setup: &analysis::Setup, entries: &[generate::Entry], strokes: &[stroke::Stroke]) {
    let text = stroke::render_outline(strokes);
    let index = generate::index_of(entries);
    if let Some(word) = index.get(&text) {
        println!("{text}  ->  {word}  (in the dictionary)");
    } else if let Some(reading) = setup.rules.resolve(strokes, Some(&generate::single_stroke_stems_of(entries))) {
        println!("{text}  ->  {reading}  (not in the dictionary; the rules' best reading)");
    } else {
        println!("{text}  ->  no reading the rules allow: a true miss");
    }
}

/// How many of the commonest words get their own stem row in the deployed plugin.
const PLUGIN_STEM_WORDS: usize = 10_000;

/// Generate the orthographic-fallback plugin's source and write it to `out`. Builds the
/// dictionary (same `setup`/`briefs`/`common` as `build`/`deploy`) to embed its single-stroke
/// stem table in the plugin — see [`rules::Rules::generate_plugin`].
pub fn write_plugin(
    setup: &analysis::Setup,
    briefs: &generate::Briefs,
    common: usize,
    rules_path: Option<&Path>,
    out: &Path,
) -> Result<()> {
    let rules = match rules_path {
        Some(path) => rules::Rules::load(path)?,
        None => rules::Rules::built_in()?,
    };
    let generated = generate::generate(setup, briefs, common)?;
    // Only the commonest words are embedded: a rarer word as the start of a run reads back from
    // its sounds, which is what a stem table is only a nicer spelling of. See `site/reference/fallback.json`.
    let mut stems = generate::single_stroke_stems(&generated);
    let mut ranked: Vec<(&String, &u64)> = setup.count.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let common_words: std::collections::HashSet<&str> =
        ranked.into_iter().take(PLUGIN_STEM_WORDS).map(|(word, _)| word.as_str()).collect();
    stems.retain(|_, word| common_words.contains(word.as_str()));
    let source = rules.generate_plugin(&stems);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, source).with_context(|| out.display().to_string())?;
    println!("wrote {}", out.display());
    Ok(())
}

/// `paths`: every way the lattice writes a word, and what became of each.
pub fn run_paths(
    setup: &impl Fn(Option<usize>) -> Result<analysis::Setup>,
    word: &str,
    limit: usize,
    common: usize,
    briefs_path: Option<&Path>,
) -> Result<()> {
    let setup = setup(Some(common))?;
    let briefs = generate::Briefs::from_path(briefs_path)?;
    let generated = generate::generate(&setup, &briefs, common)?;
    generate::print_paths(&setup, &generated, word, limit);
    Ok(())
}

/// `family`: one word's family, or the whole dictionary's family report.
pub fn run_family(
    setup: &impl Fn(Option<usize>) -> Result<analysis::Setup>,
    word: Option<&str>,
    common: usize,
    briefs_path: Option<&Path>,
) -> Result<()> {
    let setup = setup(Some(common))?;
    let briefs = match briefs_path {
        Some(path) => generate::Briefs::load(path)?,
        None => generate::Briefs::built_in()?,
    };
    let generated = generate::generate(&setup, &briefs, common)?;
    if let Some(word) = word {
        print_family(&setup, &generated, word);
    } else {
        print!("{}", family_report(&setup, &generated, 25));
    }
    Ok(())
}

/// Every member of `word`'s family, root first, then by frequency: see `Command::Family`.
pub fn print_family(setup: &analysis::Setup, generated: &generate::Generated, word: &str) {
    let links = &generated.links;
    let root = family::root(links, word);
    let cheapest = generate::cheapest(&generated.entries);
    let index = generate::index(generated);
    let mut members: Vec<&str> = links
        .keys()
        .map(String::as_str)
        .filter(|w| family::root(links, w) == root)
        .collect();
    members.sort_by_key(|w| (setup.rank.get(*w).copied().unwrap_or(usize::MAX), *w));
    members.insert(0, root);
    println!("family of {root:?}: {} member(s)", members.len());
    for member in members {
        let own = cheapest.get(member).map_or_else(|| "-".to_owned(), |o| stroke::render_outline(o));
        let Some(link) = links.get(member) else {
            println!("  {member:<18} {own:<22} (root)");
            continue;
        };
        let stem_outline = cheapest.get(link.stem.as_str());
        let backup = stem_outline.map(|s| {
            let mut composed = s.to_vec();
            composed.extend_from_slice(&link.strokes);
            setup.rules.joined(&mut composed);
            stroke::render_outline(&composed)
        });
        let backup_note = match &backup {
            Some(text) if index.get(text) == Some(&member) => format!("backup {text}"),
            Some(text) => index
                .get(text)
                .map_or_else(|| format!("backup {text} not built"), |other| format!("backup {text} lost to {other:?}")),
            None => "stem has no outline".to_owned(),
        };
        let starts = if family::follows_family(links, &cheapest, member) { "follows the family" } else { "own shape" };
        println!(
            "  {member:<18} {own:<22} = {} + -{:<5} {backup_note}  [{starts}]",
            link.stem, link.suffix
        );
    }
    let mut prefixed: Vec<(&String, &family::PrefixLink)> = generated
        .prefix_links
        .iter()
        .filter(|(_, link)| family::root(links, &link.stem) == root)
        .collect();
    prefixed.sort_by_key(|(w, _)| (setup.rank.get(*w).copied().unwrap_or(usize::MAX), w.as_str()));
    for (member, link) in prefixed {
        let own = cheapest.get(member.as_str()).map_or_else(|| "-".to_owned(), |o| stroke::render_outline(o));
        let shape = cheapest
            .get(link.stem.as_str())
            .and_then(|stem| family::prefixed(link, stem, &cheapest))
            .map(|o| stroke::render_outline(&o));
        let note = if shape.as_deref() == Some(own.as_str()) { "follows the family" } else { "own shape" };
        println!(
            "  {member:<18} {own:<22} = {}- + {:<11} prefix shape {}  [{note}]",
            link.prefix,
            link.stem,
            shape.unwrap_or_else(|| "-".to_owned())
        );
    }
}

/// The whole dictionary's family numbers for the top `TOP` words, and the commonest members whose
/// own outline doesn't start from their stem's: printed by `family` with no word and by `build`.
pub fn family_report(setup: &analysis::Setup, generated: &generate::Generated, show: usize) -> String {
    const TOP: usize = 10_000;
    let cheapest = generate::cheapest(&generated.entries);
    let index = generate::index(generated);
    let mut by_suffix: std::collections::BTreeMap<&str, [usize; 3]> = std::collections::BTreeMap::new();
    let mut odd: Vec<(usize, &str, String, &str, String)> = Vec::new();
    for (word, link) in &generated.links {
        let rank = setup.rank.get(word).copied().unwrap_or(usize::MAX);
        if rank >= TOP {
            continue;
        }
        let (Some(stem), Some(own)) = (cheapest.get(link.stem.as_str()), cheapest.get(word.as_str())) else {
            continue;
        };
        let tally = by_suffix.entry(link.suffix).or_default();
        tally[0] += 1;
        let mut composed = stem.to_vec();
        composed.extend_from_slice(&link.strokes);
        setup.rules.joined(&mut composed);
        if index.get(&stroke::render_outline(&composed)) == Some(&word.as_str()) {
            tally[1] += 1;
        }
        if family::follows_family(&generated.links, &cheapest, word) {
            tally[2] += 1;
        } else {
            odd.push((rank, word, stroke::render_outline(own), link.stem.as_str(), stroke::render_outline(stem)));
        }
    }
    let mut out = format!(
        "word families in the top {TOP}: members by ending, how many can be typed as stem + ending, \
         and how many have an outline that follows the family's shape\n"
    );
    let pct = |part: usize, whole: usize| part * 100 / whole.max(1);
    for (suffix, [n, backup, starts]) in &by_suffix {
        let _ = writeln!(
            out,
            "  -{suffix:<6} {n:>5} members   stem + ending {backup:>5} ({:>3}%)   follows family {starts:>5} ({:>3}%)",
            pct(*backup, *n),
            pct(*starts, *n),
        );
    }
    odd.sort();
    out += "the commonest members whose outline doesn't follow the family\n";
    for (rank, word, own, stem, stem_outline) in odd.into_iter().take(show) {
        let _ = writeln!(out, "  #{rank:<6} {word:<16} {own:<14} stem {stem} {stem_outline}");
    }
    out
}
