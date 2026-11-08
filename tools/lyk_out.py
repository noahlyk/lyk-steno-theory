"""Shared loading for the analysis tools: the built dictionary's role files, word ranks, syllable
counts, and the gold dictionaries' word and name sets. Everything is read relative to the repo
root, whatever directory the tool is run from."""
import glob, json, os, re
from collections import defaultdict

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')


def path(*parts):
    return os.path.join(ROOT, *parts)


def role_files(out_dir=None):
    """{role: {outline: word}} for every `02-lyk-<role>.json` in `out_dir` (default out/lyk)."""
    out_dir = out_dir or path('out', 'lyk')
    files = {}
    for f in sorted(glob.glob(os.path.join(out_dir, '02-lyk-*.json'))):
        files[os.path.basename(f)[len('02-lyk-'):-len('.json')]] = json.load(open(f))
    # A suffix is defined as an attach, `{^ed}`; the tools work with its bare text
    files['suffixes'] = {o: (w[2:-1] if w.startswith('{^') and w.endswith('}') else w)
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
