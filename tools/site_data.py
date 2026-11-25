#!/usr/bin/env python3
"""Writes the data the lessons site (`site/`) runs on, and checks every outline the lessons teach.

    cargo run --release -- build          # first, so out/lyk is current
    python3 tools/site_data.py            # writes site/data/words.json, chart.json, engine.json, joins.json, notation.json
    python3 tools/site_data.py --check    # only checks the lessons; non-zero exit on any wrong outline

It stops with "out/lyk is older than the rules: run `lyk build`" when `out/lyk/.built` (the source
hash `build` stamps, the check `lyk lookup` makes) is not this checkout's, or when a file or entry it
needs is missing; `--allow-stale` skips the hash check. `LYK_OUT` picks another out folder. The files it
writes are byte-identical between runs on one `out/lyk`.

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
from lyk_out import SPELLED, STALE, default_out_dir, path, require_fresh, role_files, ranks  # noqa: E402

SITE = path('site')
LESSONS = sorted(glob.glob(os.path.join(SITE, 'lessons', '[0-9]*.json')))
# the Reference area: pages with the same blocks as a lesson, but no drill
REFERENCE = sorted(p for p in glob.glob(os.path.join(SITE, 'reference', '*.json')) if not p.endswith('index.json'))
TOP = 12000  # words in the lookup
JOINS_PER_WORD = 12  # backups kept per common word for the engine
# in the order an outline's word is decided when two roles write the same outline
ENGINE_ROLES = ['words', 'short', 'suffixes', 'phrases', 'phrasing', 'extras', 'alternates']
LETTER = re.compile(r'^(\{>\})?\{&.\}$')


def cost(outline):
    return (outline.count('/'), len(outline.replace('-', '').replace('+', '')), outline)


def load_lessons():
    return [json.load(open(f)) for f in LESSONS + REFERENCE]


def grammar_entries(files):
    """The phrasing grammar's strokes, as the built dictionary has them: {phrase: outline}."""
    return {text: outline for outline, text in files.get('phrasing', {}).items()}


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
    # The suffix sheet's entries are the starred chords (`+*D`); the plain chord (`+-D`) is the
    # solver's, which reads it as the suffix after any word, so the site's engine does the same
    for outline, text in tomllib.load(open(path('rules', 'suffixes.toml'), 'rb')).items():
        if outline not in seen:
            seen.add(outline)
            core.setdefault(text, []).append(outline)
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
    suffixes = dict(tomllib.load(open(path('rules', 'suffixes.toml'), 'rb')))
    # the starred chords too: exact suffixes, which the engine glues on with no spelling rules
    suffixes.update(files.get('suffixes', {}))
    joins = {}
    for outline, word in files.get('backups', {}).items():
        if word in pick and typable(outline) and outline not in seen:
            joins.setdefault(word, []).append(outline)
    joins = {w: sorted(o, key=cost)[:JOINS_PER_WORD]
             for w, o in sorted(joins.items(), key=lambda kv: (rank.get(kv[0], 10**9), kv[0]))}
    out = os.path.join(SITE, 'data')
    for name, data in (('engine.json', {'d': core, 'p': reserved, 's': suffixes}), ('joins.json', joins)):
        with open(os.path.join(out, name), 'w') as f:
            json.dump(data, f, separators=(',', ':'))
        print(f'{name}: {os.path.getsize(os.path.join(out, name)):,} bytes')


def write_notation(words, lessons):
    """site/data/notation.json: lyk's short notation (`out/lyk/.keymux-notation.json`, as keymux gets it), for the
    site to show outlines in (site/js/notation.js). Its outlines whose split is known (`agree`, `outlines`) are cut to
    the outlines the site shows: the lookup's and every [[word|OUTLINE]] of a page. Any other outline is shown raw."""
    table = json.load(open(os.path.join(default_out_dir(), '.keymux-notation.json'), encoding='utf-8'))
    shown = {o for outs in words.values() for o in outs}
    for lesson in lessons:
        shown.update(outline for _, outline, _ in lesson_claims(lesson))
    # The dictionary keys spell each known split (`role_files` read them back to raw); the site's
    # own data stays raw, so the spellings of the outlines it shows go beside the table
    # (in `agree` where the table writes them so, as the site reads it, else in `outlines`)
    import notation
    reader = notation.Notation(table)
    agree, own = [], {}
    for raw, key in SPELLED.items():
        if raw not in shown:
            continue
        if '/'.join(reader.table_text(notation.raw_bits(part)) for part in raw.split('/')) == key:
            agree.append(raw)
        else:
            own[raw] = key
    table['agree'] = sorted(agree)
    table['outlines'] = own
    with open(os.path.join(SITE, 'data', 'notation.json'), 'w', encoding='utf-8') as f:
        json.dump(table, f, ensure_ascii=False, separators=(',', ':'), sort_keys=True)
    print(f"notation.json: {len(agree) + len(own)} outlines shown short as their keys spell them ({len(own)} not as the table writes them)")


def main():
    check_only = '--check' in sys.argv
    require_fresh()
    files = role_files()
    for role in ('words', 'short', 'suffixes', 'phrases', 'phrasing', 'extras', 'alternates', 'backups'):
        if not files.get(role):
            sys.exit(f'{STALE}\n  (02-lyk-{role}.json is missing or empty)')
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
    grammar = grammar_entries(files)
    for lesson in lessons:
        for word, outline, where in lesson_claims(lesson):
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
    top = sorted((w for w in by_word if w in rank and w.isalpha()), key=lambda w: (rank[w], w))[:TOP]
    shown = set().union(*(shown_words(l) for l in lessons)) if lessons else set()
    pick = set(top) | {w for w in wanted | shown if w in by_word}
    words = {}
    for w in pick:
        outs = by_word[w]
        main = [o for o, r in outs.items() if r & {'words', 'short', 'suffixes', 'phrases', 'phrasing'}]
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
        json.dump({'words': dict(sorted(words.items(), key=lambda kv: (rank.get(kv[0], 10**9), kv[0])))}, f, separators=(',', ':'))
    print(f'words.json: {len(words)} words')
    write_engine(files, pick, rank)
    write_notation(words, lessons)

    # chart.json
    rules = tomllib.load(open(path('rules', 'plover.toml'), 'rb'))
    chart = {k: rules[k] for k in ('onset', 'vowel', 'coda')}
    chart['onset_cluster'] = rules['onset_cluster']
    chart['coda_cluster'] = rules['coda_cluster']
    chart['vowel_coda'] = rules['vowel_coda']
    with open(os.path.join(SITE, 'data', 'chart.json'), 'w') as f:
        json.dump(chart, f, indent=1)
    print('chart.json written')
    write_grammar(grammar, files.get('phrases', {}))
    sys.exit(1 if bad else 0)


def write_grammar(grammar, merged_pairs):
    """site/data/grammar.json: the grammar's parts (`rules/grammar.toml`), the verb codes the built dictionary
    has, and every stroke it writes, for the composer and the lookup."""
    table = tomllib.load(open(path('rules', 'grammar.toml'), 'rb'))
    starters = [[s['word'], s['chord'], s['person']] for s in table['starter']]
    first_word, first_chord, first_person = starters[0]

    def code(text):
        phrase = f'{first_word} {text}'
        if phrase not in grammar:
            sys.exit(f'{STALE}\n  (the phrasing file has no {phrase!r}, {len(grammar)} strokes)')
        stroke = grammar[phrase]
        assert stroke.startswith(first_chord + '-'), (text, stroke)
        return stroke[len(first_chord):]

    # A question the grammar leaves to a merged pair (`do you`, `are you`) keeps that outline: the composer
    # shows it from here. Only texts that read as a question (an auxiliary, then a starter) are kept.
    starter_words = {w.lower() for w, _, _ in starters}
    aux_words = set()
    for m in table['modal']:
        aux_words |= {w.split()[0] for w in (m['present'], m.get('past'), m['negative'], m.get('past_negative')) if w}
    for form in ('present', 'present_negative', 'past', 'past_negative'):
        aux_words |= {w.split()[0] for w in table['be'][form].values()}
    for aux in (table['have'], table['do']):
        aux_words |= {aux['base'], aux['third'], aux['past']} | {w + aux['negative'] for w in (aux['base'], aux['third'], aux['past'])}
    merged = {}
    for outline, text in merged_pairs.items():
        words = text.lower().split()
        if len(words) >= 2 and words[0] in aux_words and words[1] in starter_words and text not in grammar:
            merged[text] = outline
    third = first_person == '3s'
    have = table['have']
    verbs = [[v['base'], v['third'], v['past'], code(v['third'] if third else v['base'])] for v in table['verb']]
    be = code(table['be']['present'][first_person])
    data = {
        'starters': starters,
        'modals': [[m['name'], m['keys'], m['present'], m.get('past'), m['negative'], m.get('past_negative')] for m in table['modal']],
        'verbs': verbs,
        'special': [['be', be], ['have', code(have['third'] if third else have['base'])],
                    ['have been', code((have['third'] if third else have['base']) + ' been')]],
        'entries': dict(sorted(grammar.items())),
        'question': table['question'],
        'merged': dict(sorted(merged.items())),
    }
    with open(os.path.join(SITE, 'data', 'grammar.json'), 'w') as f:
        json.dump(data, f, separators=(',', ':'))
    print(f'grammar.json: {len(grammar)} strokes ({sum(1 for o in grammar.values() if o.startswith(table["question"]))} questions), {len(merged)} merged pairs read as questions')


if __name__ == '__main__':
    main()
