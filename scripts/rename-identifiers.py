#!/usr/bin/env python3
"""Mechanical identifier rename, with the guards this repo's breakage taught.

The workspace is being converted to English identifiers crate by crate
(CLAUDE.md, language policy). Doing that with a bare `sed` is how you
take production down, so this exists instead.

1. STRING LITERALS MUST NOT CHANGE. A blanket regex renamed a SQL column
   inside a query string and 500'd every role endpoint. Every literal is
   collected before and after and compared byte for byte.

2. SERDE FIELDS ARE A WIRE CONTRACT. Renaming a field on a struct that
   derives Serialize/Deserialize silently changes the JSON a client
   sends or reads -- and guard 1 cannot see it, because the field name
   is code, not a string. Those are reported, never renamed silently.

Three things it does NOT know, which are yours to check:

  - Cross-crate names. A map applied to a dependent will happily rename
    a function that belongs to a crate you have not converted yet. Build
    after every run.
  - Database columns. Applied migrations are append-only, so a column
    keeps its name; map it to an English field with `r.get("old_name")`.
  - Whether the new name is right. It only guarantees the rename is
    faithful, not that it is good.

Usage:
    scripts/rename-identifiers.py map.tsv $(find crates/X/src -name '*.rs')

map.tsv is one `old<TAB>new` per line; `#` comments are ignored.
"""
import re, sys, pathlib

# Raw strings FIRST: `r#"..."#` has no escapes and may contain bare
# quotes, so a plain-string regex would end it at the first inner `"`
# and treat the rest of a JSON schema as code. That is how a `COMPANY`
# placeholder got renamed on one side of a `.replace()` and not the
# other -- caught by a test, not by the guard that was supposed to.
STR = re.compile(r'r(#*)"(?:(?!"\1).)*"\1|"(?:[^"\\\n]|\\.)*"', re.S)
DERIVE = re.compile(r'#\[derive\([^)]*\b(Serialize|Deserialize)\b[^)]*\)\]')


def serde_fields(src):
    """Field names of every struct deriving Serialize/Deserialize."""
    out = set()
    for m in DERIVE.finditer(src):
        brace = src.find("{", m.end())
        if brace < 0:
            continue
        depth, i = 0, brace
        while i < len(src):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        body = src[brace : i + 1]
        for f in re.finditer(r'^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:', body, re.M):
            out.add(f.group(1))
    return out


CAPTURE = re.compile(r"\{([A-Za-z_][A-Za-z0-9_]*)([:}])")


def captures(seg, pairs):
    """Inline format captures -- `{name}` -- live INSIDE a string literal,
    so the literal guard protects them while the identifier they name
    moves out from under them. They are the one thing in a string that
    must be renamed with the code."""
    lookup = dict(pairs)
    return CAPTURE.sub(lambda m: "{%s%s" % (lookup.get(m.group(1), m.group(1)), m.group(2)), seg)


def rename(src, pairs):
    parts, last = [], 0
    for m in STR.finditer(src):
        seg = src[last : m.start()]
        for a, b in pairs:
            seg = re.sub(r"\b%s\b" % re.escape(a), b, seg)
        parts.append(seg)
        parts.append(captures(m.group(0), pairs))
        last = m.end()
    seg = src[last:]
    for a, b in pairs:
        seg = re.sub(r"\b%s\b" % re.escape(a), b, seg)
    parts.append(seg)
    return "".join(parts)


def main():
    pairs = []
    for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            a, b = line.split("\t")
            pairs.append((a.strip(), b.strip()))
    wire = {}
    changed = 0
    for f in sys.argv[2:]:
        p = pathlib.Path(f)
        src = p.read_text()
        fields = serde_fields(src)
        hits = sorted(fields & {a for a, _ in pairs})
        if hits:
            wire[f] = hits
        out = rename(src, pairs)
        before = [captures(x, pairs) for x in STR.findall(src)]
        after = STR.findall(out)
        if before != after:
            bad = [(x, y) for x, y in zip(before, after) if x != y][:3]
            print(f"REFUSED {f}: string literals changed, e.g. {bad}", file=sys.stderr)
            return 1
        if out != src:
            p.write_text(out)
            changed += 1
    print(f"rewrote {changed} file(s); every string literal byte-identical")
    if wire:
        print("\nWIRE CONTRACT -- these renamed names are serde fields:")
        for f, hits in wire.items():
            print(f"  {f}: {', '.join(hits)}")
        print("Each needs #[serde(rename)] or a matching client change.")
    return 0


sys.exit(main())
