"""Prototype of the phrasing grammar in plans/08-phrasing-grammar.md, for typing it before it is
built into the generator. Usage: python3 tools/phrasing_prototype.py [OUT_DIR=out/grammar-trial]

One stroke = a starter (left bank) + an auxiliary (vowels; the star is "not") + a verb (right
bank). Writes OUT_DIR/02-lyk-grammar.json and copies of out/lyk's role files without the entries
the grammar takes over, so the folder can be deployed as a whole; prints what it displaced, the
strokes it saves on the corpora, and a practice list."""
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


# --- what it does to today's dictionary --------------------------------------------------------

def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else lyk_out.path('out', 'grammar-trial')
    grammar = entries()
    assert len(set(grammar.values())) == len(grammar), 'two strokes write the same phrase'
    files = lyk_out.role_files()
    holders = {o: (w, role) for role, d in files.items() for o, w in d.items()}
    by_word = lyk_out.outlines_by_word(files)
    rank = lyk_out.ranks()
    displaced = sorted(((rank.get(holders[o][0], 10**6), holders[o][0], o, holders[o][1], text)
                        for o, text in grammar.items() if o in holders))
    print(f"{len(grammar)} grammar strokes; {len(displaced)} take a stroke something has today:")
    kinds = collections.Counter(role for _, _, _, role, _ in displaced)
    print('  by role:', dict(kinds))
    stranded = []
    for r, word, o, role, text in displaced:
        others = [x for rs in by_word[word].values() for x in rs if x != o]
        if role != 'phrases' and not others:
            stranded.append(word)
        if role != 'phrases' and r < 20000:
            print(f"  {word:>14} #{r:<6} {o:<10} ({role}) -> {text!r}; still has {lyk_out.fastest(others) if others else 'NOTHING'}")
    print('  words left with no outline:', stranded or 'none')

    # the trial folder: every role file minus what the grammar takes, plus the grammar
    if os.path.isdir(out_dir):
        shutil.rmtree(out_dir)
    os.makedirs(out_dir)
    for role, d in files.items():
        kept = {o: w for o, w in d.items() if o not in grammar}
        json.dump(kept, open(os.path.join(out_dir, f'02-lyk-{role}.json'), 'w'), indent=0, sort_keys=True)
    json.dump(dict(sorted(grammar.items())), open(os.path.join(out_dir, '02-lyk-grammar.json'), 'w'), indent=0)
    print(f"wrote {out_dir}")

    # strokes saved, over today's dictionary with today's phrases
    cost = {}
    for word, roles in by_word.items():
        cost[word.lower()] = min(o.count('/') + 1 for rs in roles.values() for o in rs)
    pairs = {tuple(w.lower().split()): o.count('/') + 1 for o, w in files['phrases'].items()}
    phr = {tuple(t.lower().split()): o for o, t in grammar.items()}
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

        def walk(use_grammar):
            strokes, i, hits = 0, 0, collections.Counter()
            while i < len(words):
                for n in (range(longest, 1, -1) if use_grammar else ()):
                    if tuple(words[i:i + n]) in phr:
                        strokes += 1; hits[tuple(words[i:i + n])] += 1; i += n
                        break
                else:
                    if tuple(words[i:i + 2]) in pairs:
                        strokes += pairs[tuple(words[i:i + 2])]; i += 2
                    else:
                        strokes += cost.get(words[i], 2); i += 1
            return strokes, hits
        before, _ = walk(False)
        after, hits = walk(True)
        print(f"{kind}: {len(words)} words, {before} -> {after} strokes ({100 * (before - after) / before:.2f}% fewer)")
        if kind == 'gutenberg':
            print('practice, commonest first:')
            for p, n in hits.most_common(40):
                print(f"  {phr[p]:<12} {' '.join(p)}")


if __name__ == '__main__':
    main()
