//! lyk-steno-theory: generates a steno dictionary from phonetic rules.
//!
//! The rules (`rules/plover.toml`) say where each sound goes on the keyboard, in Plover theory.
//! They are judged by how many of the words Plover and Lapwing write they reproduce.

mod analysis;
mod answer;
mod built;
mod commands;
#[cfg(test)]
mod corpus_extract;
mod data;
#[cfg(test)]
mod dictionary_fixture;
mod fetch;
mod frequency;
mod generate;
mod interactive;
mod num;
mod output;
mod phrases;
mod gold;
mod ipa;
mod quote;
#[cfg(test)]
mod quote_corpus;
#[cfg(test)]
mod quality;
mod ergonomics;
mod family;
mod reverse;
mod rules;
mod stroke;
mod version;
mod view;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version = version::VERSION, about)]
struct Cli {
    /// Where the downloaded data lives
    #[arg(long, global = true, default_value = "data")]
    data: PathBuf,
    /// Rules to use instead of the built-in ones
    #[arg(long, global = true)]
    rules: Option<PathBuf>,
    /// Do not note when `out/lyk` was built by other code or rules than these (also
    /// `LYK_NO_STALE_NOTE=1`)
    #[arg(long, global = true)]
    no_stale_check: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the version (the same as `--version`)
    #[command(hide = true)]
    Version,
    /// Download the pronunciation, frequency and Plover/Lapwing data (American English)
    Fetch,
    /// Say what was loaded, and how well the pronunciations cover Plover and Lapwing
    Report,
    /// How many words the rules write the way Plover or Lapwing do
    Score {
        /// Only the this many most common words
        #[arg(long, default_value_t = 20_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Check that README.md's score table matches this, instead of printing it; exits
        /// non-zero and shows the up-to-date table when it doesn't
        #[arg(long)]
        check_readme: bool,
        /// Like `--check-readme`, but rewrite any stale table in place (only between its own
        /// markers) instead of failing
        #[arg(long)]
        write_readme: bool,
    },
    /// The most common words the rules get wrong
    Mismatches {
        /// Only words with this many syllables
        #[arg(long, default_value_t = 1)]
        syllables: usize,
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Only words the rules cannot write at all
        #[arg(long)]
        missing: bool,
        /// Skip the this many most common words: they get briefs
        #[arg(long, default_value_t = 0)]
        after: usize,
    },
    /// Which keys Plover and Lapwing use for each group of sounds, where they agree
    Learn {
        /// Hide groups with fewer words than this
        #[arg(long, default_value_t = 8)]
        min: usize,
        #[arg(long, default_value_t = 60)]
        limit: usize,
    },
    /// Show how the rules write a word, next to the real dictionary's own settled answer. With no
    /// word, opens the interactive lookup (all three theories, live as you type); Enter explains
    /// the line
    Explain {
        word: Option<String>,
        /// Words to cover when building the real dictionary to compare against, most common
        /// first. Matches `build`'s own default so the comparison reflects what's actually
        /// deployed.
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Build the dictionary: briefs first, then the rules, and say what it costs to type
    Build {
        /// The folder to write the Plover JSON dictionary files to, one per role (`02-lyk-words.json`,
        /// `02-lyk-backups.json`, ...); a file that would come out the same is left untouched
        #[arg(long, default_value = "out/lyk")]
        out: PathBuf,
        /// Words to cover, most common first. 100,000 is already in Plover/Lapwing's own
        /// ballpark and builds in a few seconds; raising it further has diminishing returns,
        /// bounded by how many words the pronunciation data covers, not by build time
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Another dictionary (JSON or RTF/CRE, such as Magnum's) to put in the cost tables
        #[arg(long)]
        against: Option<PathBuf>,
        /// Also print the full diagnostics: entries per source, words the rules could not place,
        /// collision and phrase counts, what a word costs next to Plover and Lapwing, keys by
        /// frequency band, the commonest words on a star, and the words costing the most extra
        /// keys over Plover. Off by default; the same conditions are asserted by `cargo test`
        #[arg(long)]
        report: bool,
    },
    /// Grab a random quote off the web and stroke it out, lyk next to Plover and Lapwing
    Quote {
        /// Words to cover when building the dictionary, most common first. Matches `build`'s own
        /// default so a quote shows the outlines the real, deployed dictionary actually has, not
        /// a smaller build's weaker stand-ins for words ranked past wherever a lower count cuts
        /// off (a mid-frequency word can only earn its full brevity treatment once the generator
        /// has placed enough of the words ahead of it to know what's still free).
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Try for a quote with at most this many words
        #[arg(long, default_value_t = 20)]
        words: usize,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Read a stroke outline back into sounds, as the engine's last resort when it misses the
    /// dictionary: the word if it's one the dictionary writes, otherwise the rules' best
    /// phonetic reading of the keys, or a true miss if no reading fits them at all. Keymux's
    /// `steno stroke`: the same lookup as `lookup`, every query read as stroke notation
    /// (lowercase is read as capitals). With no outline, opens the interactive lookup.
    #[command(visible_alias = "stroke")]
    Resolve {
        /// A stroke outline, such as `KAT` or `TPAEU/+HR`
        outline: Option<String>,
        /// Words to cover when building the dictionary, most common first. Matches `build`'s own
        /// default so a resolved reading matches what the real, deployed dictionary would do.
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch instead of reading back `out/lyk`
        #[arg(long)]
        rebuild: bool,
    },
    /// Look a word, phrase, or stroke up, whichever it is: `lookup cat` lists every outline that
    /// types it in lyk, Plover and Lapwing side by side, best first, then longer words that start
    /// with it; `lookup "tell me how"` does each word of a phrase; and `lookup KAT/-D` (all caps,
    /// valid notation) finds what that stroke types instead. A word with no entry is composed
    /// from entries or spelled out, and marked (quotes, or a note). With no query, opens the
    /// interactive lookup: type, and the answer follows live. Keymux's `steno lookup` / `steno
    /// keys`, which are the same command, for its one engine setup instead of three theories.
    #[command(visible_alias = "keys")]
    Lookup {
        /// The word, phrase, or stroke to look up, split on spaces; leave it out for the
        /// interactive lookup
        query: Vec<String>,
        /// Also show the physical keys that press each of lyk's strokes
        #[arg(long)]
        show_keys: bool,
        /// Show only the fastest outline for each word, not every outline
        #[arg(long)]
        fast: bool,
        /// Words to cover when building the dictionary, most common first. Matches `build`'s own
        /// default so a lookup matches the real, deployed dictionary.
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Every way a word can be written as joined strokes (the lattice over its sounds, cheapest
    /// first), and what became of each: in the dictionary, taken by another word, or rejected.
    /// With no word, opens the interactive lookup; Enter shows the paths of the line.
    Paths {
        word: Option<String>,
        /// How many paths to list
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// Words to cover when building the dictionary, most common first
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Show a word's whole family (`build`, `builds`, `building`, `buildings`, ...): each member's
    /// stem and ending, its outline, its stem-plus-ending backup, and whether its outline starts
    /// from the stem's own keys. With no word, opens the interactive lookup; Enter shows the family
    /// of the line. `--report` prints the family report for the whole dictionary instead.
    Family {
        word: Option<String>,
        /// Print the family report for the whole dictionary (what `family` with no word used to do)
        #[arg(long, conflicts_with = "word")]
        report: bool,
        /// Words to cover when building the dictionary, most common first
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Score a stroke outline or word's physical comfort, term by term: which finger reaches
    /// where, which pairs cost a transition, which finger is a gap, and the pinky tax - not just
    /// the one number `score`/`build` otherwise sum into `candidate_cost`. See `src/ergonomics.rs`.
    /// With no query, opens the interactive lookup; Enter scores the line.
    Ergonomics {
        /// A stroke outline (`KAT`, `TPAEU/+HR`) or a word (`cat`); a word is looked up against
        /// the built dictionary's own chosen outline
        query: Option<String>,
        /// Words to cover when building the dictionary, most common first (only matters for a
        /// word query). Matches `build`'s own default so the outline explained is the real,
        /// deployed one.
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
        /// Generate the dictionary from scratch (tens of seconds) instead of reading back the one
        /// `build` wrote to `out/lyk`: only for a change to the rules or briefs not built yet
        #[arg(long)]
        rebuild: bool,
    },
    /// Build the dictionary, then copy it straight into the live keymux config along with the
    /// reserved stroke tables: the one command for everything `build` otherwise leaves as a manual copy/paste
    Deploy {
        /// Where keymux's lyk config lives
        #[arg(long, default_value = "~/.config/keymux/steno/lyk")]
        to: PathBuf,
        /// Words to cover, most common first
        #[arg(long, default_value_t = 100_000)]
        common: usize,
        /// Briefs to use instead of the built-in ones
        #[arg(long)]
        briefs: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    version::silence_stale_note(cli.no_stale_check || std::env::var_os("LYK_NO_STALE_NOTE").is_some_and(|v| !v.is_empty() && v != "0"));
    let setup =
        |common: Option<usize>| analysis::Setup::load(&cli.data, cli.rules.as_deref(), common);
    match &cli.command {
        Command::Version => {
            println!("{}", version::version_line());
            Ok(())
        }
        Command::Fetch => fetch::run(&cli.data),
        Command::Report => data::report(&cli.data),
        Command::Score {
            common,
            briefs,
            check_readme,
            write_readme,
        } => {
            let setup = setup(Some(*common))?;
            let briefs = generate::Briefs::from_path(briefs.as_deref())?;
            if *check_readme || *write_readme {
                analysis::check_readme(&setup, &briefs, *common, Path::new("README.md"), *write_readme)
            } else {
                analysis::score(&setup, &briefs, *common)
            }
        }
        Command::Mismatches {
            syllables,
            limit,
            missing,
            after,
        } => {
            analysis::mismatches(&setup(None)?, *syllables, *limit, *missing, *after);
            Ok(())
        }
        Command::Learn { min, limit } => {
            analysis::learn(&setup(None)?, *min, *limit);
            Ok(())
        }
        Command::Explain { word, common, briefs, rebuild } => {
            commands::run_explain(&setup, word.as_deref(), *common, briefs.as_deref(), *rebuild)
        }
        Command::Build {
            out,
            common,
            briefs,
            against,
            report,
        } => {
            let setup = setup(Some(*common))?;
            let briefs = generate::Briefs::from_path(briefs.as_deref())?;
            let against = against
                .as_deref()
                .map(|path| -> Result<_> {
                    let name = path.file_stem().map_or_else(|| "other".to_owned(), |s| s.to_string_lossy().into_owned());
                    Ok((name, data::load_words(path)?))
                })
                .transpose()?;
            generate::build(&setup, &briefs, *common, out, against.as_ref(), *report)
        }
        Command::Quote {
            common,
            briefs,
            words,
            rebuild,
        } => {
            let setup = setup(Some(*common))?;
            let entries = commands::dictionary_entries(&setup, briefs.as_deref(), *common, *rebuild)?;
            quote::run(&setup, &entries, *words)
        }
        Command::Resolve { outline, common, briefs, rebuild } => {
            let options = commands::LookupOptions { stroke_mode: true, ..Default::default() };
            commands::run_lookup(&setup, outline.as_deref(), options, *common, briefs.as_deref(), *rebuild)
        }
        Command::Lookup { query, show_keys, fast, common, briefs, rebuild } => {
            let query = (!query.is_empty()).then(|| query.join(" "));
            let options = commands::LookupOptions { show_keys: *show_keys, fast: *fast, stroke_mode: false };
            commands::run_lookup(&setup, query.as_deref(), options, *common, briefs.as_deref(), *rebuild)
        }
        Command::Paths { word, limit, common, briefs, rebuild } => {
            commands::run_paths(&setup, word.as_deref(), *limit, *common, briefs.as_deref(), *rebuild)
        }
        Command::Family { word, report, common, briefs, rebuild } => {
            commands::run_family(&setup, word.as_deref(), *report, *common, briefs.as_deref(), *rebuild)
        }
        Command::Ergonomics { query, common, briefs, rebuild } => {
            commands::run_ergonomics(&setup, query.as_deref(), *common, briefs.as_deref(), *rebuild)
        }
        Command::Deploy { to, common, briefs } => {
            let setup = setup(Some(*common))?;
            let briefs = generate::Briefs::from_path(briefs.as_deref())?;
            commands::deploy(&setup, &briefs, *common, to)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_version_subcommand_works_but_is_in_no_help() {
        let mut command = Cli::command();
        assert!(Cli::try_parse_from(["lyk-steno-theory", "version"]).is_ok());
        let top = command.render_help().to_string();
        let long = command.render_long_help().to_string();
        for help in [top, long] {
            assert!(!help.lines().any(|l| l.trim_start().starts_with("version")), "{help}");
        }
        assert!(command.get_subcommands().any(|c| c.get_name() == "version" && c.is_hide_set()));
    }

    #[test]
    fn the_version_flag_prints_the_baked_in_version() {
        let err = Cli::try_parse_from(["lyk-steno-theory", "-V"]).err().unwrap();
        assert_eq!(err.to_string().trim(), version::version_line());
    }
}
