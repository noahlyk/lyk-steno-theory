"""Words whose primary outline ends in a bare `/+*` marker stroke, by frequency band, split into
real words, names and neither (by the gold dictionaries, as `Gold::is_name` splits them), with
who holds the plain and starred stroke each real word wanted. Usage: python3 tools/markers.py [N=40]
See site/reference/collisions.json."""
import re, sys
import lyk_out

n = int(sys.argv[1]) if len(sys.argv) > 1 else 40
files = lyk_out.role_files()
holder = {o: w for d in files.values() for o, w in d.items()}
gold_words, names = lyk_out.gold_kinds()
rank = lyk_out.ranks()
marked = sorted((rank.get(w, 10**9), w, o) for o, w in files['words'].items() if o.endswith('/+*'))


def starred(stroke):
    """The stroke with the star put where steno order puts it: after A and O, before E and U."""
    m = re.fullmatch(r'(\+?S?T?K?P?W?H?R?A?O?)\*?(E?U?)-?(F?R?P?B?L?G?T?S?D?Z?)', stroke)
    return f"{m[1]}*{m[2]}{m[3]}" if m else stroke


for lo, hi in [(0, 5000), (5000, 20000), (20000, 10**9 + 1)]:
    band = [m for m in marked if lo <= m[0] < hi]
    real = [m for m in band if m[1] in gold_words]
    print(f"rank {lo}-{hi if hi < 10**9 else 'end'}: {len(band)} on a marker, {len(real)} real words, "
          f"{sum(1 for m in band if m[1] in names)} names")
    for _, w, o in real[:n]:
        plain = o[:-len('/+*')]
        print(f"  {w:>12} {o:<16} {plain} is {holder.get(plain, '-')}, {starred(plain)} is "
              f"{holder.get(starred(plain), '-')}")
