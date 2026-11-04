"""How many words of two syllables or more can be joined in, by frequency band. Usage:
python3 tools/coverage.py [word ...]   (after `build`; a word prints every outline it has)

Columns: words in the band, the share with a one-stroke outline, the share with at least one
joined way in (two strokes or more, not a bare `/+*` marker), the share with a stem-plus-ending
backup, joined ways in per word, and the share with two or more. See docs/joins.md."""
import sys
import lyk_out

ROLES = ['words', 'short', 'backups', 'alternates', 'extras']
files = lyk_out.role_files()
outs = lyk_out.outlines_by_word(files, ROLES)
syl = lyk_out.syllables()
rank = {w: r for w, r in sorted(lyk_out.ranks(10**7).items(), key=lambda x: x[1]) if w in outs}
dense = {w: n + 1 for n, w in enumerate(sorted(rank, key=rank.get))}


def strokes(o):
    return o.count('/') + 1


def joined(o):
    return strokes(o) >= 2 and '/+*' not in o


bands = [(1, 1000), (1001, 5000), (5001, 10000), (10001, 20000), (20001, 50000), (50001, 10**7)]
print(f"{'band':>14} {'n':>6} {'1-str':>6} {'joined':>7} {'stem+':>6} {'paths/w':>8} {'2+paths':>8}")
for lo, hi in bands:
    ws = [w for w, r in dense.items() if lo <= r <= hi and syl.get(w, 0) >= 2]
    n = max(1, len(ws))
    one = sum(1 for w in ws if any(strokes(o) == 1 for r in ('words', 'short', 'extras') for o in outs[w][r]))
    paths = {w: {o for r in ROLES for o in outs[w][r] if joined(o)} for w in ws}
    reach = sum(1 for w in ws if paths[w])
    stem = sum(1 for w in ws if any(joined(o) for o in outs[w]['backups']))
    many = sum(1 for w in ws if len(paths[w]) >= 2)
    avg = sum(len(p) for p in paths.values()) / n
    print(f"{lo:>6}-{hi:<7} {len(ws):>6} {one/n:>6.1%} {reach/n:>7.1%} {stem/n:>6.1%} {avg:>8.2f} {many/n:>8.1%}")
print('entries:', {r: len(d) for r, d in files.items() if r in ROLES})
for w in sys.argv[1:]:
    print(w, dict(outs[w]))
