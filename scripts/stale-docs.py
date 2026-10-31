#!/usr/bin/env python3
"""Backticked code names in docs/ that no longer exist in crates/.

docs/ is audit-facing, and nothing checks it. The rename batches found
two wrong names (`settings::krev_mva_registrert`, `hent_llm_nokkel`)
only because a rename happened to touch the same word. This looks on
purpose: every `code span` that LOOKS like a Rust path or identifier,
checked against the whole crate tree as plain text.

It overcounts, and the overcount is why this is NOT a CI gate: a span
may name a SQL column, a k8s field, an XML element, a portal symbol
(ui/ is not scanned), a satsregister key, an env var, or a name the
prose is deliberately quoting as HISTORY ("it was called X until
2026-10-29"). 60 of the 83 it first found were one of those. Gating
would mean an allowlist that churns on every doc edit. So it reports,
and a human reads it -- run it with every rename batch.

KNOWN BLIND SPOT: a span's parts are matched as plain text ANYWHERE
in the tree, so a reference to the WRONG function still reads as
valid as long as the name exists somewhere. docs/auth.md said the
manual-coverage rule came from `avslutt` while the only `avslutt` in
the tree was the FISCAL YEAR close; the real function is
`platform::end_coverage`. That surfaced only when a rename removed
`avslutt` from the tree entirely. Narrowing this would mean resolving
paths properly, which is a different tool.
"""
import pathlib, re, sys

# Literals are NOT blanked, and that is deliberate even though it hides
# real staleness: docs/ legitimately cites SQL columns, JSON keys, stored
# status values and right slugs, every one of which lives only in a
# literal. Blanking them was tried -- it took the report from 62 spans to
# 471, which no human reads. The cost is the blind spot this docstring
# describes: `lonnsslipp` survives in a download filename and `avslutt`
# survived in prose, so each looked live.
code = "\n".join(
    p.read_text() for p in pathlib.Path("crates").rglob("*.rs")
)
SPAN = re.compile(r"`([^`\n]+)`")
# Rust-ish: a path with ::, or snake_case with _, or CamelCase.
RUSTY = re.compile(r"^[A-Za-z_][A-Za-z0-9_-]*(::[A-Za-z_][A-Za-z0-9_]*)+(\(\))?$|^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*(\(\))?$")

hits = {}
for doc in sorted(pathlib.Path("docs").rglob("*.md")):
    for n, line in enumerate(doc.read_text().splitlines(), 1):
        for span in SPAN.findall(line):
            s = span.strip().removesuffix("()")
            if not RUSTY.match(s):
                continue
            if "::" not in s and "_" not in s and not re.search(r"[a-z][A-Z]", s):
                continue  # a bare word: too noisy to judge
            parts = [p for p in s.split("::") if p and "-" not in p]
            # The last segment is the name; earlier ones are modules.
            if all(re.search(r"\b%s\b" % re.escape(p), code) for p in parts):
                continue
            hits.setdefault(str(doc), []).append((n, span))

for doc, items in hits.items():
    for n, span in items:
        print(f"{doc}:{n}: {span}")
print(f"\n{sum(len(v) for v in hits.values())} span(s) not found in crates/", file=sys.stderr)
