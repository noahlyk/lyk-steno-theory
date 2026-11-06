#!/usr/bin/env python3
"""Writes the data the lessons site (`site/`) runs on, and checks every outline the lessons teach.

    cargo run --release -- build          # first, so out/lyk is current
    python3 tools/site_data.py            # writes site/data/words.json and site/data/chart.json
    python3 tools/site_data.py --check    # only checks the lessons; non-zero exit on any wrong outline

`words.json` has the commonest words with their fastest outline and a few more ways in (for the
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
TOP = 12000  # words in the lookup


def cost(outline):
    return (outline.count('/'), len(outline.replace('-', '').replace('+', '')), outline)


def load_lessons():
    return [json.load(open(f)) for f in LESSONS]


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
    for lesson in lessons:
        for word, outline, where in lesson_claims(lesson):
            wanted.add(word)
            if outline not in by_word.get(word, {}):
                here = sorted(by_word.get(word, {}), key=cost)[:4]
                bad.append(f'{where}: {word!r} is not {outline} (has {here})')
    for line in bad:
        print('WRONG', line)
    print(f'{len(lessons)} lessons, {len(wanted)} words checked, {len(bad)} wrong')
    if check_only:
        sys.exit(1 if bad else 0)

    # words.json
    rank = ranks()
    top = sorted((w for w in by_word if w in rank and w.isalpha()), key=lambda w: rank[w])[:TOP]
    pick = set(top) | {w for w in wanted if w in by_word}
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

    # chart.json
    rules = tomllib.load(open(path('rules', 'plover.toml'), 'rb'))
    chart = {k: rules[k] for k in ('onset', 'vowel', 'coda')}
    chart['onset_cluster'] = rules['onset_cluster']
    chart['coda_cluster'] = rules['coda_cluster']
    chart['vowel_coda'] = rules['vowel_coda']
    with open(os.path.join(SITE, 'data', 'chart.json'), 'w') as f:
        json.dump(chart, f, indent=1)
    print('chart.json written')
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
