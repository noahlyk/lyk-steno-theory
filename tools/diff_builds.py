"""What changed between two builds. Usage:
python3 tools/diff_builds.py BEFORE_DIR [AFTER_DIR=out/lyk] [N=60]

Copy out/lyk somewhere before a change (`cp -r out/lyk /tmp/before`), rebuild, then run this:
per-file entry and word counts, the words only one build has, and every word whose fastest
outline changed, commonest first, with how many fall in the top 1,000 / 5,000 / 20,000."""
import sys
import lyk_out

before_dir = sys.argv[1]
after_dir = sys.argv[2] if len(sys.argv) > 2 else None
n = int(sys.argv[3]) if len(sys.argv) > 3 else 60
before, after = lyk_out.role_files(before_dir), lyk_out.role_files(after_dir)
for role in sorted(set(before) | set(after)):
    a, b = before.get(role, {}), after.get(role, {})
    print(f"{role:12} entries {len(a):>7} -> {len(b):>7}   words {len(set(a.values())):>6} -> {len(set(b.values())):>6}")

ROLES = {'words', 'short', 'backups', 'alternates', 'extras'}
a_out, b_out = lyk_out.outlines_by_word(before, ROLES), lyk_out.outlines_by_word(after, ROLES)
gone, new = sorted(set(a_out) - set(b_out)), sorted(set(b_out) - set(a_out))
print(f"\nwords only before: {len(gone)}  {' '.join(gone[:30])}")
print(f"words only after:  {len(new)}  {' '.join(new[:30])}")

fast = lambda by: {w: lyk_out.fastest([o for r in roles.values() for o in r]) for w, roles in by.items()}
a_fast, b_fast = fast(a_out), fast(b_out)
rank = lyk_out.ranks()
changed = sorted((w for w in set(a_fast) & set(b_fast) if a_fast[w] != b_fast[w]), key=lambda w: rank.get(w, 10**9))
print(f"\nfastest outline changed: {len(changed)} of {len(a_fast)}", end='')
for cut in (1000, 5000, 20000):
    print(f"   top {cut}: {sum(1 for w in changed if rank.get(w, 10**9) < cut)}", end='')
print()
for w in changed[:n]:
    print(f"{rank.get(w, '-')!s:>7} {w:>16}  {a_fast[w]:>20} -> {b_fast[w]}")
