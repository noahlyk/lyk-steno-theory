"""Simulate letting phrases displace words. Usage: python3 tools/phrase_sim.py [protect_rank=1500] [show=N]
Reads out/lyk/02-lyk-*.json, data/corpus, data/frequency. See plans/06-phrase-briefs.md."""
import json, glob, re, collections, math, sys
import os
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..') + '/'
ORDER = list("STKPWHRAO*EUFRPBLGTSDZ")
LEFT = list("STKPWHR"); VOW = list("AO*EU"); RIGHT = list("FRPBLGTSDZ")

def parse(s):
    """stroke string (no '/') -> frozenset of positional keys (index in ORDER), or None."""
    if '/' in s or '+' in s or '~' in s:
        return None
    keys = set(); i = 0
    seen_vowel = '-' in s
    # find the split: left part until first vowel/star/hyphen
    m = re.match(r'^([STKPWHR]*)([AO*EU]*)(-?)([FRPBLGTSDZ]*)$', s)
    if not m:
        return None
    l, v, h, r = m.groups()
    # greedy left must be in order
    def place(chars, bank, base):
        pos = 0; out = []
        for ch in chars:
            try:
                j = bank.index(ch, pos)
            except ValueError:
                return None
            out.append(base + j); pos = j + 1
        return out
    a = place(l, LEFT, 0); b = place(v, VOW, 7); c = place(r, RIGHT, 12)
    if a is None or b is None or c is None:
        return None
    return frozenset(a + b + c)

def render(ks):
    ks = sorted(ks)
    l = ''.join(ORDER[k] for k in ks if k < 7)
    v = ''.join(ORDER[k] for k in ks if 7 <= k < 12)
    r = ''.join(ORDER[k] for k in ks if k >= 12)
    return l + v + ('-' if (not v and r) else '') + r

words = collections.defaultdict(list); owner = {}
for f in glob.glob(root + 'out/lyk/02-lyk-*.json'):
    role = f.split('02-lyk-')[1][:-5]
    for k, v in json.load(open(f)).items():
        owner[k] = (v, role); words[v].append((k, role))
prim = {w: k for w, l in words.items() for k, r in l if r == 'words' and '/' not in k}
# corpora
G_uni = collections.Counter(); G_pair = collections.Counter()
for f in glob.glob(root + 'data/corpus/*.txt'):
    t = open(f, encoding='utf8', errors='ignore').read().lower()
    w = re.findall(r"[a-z]+(?:'[a-z]+)?", t)
    G_uni.update(w); G_pair.update(zip(w, w[1:]))
W_pair = {}; W_uni = collections.Counter()
for line in open(root + 'data/frequency/bigrams_en.txt', encoding='utf8'):
    p, c = line.rstrip('\n').rsplit('\t', 1); a, b = p.split(' ')
    W_pair[(a, b)] = int(c); W_uni[a] += int(c)
S_uni = {}
for line in open(root + 'data/frequency/en_full.txt', encoding='utf8'):
    p = line.split()
    if len(p) == 2: S_uni[p[0]] = int(p[1])
Gt = sum(G_pair.values()); Wt = sum(W_pair.values()); St = sum(S_uni.values())
def gm(*xs):
    xs = [max(x, 1e-9) for x in xs]
    return math.exp(sum(map(math.log, xs)) / len(xs))
def pair_rate(a, b):   # per-million blended pair rate (geometric mean of the two written corpora)
    return gm(G_pair.get((a, b), 0) / Gt * 1e6 + .01, W_pair.get((a, b), 0) / Wt * 1e6 + .01)
def uni_rate(w):       # per-million blended word rate: gutenberg, subtitles
    return gm(G_uni.get(w, 0) / sum(G_uni.values()) * 1e6 + .01, S_uni.get(w, 0) / St * 1e6 + .01)
PROTECT = int(sys.argv[1]) if len(sys.argv) > 1 else 1500
rank = {w: i for i, (w, _) in enumerate(sorted(S_uni.items(), key=lambda kv: -kv[1]))}
have = {tuple(v.lower().split(' ')) for v in json.load(open(root + 'out/lyk/02-lyk-phrases.json')).values() if v.count(' ') == 1}
STROKE = 25.0
def keys(o): return sum(len(parse(s) or ()) for s in o.split('/'))
cand = set(G_pair) | set(k for k, c in sorted(W_pair.items(), key=lambda kv: -kv[1])[:20000])
rows = []
for a, b in cand:
    if (a, b) in have: continue
    pa = prim.get('I' if a == 'i' else a); pb = prim.get('I' if b == 'i' else b)
    if not pa or not pb: continue
    ka, kb = parse(pa), parse(pb)
    if not ka or not kb or ka & kb: continue
    m = render(ka | kb)
    rate = pair_rate(a, b)
    if rate < 20: continue
    rows.append((rate, a, b, m, owner.get(m)))
rows.sort(reverse=True)
free = [r for r in rows if not r[4]]
taken = [r for r in rows if r[4]]
claimed = set()
chosen = []
for rate, a, b, m, cur in rows:
    if m in claimed: continue
    if cur is None:
        chosen.append((rate, a, b, m, None, 0)); claimed.add(m); continue
    word, role = cur
    if word.count(' ') == 1 and role == 'phrases':
        continue
    if role == 'words' and rank.get(word.lower(), 10**9) < PROTECT:
        continue
    def ocost(o): return STROKE * (o.count('/') + 1) + keys(o)
    alts = [k for k, r in words[word] if k != m and '~' not in k]
    if alts:
        loss = uni_rate(word.lower()) * max(min(ocost(k) for k in alts) - ocost(m), 0)
    else:
        loss = uni_rate(word.lower()) * 10 * STROKE
    gain = rate * STROKE
    if gain > loss:
        chosen.append((rate, a, b, m, cur, gain - loss)); claimed.add(m)
print('protect', PROTECT, 'chosen', len(chosen), 'free', sum(1 for c in chosen if c[4] is None), 'displacing', sum(1 for c in chosen if c[4]))
def cover(pair_ct, total, picks):
    return sum(pair_ct.get((a, b), 0) for _, a, b, *_ in picks) / total * 100
print('extra pairs saved: gutenberg %.2f%%  web bigrams %.2f%%' % (cover(G_pair, Gt, chosen), cover(W_pair, Wt, chosen)))
print('already', 'gutenberg %.2f%% web %.2f%%' % (sum(G_pair.get(h, 0) for h in have) / Gt * 100, sum(W_pair.get(h, 0) for h in have) / Wt * 100))
for c in chosen[:int(sys.argv[2]) if len(sys.argv) > 2 else 0]:
    print(f'{c[1]} {c[2]:8} rate {c[0]:7.0f} {c[3]:9} {c[4] and (c[4][0], c[4][1])}')
