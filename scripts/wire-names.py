#!/usr/bin/env python3
"""Effective serde wire names, so an identifier rename can be proven safe.

`scripts/rename-identifiers.py` REPORTS the serde fields it renamed and
leaves the `#[serde(rename)]` attributes to a human. That report tells
you where to look; it does not tell you whether you got it right, and
"did the JSON move?" is the one question a wire-breaking refactor must
answer with evidence rather than care.

A field's wire name is the field name unless a `#[serde(rename = "...")]`
overrides it. This prints every such struct's effective names, so:

    git stash && scripts/wire-names.py $(find crates -name '*.rs') > before
    git stash pop && scripts/wire-names.py $(find crates -name '*.rs') > after
    diff before after      # empty == the wire did not move

Not a parser: it reads `#[derive(...Serialize/Deserialize...)]` structs by
brace matching, and a `rename` on the line above a field belongs to it.
That covers how this repo writes them. `rename_all`, flattening and
enum representations are NOT handled -- add them when something uses one.
"""
import re, sys, pathlib

DERIVE = re.compile(r'#\[derive\([^)]*\b(Serialize|Deserialize)\b[^)]*\)\]')
RENAME = re.compile(r'serde\([^)]*\brename\s*=\s*"([^"]+)"')
FIELD = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:')

out = {}
for f in sys.argv[1:]:
    src = pathlib.Path(f).read_text()
    for m in DERIVE.finditer(src):
        brace = src.find("{", m.end())
        if brace < 0:
            continue
        name = re.search(r'struct\s+(\w+)', src[m.end() : brace])
        name = name.group(1) if name else "?"
        depth, i = 0, brace
        while i < len(src):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        pending = None
        for line in src[brace : i + 1].splitlines():
            r = RENAME.search(line)
            if r:
                pending = r.group(1)
                continue
            fm = FIELD.match(line)
            if fm:
                key = f"{pathlib.Path(f).name}:{name}"
                out.setdefault(key, []).append(pending or fm.group(1))
                pending = None

for k in sorted(out):
    print(k, " ".join(sorted(out[k])))
