# lyk's modifier chords: an OS key press (Ctrl+C, Alt+Tab, Shift+B, F5, arrows, volume ...) as
# one stroke. Written for lyk. The layout is the community's "Emily's modifier chords" scheme,
# so strokes already in a typist's hands carry over; none of this file is taken from another
# project's code.
#
# A stroke is  [left-hand chord choosing the key] [modifier keys F R P B] LGTS.
# The ender LGTS is on every stroke. The modifier keys before it say which of Ctrl (F), Shift
# (R), Super (P), Alt (B) are held (at least one is). The left-hand chord, with the vowel keys
# and the star, selects the key itself:
#   * no A/O pair, no star    a letter (the same chords as fingerspelling; y is KWH)
#   * AO, no star, no E/U     a digit (left bottom row counted in binary), or an F-key with
#                             T and P also held (F1-F12 only)
#   * a star                  a symbol or special key; A and O pick one of four variants
#   * nothing at all          the modifiers alone
#
# `python3 regenerate.py` turns this module into emilys-modifiers.json, which keymux loads.
# (regenerate.py asks a dictionary module lookup() about strokes; this one answers from the
# table it builds below.)

LONGEST_KEY = 1
# The dictionary keymux loads keeps its established file name.
OUTPUT_NAME = "emilys-modifiers"

# Every entry starts with the first-up marker: the chord is sent when its first non-exempt key
# comes up, the exempt keys being the held right-hand base (modifier keys and the ender).
MARKER = "{:firstup:except=-FRPBLGTS}"

# Modifier keys in stroke order, each with the name it sends.
MODIFIER_KEYS = (("F", "control"), ("R", "shift"), ("P", "super"), ("B", "alt"))
# They wrap the key innermost first.
WRAP_ORDER = ("shift", "alt", "super", "control")
ENDER = "LGTS"

# Letters: the chord of each. Vowel keys are the middle of the stroke.
LETTERS = {
    "a": "A", "b": "PW", "c": "KR", "d": "TK", "e": "E", "f": "TP", "g": "TKPW", "h": "H",
    "i": "EU", "j": "SKWR", "k": "K", "l": "HR", "m": "PH", "n": "TPH", "o": "O", "p": "P",
    "q": "KW", "r": "R", "s": "S", "t": "T", "u": "U", "v": "SR", "w": "W", "x": "KP",
    "y": "KWH", "z": "STKPW",
}

# Left-hand keys that carry a binary weight for digits and F-keys (steno order S T K P W R).
BOTTOM_ROW = (("S", 8), ("K", 4), ("W", 2), ("R", 1))

# Star chords: left-hand consonants -> the key sent plain, with A, with O, with A and O.
# None where that variant is not a key; a short tuple has no vowel variants.
STAR_KEYS = {
    "": (None, "return", "tab", "space"),
    "TR": (None, "backspace", "delete", "escape"),
    "KPWR": ("up", "left", "right", "down"),
    "TKPWR": ("page_up", "home", "end", "page_down"),
    "TKW": ("AudioMute", "AudioLowerVolume", "AudioRaiseVolume", None),
    "TPH": ("parenleft", "less", "bracketleft", "braceleft"),
    "KWR": ("parenright", "greater", "bracketright", "braceright"),
    "TK": ("exclam",), "TP": ("quotedbl",), "TKHR": ("numbersign",), "KPWH": ("dollar",),
    "TKPW": ("percent",), "TWR": ("ampersand",), "T": ("apostrophe",), "H": ("asterisk",),
    "R": ("plus",), "W": ("comma",), "PH": ("minus",), "K": ("period",), "KP": ("slash",),
    "HR": ("colon",), "KW": ("semicolon",), "PWHR": ("equal",), "TPW": ("question",),
    "TKPWHR": ("at",), "TW": ("backslash",), "KPR": ("asciicircum",), "WR": ("underscore",),
    "P": ("grave",), "PW": ("bar",), "TPWR": ("asciitilde",),
}


# Two strokes the dictionary has always written with one extra key, kept as shipped so no
# stroke in use moves: the rule would give the key on the left, the dictionary has the one on
# the right. (Remove an entry to move that stroke to its plain form.)
SHIPPED_WITH_EXTRA_KEY = {
    "STKPW-FRPBLGTS": "STKPWHR-FRPBLGTS",  # z with all four modifiers
    "HR*PBLGTS": "HR*EPBLGTS",  # colon with Super+Alt
}


def bottom_row(count):
    """The bottom-row keys that spell `count` in binary."""
    return [key for key, weight in BOTTOM_ROW if count & weight]


def key_chords():
    """Every key the dictionary can press, as (left consonants, middle keys, key name).
    The key name is what the {#...} command presses; None is the modifiers alone."""
    chords = [("", "", None)]
    for name, chord in LETTERS.items():
        chords.append(("".join(k for k in chord if k not in "AOEU"), "".join(k for k in chord if k in "AOEU"), name))
    for digit in range(10):
        chords.append(("".join(bottom_row(digit)), "AO", str(digit)))
    for number in range(1, 13):
        # T and P are always down; the count's keys are added around them in steno order
        keys = bottom_row(number)
        left = ("S" if "S" in keys else "") + "T" + ("K" if "K" in keys else "") + "P"
        left += "".join(k for k in keys if k in "WR")
        chords.append((left, "AO", f"F{number}"))
    for consonants, names in STAR_KEYS.items():
        for vowels, name in zip(("", "A", "O", "AO"), names):
            if name:
                chords.append((consonants, vowels + "*", name))
    return chords


def modifier_sets():
    """Every non-empty combination of the four modifier keys, as (keys in stroke order,
    names wrapped innermost first)."""
    combos = []
    for mask in range(1, 16):
        held = [pair for i, pair in enumerate(MODIFIER_KEYS) if mask >> i & 1]
        names = [name for _, name in held]
        combos.append(("".join(key for key, _ in held), sorted(names, key=WRAP_ORDER.index)))
    return combos


def stroke(consonants, middle, modifier_keys):
    """The stroke as Plover writes it: a hyphen separates the hands when nothing sits between."""
    return consonants + ("" if middle else "-") + middle + modifier_keys + ENDER


def build():
    table = {}
    for consonants, middle, key in key_chords():
        for modifier_keys, names in modifier_sets():
            text = key or ""
            for name in names:
                text = f"{name}({text})"
            table[stroke(consonants, middle, modifier_keys)] = f"{MARKER}{{^}}{{#{text}}}{{^}}"
    return {SHIPPED_WITH_EXTRA_KEY.get(outline, outline): text for outline, text in table.items()}


ENTRIES = build()


def lookup(strokes):
    return ENTRIES[strokes[0]]
