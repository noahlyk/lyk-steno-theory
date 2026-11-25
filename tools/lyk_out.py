"""Shared loading for the analysis tools: the built dictionary's role files, word ranks, syllable
counts, and the gold dictionaries' word and name sets. Everything is read relative to the repo
root, whatever directory the tool is run from."""
import glob, json, os, re
from collections import defaultdict

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')


def path(*parts):
    return os.path.join(ROOT, *parts)


def default_out_dir():
    """The built dictionary folder: `$LYK_OUT/lyk` if set, else `out/lyk` in the checkout."""
    env = os.environ.get('LYK_OUT')
    return os.path.join(env, 'lyk') if env else path('out', 'lyk')


def source_hash(root=ROOT):
    """The hash `build` stamps into `out/lyk/.built`: the same FNV-1a over `src/**/*.rs`, `rules/**`,
    `Cargo.toml` and `Cargo.lock` (relative names and contents, in path order) as `build.rs`."""
    files = []
    for top, only_rs in (('src', True), ('rules', False)):
        for dirpath, _, names in os.walk(os.path.join(root, top)):
            files += [os.path.join(dirpath, n) for n in names if not only_rs or n.endswith('.rs')]
    files += [p for p in (os.path.join(root, 'Cargo.toml'), os.path.join(root, 'Cargo.lock')) if os.path.isfile(p)]
    if not files:
        return 'none'
    h = 0xcbf29ce484222325
    for p in sorted(files, key=lambda p: os.path.relpath(p, root).split(os.sep)):
        name = os.path.relpath(p, root).replace(os.sep, '/').encode()
        with open(p, 'rb') as f:
            data = f.read()
        for chunk in (name, data):
            for b in chunk:
                h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return f'{h:016x}'


STALE = 'out/lyk is older than the rules: run `lyk build` (cargo run --release -- build), then run this again'


def stale_reason(out_dir=None):
    """Why the built dictionary in `out_dir` is not from the rules and generator in this checkout
    (the `lookup` stale note's check: `.built` against the source hash), else None."""
    out_dir = out_dir or default_out_dir()
    if not os.path.isdir(out_dir):
        return f'{out_dir} does not exist'
    try:
        stamp = open(os.path.join(out_dir, '.built')).read()
    except OSError:
        return 'it has no .built stamp (built before builds recorded their source)'
    built = next((line[len('source='):] for line in stamp.splitlines() if line.startswith('source=')), None)
    now = source_hash()
    if built is None:
        return 'its .built stamp names no source'
    if built != now and built != 'none' and now != 'none':
        return 'the rules or the generator changed after it was built'
    return None


def require_fresh(out_dir=None):
    """Exit with a clear message when `out_dir` is stale (`--allow-stale` on the command line skips)."""
    import sys
    if '--allow-stale' in sys.argv:
        return
    why = stale_reason(out_dir)
    if why:
        sys.exit(f'{STALE}\n  ({why})')


# Each outline whose key is spelled short in the files `role_files` read: raw outline -> its key
SPELLED = {}


def role_files(out_dir=None):
    """{role: {outline: word}} for every `02-lyk-<role>.json` in `out_dir` (default out/lyk), each
    outline in raw notation: a key spelled in the short notation (`NĪT`) is read back to its raw
    keys (`TPHAOEUT`) with the notation file beside it, and kept in `SPELLED`."""
    import notation
    out_dir = out_dir or default_out_dir()
    table = notation.load(out_dir)
    files = {}
    for f in sorted(glob.glob(os.path.join(out_dir, '02-lyk-*.json'))):
        entries = json.load(open(f, encoding='utf-8'))
        if table is not None:
            raw_entries = {}
            for key, word in entries.items():
                if key.isascii() and notation.raw_bits(key.split('/')[0]) is not None and table.to_raw(key) == key:
                    raw_entries[key] = word
                    continue
                raw = table.to_raw(key)
                if raw != key:
                    SPELLED[raw] = key
                raw_entries[raw] = word
            entries = raw_entries
        files[os.path.basename(f)[len('02-lyk-'):-len('.json')]] = entries
    # A suffix is defined as an exact attach, `{^}ed`; the tools work with its bare text
    files['suffixes'] = {o: (w[3:] if w.startswith('{^}') else w)
                         for o, w in files.get('suffixes', {}).items()}
    return files


def outlines_by_word(files, roles=None):
    """{word: {role: [outline, ...]}} over `roles` (default every role)."""
    by = defaultdict(lambda: defaultdict(list))
    for role, d in files.items():
        if roles is None or role in roles:
            for outline, word in d.items():
                by[word][role].append(outline)
    return by


def fastest(outlines):
    """Fewest strokes, then fewest keys, then text."""
    return min(outlines, key=lambda o: (o.count('/'), len(o.replace('-', '').replace('+', '')), o))


def ranks(limit=200_000):
    """{word: line number} in the subtitle frequency list (0 is the commonest)."""
    rank = {}
    for i, line in enumerate(open(path('data', 'frequency', 'en_full.txt'))):
        rank.setdefault(line.rsplit(' ', 1)[0], i)
        if i >= limit:
            break
    return rank


def syllables():
    """{word: syllable count of its first CMUdict pronunciation}."""
    syl = {}
    for line in open(path('data', 'pronunciation', 'cmudict.dict')):
        word, *phones = line.split('#')[0].split()
        word = re.sub(r'\(\d+\)$', '', word)
        syl.setdefault(word, sum(1 for p in phones if p[-1].isdigit()))
    return syl


def gold_kinds():
    """(words, names): what Plover or Lapwing write lowercase, and what they write only capitalized
    (lowercased), the same split `Gold::is_name` makes."""
    translations = set()
    for name in ('plover-main', 'lapwing-base'):
        translations.update(json.load(open(path('data', 'dictionaries', f'{name}.json'))).values())
    words = {t for t in translations if t.islower()}
    names = {t.lower() for t in translations if t[:1].isupper() and t[1:].islower() and t.lower() not in words}
    return words, names
