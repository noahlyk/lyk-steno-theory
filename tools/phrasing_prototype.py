"""The phrasing grammar of plans/08-phrasing-grammar.md as the spec the generator is checked against:
`entries()` writes every (stroke, text) from the tables below, independently of `rules/grammar.toml`
and `src/phrases/grammar.rs`.

    python3 tools/phrasing_prototype.py [DIR]       # DIR defaults to out/lyk: every stroke of DIR/02-lyk-phrasing.json
                                                    # against the prototype's; exit 1 on any difference
    python3 tools/phrasing_prototype.py --savings BEFORE   # strokes the corpora save, a build without the grammar (BEFORE)
                                                           # against out/lyk

The prototype also drew the first typing trial; that folder is no longer made (`lyk deploy` ships the grammar)."""
import collections, glob, json, os, re, shutil, sys
import lyk_out

# --- the grammar -------------------------------------------------------------------------------

# starter: chord, person ('1s', '2', '3s', 'pl'), text
STARTERS = {
    'I': ('SWR', '1s'), 'you': ('KPWR', '2'), 'he': ('STWH', '3s'), 'she': ('SKWH', '3s'),
    'it': ('KPWH', '3s'), 'we': ('TWR', 'pl'), 'they': ('STWHR', 'pl'), 'there': ('STHR', '3s'),
}
PAST, NOT = 'E', '*'
# modal: vowel keys, present form, past form (None: no past), negative of each
MODALS = {
    'can': ('A', 'can', 'could', "can't", "couldn't"),
    'will': ('O', 'will', 'would', "won't", "wouldn't"),
    'shall': ('U', 'shall', 'should', 'shall not', "shouldn't"),
    'may': ('AO', 'may', 'might', 'may not', 'might not'),
    'must': ('AU', 'must', None, "mustn't", None),
}
# verb: base, 3rd singular, past. Code by the rule in the plan: the last consonant on the right
# bank; if another verb (earlier in this list, which is by frequency) has it, add the first
# consonant; if that is taken too, add -Z.
VERBS = [
    ('say', 'says', 'said', '-S'), ('go', 'goes', 'went', '-G'), ('get', 'gets', 'got', '-T'),
    ('make', 'makes', 'made', '-BG'), ('know', 'knows', 'knew', '-PB'), ('think', 'thinks', 'thought', '-PBG'),
    ('take', 'takes', 'took', '-BGT'), ('see', 'sees', 'saw', '-SZ'), ('come', 'comes', 'came', '-PL'),
    ('want', 'wants', 'wanted', '-PBT'), ('look', 'looks', 'looked', '-BLG'), ('tell', 'tells', 'told', '-L'),
    ('feel', 'feels', 'felt', '-FL'), ('mean', 'means', 'meant', '-PBL'), ('need', 'needs', 'needed', '-D'),
    ('like', 'likes', 'liked', '-BLGZ'), ('hear', 'hears', 'heard', '-R'), ('seem', 'seems', 'seemed', '-PLS'),
    ('find', 'finds', 'found', '-PBD'), ('give', 'gives', 'gave', '-FG'),
]
BE, HAVE, HAVE_BEEN = '-B', '-F', '-FB'

ORDER = 'STKPWHRAO*EUFRPBLGTSDZ'
RIGHT_START = ORDER.index('F')


def chord(left, middle, right):
    """One stroke from its three parts, keys in steno order, with the dash where it is needed."""
    keys = set()
    for i, k in enumerate(ORDER):
        if i < 7 and k in left:
            keys.add(i)
        elif 7 <= i < RIGHT_START and k in middle:
            keys.add(i)
    right_keys = right.lstrip('-')
    pos = RIGHT_START
    for k in right_keys:
        pos = ORDER.index(k, pos)
        keys.add(pos)
        pos += 1
    out = ''.join(ORDER[i] for i in sorted(keys))
    has_middle = any(7 <= i < RIGHT_START for i in keys)
    if right_keys and not has_middle:
        cut = len([i for i in keys if i < 7])
        out = out[:cut] + '-' + out[cut:]
    return out


def be_form(person, past, negative):
    if past:
        word = 'were' if person in ('2', 'pl') else 'was'
        return word + "n't" if negative else word
    word = {'1s': 'am', '2': 'are', 'pl': 'are'}.get(person, 'is')
    if negative:
        return 'am not' if word == 'am' else word + "n't"
    return word


def entries():
    """Every (outline, text) the grammar writes."""
    out = {}
    for subject, (left, person) in STARTERS.items():
        third = person == '3s'
        for modal, spec in [(None, None), *MODALS.items()]:
            for past in (False, True):
                if modal and past and spec[2] is None:
                    continue
                for negative in (False, True):
                    middle = (spec[0] if spec else '') + (PAST if past else '') + (NOT if negative else '')
                    if spec:
                        aux = spec[(4 if past else 3) if negative else (2 if past else 1)]
                    else:
                        aux = None
                    # the verbs: none, be, have, have been, then the lexical verbs
                    verbs = [(None, ''), ('be', BE), ('have', HAVE), ('have been', HAVE_BEEN)]
                    verbs += [(v, code) for v, *_, code in VERBS]
                    for verb, code in verbs:
                        if subject == 'there' and verb not in (None, 'be', 'have been') and not modal:
                            continue
                        if subject == 'there' and verb not in (None, 'be', 'have been'):
                            continue
                        if aux:
                            words = [subject, aux] + ([verb] if verb else [])
                        elif verb is None:
                            do = ('does' if third else 'do') if not past else 'did'
                            if subject == 'there':
                                continue
                            words = [subject, do + "n't" if negative else do]
                        elif verb == 'be':
                            words = [subject, be_form(person, past, negative)]
                        elif verb in ('have', 'have been'):
                            hv = 'had' if past else ('has' if third else 'have')
                            words = [subject, hv + "n't" if negative else hv] + (['been'] if verb == 'have been' else [])
                        else:
                            base, s3, ed = next((b, s, e) for b, s, e, _ in VERBS if b == verb)
                            if negative:
                                do = "didn't" if past else ("doesn't" if third else "don't")
                                words = [subject, do, base]
                            else:
                                words = [subject, ed if past else (s3 if third else base)]
                        out[chord(left, middle, code)] = ' '.join(words)
    return out


# --- checking the generator against it, and what it saves ---------------------------------------

def cross_check(directory):
    """Every stroke the generator wrote to `directory/02-lyk-phrasing.json` against this prototype's:
    prints each difference and returns how many there are."""
    built = json.load(open(os.path.join(directory, '02-lyk-phrasing.json')))
    spec = entries()
    assert len(set(spec.values())) == len(spec), 'two strokes write the same phrase'
    bad = 0
    for outline in sorted(set(spec) | set(built)):
        if spec.get(outline) != built.get(outline):
            bad += 1
            print(f'  {outline}: prototype {spec.get(outline)!r}, generator {built.get(outline)!r}')
    print(f'{len(spec)} prototype strokes, {len(built)} generator strokes, {bad} different')
    return bad


def savings(before_dir, after_dir):
    """Strokes the corpora take walked word by word with the phrases of `before_dir` (a build without the
    grammar), against the same walk over `after_dir` (a build with it): the longest phrase first."""
    before_files = lyk_out.role_files(before_dir)
    after_files = lyk_out.role_files(after_dir)

    def costs(files):
        by_word = lyk_out.outlines_by_word(files)
        return {w.lower(): min(o.count('/') + 1 for rs in roles.values() for o in rs) for w, roles in by_word.items()}

    def pairs(files):
        return {tuple(w.lower().split()): o.count('/') + 1 for o, w in files['phrases'].items()}

    cost_before, cost_after = costs(before_files), costs(after_files)
    pairs_before, pairs_after = pairs(before_files), pairs(after_files)
    phr = {tuple(t.lower().split()): o for o, t in after_files.get('phrasing', {}).items()}
    longest = max(len(p) for p in phr)
    for kind in ('gutenberg', 'quotes'):
        text = ''
        if kind == 'gutenberg':
            for f in glob.glob(lyk_out.path('data', 'corpus', '*.txt')):
                text += open(f).read()
        else:
            q = open(lyk_out.path('src', 'quote_corpus.rs')).read()
            text = '\n'.join(re.findall(r'"((?:[^"\\]|\\.)*)"', q))
        words = re.findall(r"[a-z]+(?:'[a-z]+)?", text.replace('’', "'").lower())

        def walk(cost, pair, use_grammar):
            strokes, i, hits = 0, 0, collections.Counter()
            while i < len(words):
                for n in (range(longest, 1, -1) if use_grammar else ()):
                    if tuple(words[i:i + n]) in phr:
                        strokes += 1
                        hits[tuple(words[i:i + n])] += 1
                        i += n
                        break
                else:
                    if tuple(words[i:i + 2]) in pair:
                        strokes += pair[tuple(words[i:i + 2])]
                        i += 2
                    else:
                        strokes += cost.get(words[i], 2)
                        i += 1
            return strokes, hits
        was, _ = walk(cost_before, pairs_before, False)
        now, hits = walk(cost_after, pairs_after, True)
        print(f"{kind}: {len(words)} words, {was} -> {now} strokes ({100 * (was - now) / was:.2f}% fewer)")
        if kind == 'gutenberg':
            print('commonest in running text:')
            for p, n in hits.most_common(40):
                print(f"  {phr[p]:<12} {' '.join(p)}")


def main():
    args = sys.argv[1:]
    if args[:1] == ['--savings']:
        savings(args[1] if len(args) > 1 else lyk_out.path('out', 'before'), lyk_out.path('out', 'lyk'))
    else:
        sys.exit(1 if cross_check(args[0] if args else lyk_out.path('out', 'lyk')) else 0)


if __name__ == '__main__':
    main()
