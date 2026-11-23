//! `fetch`: download the data the generator reads into `data/`.
//!
//! American English only. Files that are already there are skipped, so it is safe to run again.

use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::Path;

const CMU: &str = "https://raw.githubusercontent.com/cmusphinx/cmudict/master";
const MFA: &str = "https://github.com/MontrealCorpusTools/mfa-models/releases/download";
const WIKIPRON: &str = "https://raw.githubusercontent.com/CUNY-CL/wikipron/master/data/scrape/tsv";
const PLOVER: &str = "https://raw.githubusercontent.com/openstenoproject/plover/main/plover/assets";
const LAPWING: &str =
    "https://raw.githubusercontent.com/aerickt/plover-lapwing-aio/main/plover_lapwing/dictionaries";
const GUTENBERG: &str = "https://www.gutenberg.org/files";
const RAW: &str = "https://raw.githubusercontent.com";
const MONKEYTYPE: &str = "https://raw.githubusercontent.com/monkeytypegame/monkeytype/master/frontend/static/languages";

/// Monkeytype's English word lists (`{name, words[]}` JSON): `english_450k`, an unordered
/// dictionary, is the word source of `verify --giant`. (The frequency lists went with the typing
/// test to typeladder.)
pub const MONKEYTYPE_LISTS: [&str; 1] = ["english_450k"];

/// The other theories whose dictionaries are free to download, as (file name, source). Magnum's
/// own is sold with the book (Stan's is its derivative); Platinum's is a members' download, and
/// Palantype is another layout.
const THEORIES: [(&str, &str); 15] = [
    ("stan", "{R}/stanographer/steno-dictionaries/master/stanmain.json"),
    // Josiah theory: a Magnum-style write-short dictionary; the outlines on its extra `^` key do not parse here and are skipped
    ("josiah", "{R}/Josiah-tan/Josiah_theory/main/user.json"),
    ("bryan", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/Bryan.json"),
    ("startran", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/StarTran.json"),
    ("stenofoundation", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/StenoFoundation.json"),
    ("stenoway", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/The%20Steno%20Way.json"),
    ("tric", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/Tri-C.json"),
    ("pennclinic", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/clinic%5BPennsylvania%20Clinic%5D.json"),
    ("realwrite", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/realwrite.json"),
    ("rwg", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/rwg%5BRobert%20Walsh%20Gonzales%5D.json"),
    ("sastened", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/sastened%5BSan%20Antonio%20Main%5D.json"),
    ("southcoast", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/southcoastcollege.json"),
    ("stened", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/stened.json"),
    ("stenograph", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/stenograph.json"),
    ("thyra", "{R}/user202729/plover-resources/main/stenovations-dictionaries/json/thyra.json"),
];

/// Where each file goes under the data folder, and where it comes from.
fn sources() -> Vec<(String, String)> {
    let mut files = vec![
        // Pronunciations. CMUdict is ARPAbet with stress digits on the vowels. The MFA
        // dictionary is IPA with several pronunciations per word. WikiPron is IPA from Wiktionary.
        ("pronunciation/cmudict.dict".into(), format!("{CMU}/cmudict.dict")),
        ("pronunciation/cmudict.phones".into(), format!("{CMU}/cmudict.phones")),
        (
            "pronunciation/english_us_mfa.dict".into(),
            format!("{MFA}/dictionary-english_us_mfa-v3.1.0/english_us_mfa.dict"),
        ),
        (
            "pronunciation/wikipron_us_broad.tsv".into(),
            format!("{WIKIPRON}/eng_latn_us_broad.tsv"),
        ),
        // How common each word is (OpenSubtitles 2018), to decide who gets the short outlines
        (
            "frequency/en_full.txt".into(),
            "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_full.txt"
                .into(),
        ),
        // How often two words appear next to each other (Norvig's Google Books Ngrams), for the
        // phrase-brief work this only lays the groundwork for: see `data::load_bigrams`.
        (
            "frequency/bigrams_en.txt".into(),
            "https://www.norvig.com/ngrams/count_2w.txt".into(),
        ),
        // The dictionaries to match
        ("dictionaries/plover-main.json".into(), format!("{PLOVER}/main.json")),
        ("dictionaries/plover-commands.json".into(), format!("{PLOVER}/commands.json")),
    ];
    for name in [
        "lapwing-base",
        "lapwing-commands",
        "lapwing-numbers",
        "lapwing-prefixed-proper-nouns",
        "lapwing-proper-nouns",
    ] {
        files.push((
            format!("dictionaries/{name}.json"),
            format!("{LAPWING}/{name}.json"),
        ));
    }
    // Monkeytype's 450k English word list (GPL-3.0 repository), for `verify --giant`
    for name in MONKEYTYPE_LISTS {
        files.push((format!("wordlists/monkeytype-{name}.json"), format!("{MONKEYTYPE}/{name}.json")));
    }
    // Other theories' dictionaries, each a pool of gold outlines for the generator to weigh
    // (`Gold::extra`): the Magnum derivative Stan's, and the court-reporting theories Stenovations
    // published (StenEd, StarTran, Steno Foundation, Tri-C, RealWrite, Robert Walsh Gonzales, ...).
    for (name, url) in THEORIES {
        files.push((format!("dictionaries/theory-{name}.json"), url.replace("{R}", RAW)));
    }
    // Public-domain (pre-1929, no translator/editor claim) connected prose for
    // `quote_corpus`'s larger, non-hand-picked benchmark corpus (see `src/corpus_extract.rs`
    // and `docs/decisions.md`): five books across different registers and eras, the same way the
    // 103 hand-picked passages already lean on Austen/Twain/Dickens/Melville/Thoreau in miniature.
    for (slug, gutenberg_id) in [
        ("pride-and-prejudice", 1342), // Jane Austen, 1813
        ("huckleberry-finn", 76),      // Mark Twain, 1884
        ("tale-of-two-cities", 98),    // Charles Dickens, 1859
        ("moby-dick", 2701),           // Herman Melville, 1851
        ("walden", 205),               // Henry David Thoreau, 1854
    ] {
        files.push((
            format!("corpus/{slug}.txt"),
            format!("{GUTENBERG}/{gutenberg_id}/{gutenberg_id}-0.txt"),
        ));
    }
    files
}

pub fn run(root: &Path) -> Result<()> {
    for (path, url) in sources() {
        let target = root.join(&path);
        if fs::metadata(&target).is_ok_and(|m| m.len() > 0) {
            println!("have  {path}");
            continue;
        }
        println!("fetch {path}");
        download(&url, &target).with_context(|| format!("{url} -> {path}"))?;
    }
    Ok(())
}

/// Download to a temporary name and rename, so a failed download never leaves half a file.
fn download(url: &str, target: &Path) -> Result<()> {
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = target.with_extension("part");
    let response = ureq::get(url).call()?;
    let mut file = fs::File::create(&part)?;
    io::copy(&mut response.into_reader(), &mut file)?;
    fs::rename(&part, target)?;
    Ok(())
}
