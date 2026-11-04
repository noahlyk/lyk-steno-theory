"""Who holds an outline, or every outline a word has. Usage:
python3 tools/holders.py [--out DIR] OUTLINE|@word ...

`python3 tools/holders.py TKOBG 'TKO*BG' @dock` prints the word and role file each outline is in,
and every outline `dock` has (fastest first, role in brackets). Quote outlines with a star."""
import sys
import lyk_out

args = sys.argv[1:]
out_dir = None
if args[:1] == ['--out']:
    out_dir, args = args[1], args[2:]
files = lyk_out.role_files(out_dir)
holder = {o: (w, role) for role, d in files.items() for o, w in d.items()}
by_word = lyk_out.outlines_by_word(files)
for arg in args:
    if arg.startswith('@'):
        word = arg[1:]
        found = sorted(((o, role) for role, os in by_word[word].items() for o in os),
                       key=lambda x: (x[0].count('/'), len(x[0]), x[0]))
        print(f"{word:>14}: " + '  '.join(f"{o} ({role})" for o, role in found[:12]))
    else:
        print(f"{arg:>14} -> " + ('{} ({})'.format(*holder[arg]) if arg in holder else 'free'))
