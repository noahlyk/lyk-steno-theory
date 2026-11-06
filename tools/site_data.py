#!/usr/bin/env python3
"""Writes the data the lessons site (`site/`) runs on, and checks every outline the lessons teach.

    cargo run --release -- build          # first, so out/lyk is current
    python3 tools/site_data.py            # writes site/data/words.json, chart.json, engine.json, joins.json
    python3 tools/site_data.py --check    # only checks the lessons; non-zero exit on any wrong outline

`engine.json` and `joins.json` are what `site/js/engine.js` translates strokes with (the whole
dictionary less the join-built backups, then the 12 cheapest backups of each common word, both
word -> outlines, plus the reserved strokes and `rules/suffixes.toml`). `words.json` has the commonest words with their fastest outline and a few more ways in (for the
lookup page), plus every word a lesson names. `chart.json` is the key chart, read from
`rules/plover.toml` so it cannot drift from the rules. The check reads every `[[word|OUTLINE]]`
and every drill item of a lesson, and fails if the dictionary does not write that word that way.
"""
import glob, json, os, re, sys, tomllib
from collections import defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lyk_out import path, role_files, ranks  # noqa: E402

SITE = path('site')
LESSONS = sorted(glob.glob(os.path.join(SITE, 'lessons', '[0-9]*.json')))
# the Reference area: pages with the same blocks as a lesson, but no drill
REFERENCE = sorted(p for p in glob.glob(os.path.join(SITE, 'reference', '*.json')) if not p.endswith('index.json'))
TOP = 12000  # words in the lookup
JOINS_PER_WORD = 12  # backups kept per common word for the engine
# in the order an outline's word is decided when two roles write the same outline
ENGINE_ROLES = ['words', 'short', 'suffixes', 'phrases', 'extras', 'alternates']
LETTER = re.compile(r'^(\{>\})?\{&.\}$')


def cost(outline):
    return (outline.count('/'), len(outline.replace('-', '').replace('+', '')), outline)


def load_lessons():
    return [json.load(open(f)) for f in LESSONS + REFERENCE]


def grammar_entries():
    """The phrasing grammar's strokes: {phrase: outline}. It is not in the dictionary yet (plans/08), so its
    claims are written `[[=it was|KPWHEB]]` and checked against the generator's prototype instead."""
    import phrasing_prototype as g
    return {text: outline for outline, text in g.entries().items()}


TOKEN = re.compile(r'\[\[(.+?)\]\]')


def lesson_claims(lesson):
    """Every (word, outline, where) a lesson says is a word's outline."""
    claims = []

    def scan(text, where):
        for m in TOKEN.finditer(text):
            if '|' in m.group(1):
                word, outline = m.group(1).split('|', 1)
                claims.append((word, outline, where))

    def walk(value, where):
        if isinstance(value, str):
            scan(value, where)
        elif isinstance(value, list):
            for v in value:
                walk(v, where)
        elif isinstance(value, dict):
            for v in value.values():
                walk(v, where)

    walk(lesson['blocks'], lesson['id'])
    drill = lesson.get('drill')
    blocks = [b for b in lesson['blocks'] if b.get('t') == 'try'] + ([drill] if drill else [])
    for b in blocks:
        if b.get('check', 'dict') == 'grammar':
            for item in b['items']:
                claims.append(('=' + item[0], item[1], f"{lesson['id']} drill"))
        if b.get('check', 'dict') != 'dict':
            continue
        for item in b['items']:
            if isinstance(item, dict):
                prompt, outlines, word = item['p'], item['o'], item.get('w', item['p'])
            else:
                prompt, outlines, word = item[0], item[1:], item[0]
            for o in outlines:
                claims.append((word, o, f"{lesson['id']} drill"))
    return claims


SHOWN = re.compile(r'`([a-z][a-z\'-]+)`|\{\{([a-z][a-z\'-]+)\}\}')


def shown_words(lesson):
    """Lower-case words the page shows as code, as {{italic}} or as a whole table cell or list item:
    the site's hover board (site/js/hover.js) puts their outline on them, so words.json must hold them."""
    found = set()

    def cell(text):
        for part in re.split(r'\s*[,;]\s*|\s+/\s+', text.strip()):
            if re.fullmatch(r"[a-z][a-z'-]+", part):
                found.add(part)

    def walk(value):
        if isinstance(value, str):
            for m in SHOWN.finditer(value):
                found.add(m.group(1) or m.group(2))
        elif isinstance(value, list):
            for v in value:
                walk(v)
        elif isinstance(value, dict):
            for k, v in value.items():
                if k == 'rows':
                    for row in v:
                        for c in row:
                            cell(str(c))
                elif k == 'items' and lesson_blocks_list(v):
                    for i in v:
                        cell(i)
                else:
                    walk(v)
    walk(lesson['blocks'])
    return found


def lesson_blocks_list(items):
    return isinstance(items, list) and all(isinstance(i, str) for i in items)


def typable(outline):
    """The site's board has no number bar or spelling marker, so outlines using them cannot be struck."""
    return '#' not in outline and '~' not in outline


def write_engine(files, pick, rank):
    """site/data/engine.json and joins.json: what the stroke-to-text engine looks words up in."""
    core = {}
    seen = set()
    for role in ENGINE_ROLES:
        for outline, word in files.get(role, {}).items():
            if outline in seen or not typable(outline):
                continue
            seen.add(outline)
            core.setdefault(word, []).append(outline)
    for word in core:
        core[word].sort(key=cost)
    reserved = {}
    for name in ('punctuation', 'commands', 'emilys-modifiers'):
        for outline, value in json.load(open(path('rules', 'reserved', f'{name}.json'))).items():
            if not typable(outline):
                continue
            if name == 'emilys-modifiers' and not LETTER.match(value):
                continue
            reserved.setdefault(outline, value)
    suffixes = tomllib.load(open(path('rules', 'suffixes.toml'), 'rb'))
    joins = {}
    for outline, word in files.get('backups', {}).items():
        if word in pick and typable(outline) and outline not in seen:
            joins.setdefault(word, []).append(outline)
    joins = {w: sorted(o, key=cost)[:JOINS_PER_WORD]
             for w, o in sorted(joins.items(), key=lambda kv: rank.get(kv[0], 10**9))}
    out = os.path.join(SITE, 'data')
    for name, data in (('engine.json', {'d': core, 'p': reserved, 's': suffixes}), ('joins.json', joins)):
        with open(os.path.join(out, name), 'w') as f:
            json.dump(data, f, separators=(',', ':'))
        print(f'{name}: {os.path.getsize(os.path.join(out, name)):,} bytes')


def main():
    check_only = '--check' in sys.argv
    files = role_files()
    by_word = defaultdict(lambda: defaultdict(set))  # word -> outline -> roles
    for role, d in files.items():
        for outline, word in d.items():
            by_word[word][outline].add(role)
    for f in glob.glob(path('rules', 'reserved', '*.json')):
        for outline, word in json.load(open(f)).items():
            by_word[word][outline].add('reserved')

    lessons = load_lessons()
    bad = []
    wanted = set()
    grammar = grammar_entries()
    for lesson in lessons:
        for word, outline, where in lesson_claims(lesson):
            if word.startswith('='):
                if grammar.get(word[1:]) != outline:
                    bad.append(f"{where}: grammar phrase {word[1:]!r} is not {outline} (the grammar writes {grammar.get(word[1:])})")
                continue
            wanted.add(word)
            if outline not in by_word.get(word, {}):
                here = sorted(by_word.get(word, {}), key=cost)[:4]
                bad.append(f'{where}: {word!r} is not {outline} (has {here})')
    for line in bad:
        print('WRONG', line)
    print(f'{len(lessons)} lessons and reference pages, {len(wanted)} words checked, {len(bad)} wrong')
    if check_only:
        sys.exit(1 if bad else 0)

    # words.json
    rank = ranks()
    top = sorted((w for w in by_word if w in rank and w.isalpha()), key=lambda w: rank[w])[:TOP]
    shown = set().union(*(shown_words(l) for l in lessons)) if lessons else set()
    pick = set(top) | {w for w in wanted | shown if w in by_word}
    words = {}
    for w in pick:
        outs = by_word[w]
        main = [o for o, r in outs.items() if r & {'words', 'short', 'suffixes', 'phrases'}]
        extra = [o for o, r in outs.items() if r & {'extras', 'alternates'} and o not in main]
        joins = [o for o, r in outs.items() if r & {'backups'} and o not in main and o not in extra]
        row = []
        if main:
            row.append(min(main, key=cost))
        row += sorted(extra, key=cost)[:2]
        row += sorted(joins, key=cost)[:3]
        if row:
            words[w] = row
    os.makedirs(os.path.join(SITE, 'data'), exist_ok=True)
    with open(os.path.join(SITE, 'data', 'words.json'), 'w') as f:
        json.dump({'words': dict(sorted(words.items(), key=lambda kv: rank.get(kv[0], 10**9)))}, f, separators=(',', ':'))
    print(f'words.json: {len(words)} words')
    write_engine(files, pick, rank)

    # chart.json
    rules = tomllib.load(open(path('rules', 'plover.toml'), 'rb'))
    chart = {k: rules[k] for k in ('onset', 'vowel', 'coda')}
    chart['onset_cluster'] = rules['onset_cluster']
    chart['coda_cluster'] = rules['coda_cluster']
    chart['vowel_coda'] = rules['vowel_coda']
    with open(os.path.join(SITE, 'data', 'chart.json'), 'w') as f:
        json.dump(chart, f, indent=1)
    print('chart.json written')
    write_grammar(grammar)
    sys.exit(1 if bad else 0)


def write_grammar(grammar):
    """site/data/grammar.json: the grammar's parts and every stroke it writes, for the composer and its drill."""
    import phrasing_prototype as g
    data = {
        'starters': [[w, c, p] for w, (c, p) in g.STARTERS.items()],
        'modals': [[m, *spec] for m, spec in g.MODALS.items()],
        'verbs': [[b, s3, ed, code] for b, s3, ed, code in g.VERBS],
        'special': [['be', g.BE], ['have', g.HAVE], ['have been', g.HAVE_BEEN]],
        'entries': dict(sorted(grammar.items())),
    }
    with open(os.path.join(SITE, 'data', 'grammar.json'), 'w') as f:
        json.dump(data, f, separators=(',', ':'))
    print(f'grammar.json: {len(grammar)} strokes')


if __name__ == '__main__':
    main()
