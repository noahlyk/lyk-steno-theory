"""lyk's short notation read in Python, for the tools: the dictionary files' keys are spelled short
where the split is known (`"NĪT": "night"`), raw where it is not, and the tools work on raw
outlines. The same reading as `src/notation.rs` (`Notation::parse_stroke`) and `site/js/notation.js`:
text that is exactly raw notation of what it parses to is raw; otherwise the short form (`+`, `~`
and `#` anywhere, the left hand up to the first vowel symbol, vowel key, star or `-`, the vowel
part, then the right hand, each part read greedily, longest token first); otherwise raw read
leniently."""
import json, os

KEYS = '#STKPWHRAO*EUFRPBLGTSDZ'
JOIN, SPELL, NUMBER = 1 << 23, 1 << 24, 1
RIGHT_START = 13
LEFT = 0b1111_1110
VOWELS = (0b11 << 8) | (0b11 << 11)
STAR = 1 << 10
MIDDLE = VOWELS | STAR
RIGHT = 0b11_1111_1111 << 13


def raw_bits(text):
    """A raw stroke (`TPHAEPL`, `+-D`) as bits, read leniently; None if it is not one."""
    bits, rest = 0, text
    if rest.startswith('+'):
        bits, rest = JOIN, rest[1:]
    elif rest.startswith('~'):
        bits, rest = SPELL, rest[1:]
    cursor = 0
    for c in rest:
        if c == '-':
            cursor = max(cursor, RIGHT_START)
            continue
        at = KEYS.find(c, cursor)
        if at < 0:
            return None
        bits |= 1 << at
        cursor = at + 1
    return bits if (bits & ~(JOIN | SPELL)) or bits == JOIN else None


def raw_text(bits):
    """Bits as raw notation, as lyk writes them."""
    out = '+' if bits & JOIN else '~' if bits & SPELL else ''
    hyphen = (bits & (0b11111 << 8)) == 0
    for i in range(23):
        if not bits & (1 << i):
            continue
        if hyphen and i >= RIGHT_START:
            out += '-'
            hyphen = False
        out += KEYS[i]
    return out


def _letters(bits):
    return raw_text(bits).replace('-', '')


class Notation:
    """The symbol table of `.keymux-notation.json`, read as lyk reads it."""

    def __init__(self, table):
        def hand(obj, mask, dash):
            out = []
            for keys, letters in (obj or {}).items():
                bits = raw_bits(dash + keys)
                text = str(letters).upper()
                if bits is None or bits & ~mask or not text or any(b == bits or t == text for b, t in out):
                    continue
                out.append((bits, text))
            return out

        self.left = hand(table.get('left'), LEFT, '')
        self.right = hand(table.get('right'), RIGHT, '-')
        self.middle = hand(table.get('middle'), VOWELS, '')
        ascii_ = list((table.get('ascii') or {}).items())

        def tokens(aliases, mask):
            out = [(text, keys) for keys, text in aliases]
            out += [(KEYS[i], 1 << i) for i in range(23) if mask & (1 << i)]
            for symbol, spelled in ascii_:
                alias = next((k for k, t in aliases if t == symbol), None)
                if alias is not None and not any(t == spelled for _, t in aliases):
                    out.append((spelled, alias))
            return sorted(out, key=lambda t: -len(t[0]))

        self.left_tokens = tokens(self.left, LEFT)
        self.right_tokens = tokens(self.right, RIGHT)
        middle = [(text, keys) for keys, text in self.middle]
        for symbol, spelled in ascii_:
            alias = next((k for k, t in self.middle if t == symbol), None)
            if alias is not None and not any(t == spelled for _, t in self.middle):
                middle.append((spelled, alias))
        middle += [(KEYS[i], 1 << i) for i in range(23) if MIDDLE & (1 << i)]
        middle.append(('-', 0))
        self.middle_tokens = sorted(middle, key=lambda t: -len(t[0]))
        self.middle_chars = {c for text, _ in self.middle_tokens for c in text}
        self._cache = {}

    @staticmethod
    def _read(text, tokens):
        out = []
        while text:
            for token, keys in tokens:
                if text.startswith(token):
                    out.append(keys)
                    text = text[len(token):]
                    break
            else:
                return None
        return out

    def _short(self, text):
        bits, rest = 0, ''
        for ch, key in ((c, {'+': JOIN, '~': SPELL, '#': NUMBER}.get(c)) for c in text):
            if key is None:
                rest += ch
            elif bits & key:
                return None
            else:
                bits |= key
        start = next((i for i, c in enumerate(rest) if c in self.middle_chars), len(rest))
        end = next((i for i in range(start, len(rest)) if rest[i] not in self.middle_chars), len(rest))
        left = self._read(rest[:start], self.left_tokens)
        middle = self._read(rest[start:end], self.middle_tokens)
        right = self._read(rest[end:], self.right_tokens)
        if left is None or middle is None or right is None or middle.count(0) > 1:
            return None
        taken = 0
        for keys in middle:
            if keys & taken:
                return None
            taken |= keys
        for keys in left + middle + right:
            bits |= keys
        return bits if bits else None

    def stroke(self, text):
        """One stroke, raw or short, as bits; None if it reads as neither."""
        if text in self._cache:
            return self._cache[text]
        raw = raw_bits(text)
        if raw is not None and raw_text(raw) == text:
            out = raw
        else:
            short = self._short(text)
            out = short if short is not None else raw
        self._cache[text] = out
        return out

    @staticmethod
    def _tokens(aliases, bits):
        """One hand's keys as the table writes them: from the lowest key left, the alias with the
        most keys that starts there, is all down and has no key still to write inside its span."""
        out, left = [], bits
        while left:
            low = left & -left
            best = None
            for keys, text in aliases:
                span = (1 << keys.bit_length()) - (keys & -keys)
                if keys & -keys == low and keys & ~left == 0 and left & span & ~keys == 0 and (best is None or bin(keys).count('1') > bin(best[0]).count('1')):
                    best = (keys, text)
            keys, text = best or (low, _letters(low))
            out.append(text)
            left &= ~keys
        return ''.join(out)

    def table_text(self, bits):
        """One stroke as the table alone writes it (`Notation::stroke` in lyk)."""
        out = ''.join(t for k, t in ((JOIN, '+'), (SPELL, '~'), (NUMBER, '#')) if bits & k)
        out += self._tokens(self.left, bits & LEFT)
        if bits & MIDDLE:
            vowels = bits & VOWELS
            mark = next((t for k, t in self.middle if k == vowels and vowels), None)
            if mark is not None:
                out += mark + ('*' if bits & STAR else '')
            else:
                out += ''.join(KEYS[i] for i in range(23) if bits & MIDDLE & (1 << i))
        elif bits & RIGHT:
            out += '-'
        return out + self._tokens(self.right, bits & RIGHT)

    def to_raw(self, key):
        """A dictionary key, raw or short, as raw notation; the key itself if it reads as no outline."""
        strokes = [self.stroke(part) for part in key.split('/')]
        return key if any(s is None for s in strokes) else '/'.join(raw_text(s) for s in strokes)


def load(out_dir):
    """The notation `build` wrote into `out_dir`; None before any build."""
    try:
        return Notation(json.load(open(os.path.join(out_dir, '.keymux-notation.json'), encoding='utf-8')))
    except OSError:
        return None
