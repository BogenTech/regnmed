#!/usr/bin/env python3
"""Pin every serde field a rename is about to move, BEFORE renaming it.

`rename-identifiers.py` reports which renamed names are serde fields,
but adding the `#[serde(rename)]` by hand is how four batches in a row
put an attribute on a `name: value` line inside a function body
("cannot find attribute `serde` in this scope"). The fix is to do it
the way the guard reads the file: walk each struct that derives
Serialize/Deserialize, and only touch field lines inside that body.

Run it BEFORE the rename, with the same map. It pins each affected
field to the name it has NOW -- the effective wire name, so a field
that already carries a `rename` is left alone -- and the rename then
moves the Rust name out from under a pin that is correct by
construction. `wire-names.py` still has the last word.

Usage:
    scripts/pin-wire.py map.tsv $(find crates -name '*.rs')
"""
import re, sys, pathlib

DERIVE = re.compile(r'#\[derive\([^)]*\b(?:Serialize|Deserialize)\b[^)]*\)\]')
FIELD = re.compile(r'^(\s*)(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*\S')
SERDE = re.compile(r'^\s*#\[serde\((.*)\)\]\s*$')
HAS_RENAME = re.compile(r'\brename\s*=')


def struct_bodies(src):
    """(start, end) line indices of each Serialize/Deserialize body."""
    out = []
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
        out.append((src.count("\n", 0, brace), src.count("\n", 0, i)))
    return out


def pin(src, names):
    lines = src.split("\n")
    inside = set()
    for a, b in struct_bodies(src):
        inside.update(range(a, b + 1))
    out, done = [], []
    for n, line in enumerate(lines):
        m = FIELD.match(line)
        if n in inside and m and m.group(2) in names:
            field = m.group(2)
            # An existing #[serde(...)] on the line above owns this field:
            # merge into it, and leave it alone if it already renames.
            prev = SERDE.match(out[-1]) if out else None
            if prev:
                if not HAS_RENAME.search(prev.group(1)):
                    indent = out[-1][: len(out[-1]) - len(out[-1].lstrip())]
                    out[-1] = f'{indent}#[serde({prev.group(1)}, rename = "{field}")]'
                    done.append(field)
            else:
                out.append(f'{m.group(1)}#[serde(rename = "{field}")]')
                done.append(field)
        out.append(line)
    return "\n".join(out), done


def main():
    names = set()
    for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            names.add(line.split("\t")[0].strip())
    total = 0
    for f in sys.argv[2:]:
        p = pathlib.Path(f)
        src = p.read_text()
        out, done = pin(src, names)
        if out != src:
            p.write_text(out)
            total += len(done)
            print(f"  {f}: {', '.join(done)}")
    print(f"pinned {total} serde field(s)")
    return 0


sys.exit(main())
