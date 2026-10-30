#!/usr/bin/env python3
"""How much of the workspace still carries Norwegian identifiers.

CLAUDE.md records this number after every rename batch, so it has to be
REPRODUCIBLE. The first instrument was not: it counted occurrences of a
hand-kept list of Norwegian words, which meant it only ever measured the
words already identified -- so it fell towards zero as that list was
worked through, whatever was left outside it. It read 1.1% remaining
while 884 distinct Norwegian segments were still in the tree.

This counts the other way round: every identifier segment that is NOT
English. Identifiers are split on `_` and on CamelCase humps, string
literals and comments are skipped (they are allowed to be Norwegian --
docs, user-facing text, model prompts), and a segment counts as English
if it is in the system dictionary under any common inflection, or in the
STOP list of Rust/abbreviation/vendor tokens the dictionary lacks.

It OVERCOUNTS by design: a few percent of the total is residual
abbreviations (`rfind`, `hexdigit`, `x86`) rather than Norwegian. That
is the right direction for a progress gate -- the number can only be
honest about being done when it is near zero.

    scripts/norwegian-left.py            # summary + top 40
    scripts/norwegian-left.py --all      # every segment, for building a map
"""

import collections
import pathlib
import re
import sys

STR = re.compile(
    r'(?<![A-Za-z0-9_])r(#*)"(?:(?!"\1).)*"\1'
    r"|b?'(?:[^'\\]|\\.)'"
    r'|"(?:[^"\\]|\\.)*"'
    r"|//[^\n]*"
    r"|/\*(?:.|\n)*?\*/",
    re.S,
)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
SEG = re.compile(r"[A-Z]+(?![a-z])|[A-Z][a-z0-9]*|[a-z0-9]+")

# Tokens the system dictionary does not hold: Rust keywords and idioms,
# crate names, file formats, standards, and our own English coinages.
STOP = set(
    """
fn pub mod impl struct enum trait dyn async await ref mut const static unsafe extern
i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 usize isize f32 f64 str vec ok err cfg attr
serde clap axum sqlx tokio anyhow chrono reqwest flate2 quick xml oneshot mutex
println eprintln dbg todo unimplemented lossy trunc frac succ rfind hexdigit
deref deserialize serialize datelike timestamp lowercase uppercase formatting skipped
counterparty subledger
fmt ymd utc hms millis micros nanos v1 v2 v3 v4 v5 v6 v7
buf arg args kwargs seq idx cnt len ptr arc rc cow env var ext dir fs io os tcp udp
eq ne lt gt le ge num inv acct msg info diff config params payload resp req ctx
bytes byte debug email filename paid inbox verified mapping decrypt encrypt
regnmed regnid idp tx app pg auth std sysadmin systemadmin bokforer
ore bp sql json csv pdf url uri api db cli http https utf8 uuid jwt jwk jwks oidc
sha1 sha256 hash32 hmac rsa pem der pkcs asn1 base64 rfc3339 iso rfc csp xsd clippy
mva saft ehf ocr kid aga ns gb kb mb pr ci ui ux svg png css js ts jpg jpeg webp ico
tsa smp as4 ubl bis peppol brreg nats jetstream stripe postgres psql camt camt053
pain001 mcp llm rpc stdio pwa spa sse cors pkce sso iban bic bban scor mod10 mod11
btreemap hashmap vecdeque jsonrpc nin orgnumber subledger voucher timesheet
withholding invoiceable milli payslip ytd nok kr km gov spes ops opf webhook avatar
mailq agentllm ntry x86 x2 ee
""".split()
)

DICT = pathlib.Path("/usr/share/dict/words")
SUFFIXES = [
    ("s", ""),
    ("es", ""),
    ("ies", "y"),
    ("ed", ""),
    ("ed", "e"),
    ("ing", ""),
    ("ing", "e"),
    ("er", ""),
    ("er", "e"),
    ("est", ""),
    ("ly", ""),
    ("able", ""),
    ("ness", ""),
]


def english(words, s):
    if s in words or s in STOP:
        return True
    return any(
        s.endswith(suf) and s[: len(s) - len(suf)] + base in words
        for suf, base in SUFFIXES
    )


def main():
    if not DICT.exists():
        sys.exit(f"no word list at {DICT}")
    words = {w.strip().lower() for w in DICT.read_text().splitlines()}
    segs = collections.Counter()
    where = collections.defaultdict(collections.Counter)
    total = 0
    for path in sorted(pathlib.Path("crates").rglob("*.rs")):
        code = STR.sub(" ", path.read_text())
        crate = path.parts[1]
        for ident in IDENT.finditer(code):
            for part in ident.group(0).split("_"):
                for raw in SEG.finditer(part):
                    s = raw.group(0).lower()
                    if len(s) < 2 or s.isdigit():
                        continue
                    total += 1
                    if english(words, s):
                        continue
                    segs[s] += 1
                    where[s][crate] += 1

    left = sum(segs.values())
    print(f"{left} of {total} identifier segments ({left / total:.1%}), {len(segs)} distinct")
    per = collections.Counter()
    for counts in where.values():
        per.update(counts)
    print("per crate:", "  ".join(f"{k} {v}" for k, v in per.most_common()))
    rows = segs.most_common() if "--all" in sys.argv else segs.most_common(40)
    for s, c in rows:
        print(f"  {s:24} {c:5}  {dict(where[s])}")


if __name__ == "__main__":
    main()
