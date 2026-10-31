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

3. A MIS-TOKENIZED LITERAL SKIPS CODE SILENTLY, and the guard cannot
   see it, because the before/after comparison uses the same tokenizer.
   Ordinary Rust strings MAY span newlines -- every multi-line SQL query
   is one -- and excluding `\n` from the string pattern made the scanner
   fall through into the query text, where `returning last_number",`
   offers an `r"` that reads as a raw string and swallows the next
   twenty lines as a literal. Those lines were never renamed and nothing
   reported it. Char literals are matched too, so `'"'` cannot open a
   bogus string.

4. COMMENTS ARE PROSE, NOT CODE. `to -> two` turned "fails to open"
   into "fails two open" across a module's doc comments. Comments are
   skipped like literals; lines mentioning an old name are reported so
   the genuine identifier references get fixed by hand.

Three things it does NOT know, which are yours to check:

  - Cross-crate names. A map applied to a dependent will happily rename
    a function that belongs to a crate you have not converted yet. Build
    after every run. And NEVER fix up call sites with a bare
    `\bname\b` sweep over crates/: `rate_on -> value_on` for the
    satsregister also renamed `mva::rate_on`, which is a different
    function (the VAT rate) that happened to share the name. Rewrite
    QUALIFIED paths, or name the files.
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
STR = re.compile(
    # A raw-string prefix is only a prefix at a token boundary: the `r`
    # at the end of `last_number"` is not one, and reading it as `r"..."`
    # is how this silently skipped a whole function (see below).
    r'(?<![A-Za-z0-9_])r(#*)"(?:(?!"\1).)*"\1'   # raw string
    r"|b?'(?:[^'\\]|\\.)'"                      # char / byte literal
    r'|"(?:[^"\\]|\\.)*"'                       # ordinary string -- MAY span lines
    r'|//[^\n]*'                                 # line comment, incl. /// and //!
    r'|/\*(?:.|\n)*?\*/',                        # block comment
    re.S,
)
COMMENT = re.compile(r'^(?://|/\*)')
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
    must be renamed with the code.

    AND THE ONE THING THAT GOES WRONG WHEN A `{...}` IS NOT A FORMAT
    CAPTURE. `regnmed-mcp`'s tool table holds paths like
    `"/companies/{selskap}/invoices"`, where the placeholder is
    substituted at RUNTIME against the tool schema's own property name
    -- a Norwegian WIRE name that must not move. Renaming `selskap`
    rewrote 32 of them to `{company}` while the schemas kept saying
    `"selskap"`, and every affected tool would have failed for an
    agent. Nothing in this script can tell the two apart, so the guard
    is a test in that crate:
    `tool::tests::every_path_placeholder_is_a_declared_property`, which
    is what caught it. If a crate builds strings with its own
    placeholder syntax, it needs a test of that shape before you run a
    rename through it."""
    lookup = dict(pairs)
    return CAPTURE.sub(lambda m: "{%s%s" % (lookup.get(m.group(1), m.group(1)), m.group(2)), seg)


def rename(src, pairs):
    parts, last = [], 0
    for m in STR.finditer(src):
        seg = src[last : m.start()]
        for a, b in pairs:
            seg = re.sub(r"\b%s\b" % re.escape(a), b, seg)
        parts.append(seg)
        tok = m.group(0)
        # Comments are skipped for the same reason literals are, and for
        # a worse one: a map entry like `to -> two` rewrites ordinary
        # English prose ("fails two open"). Mentions of a renamed
        # identifier in a comment are reported and fixed by hand.
        parts.append(tok if COMMENT.match(tok) else captures(tok, pairs))
        last = m.end()
    seg = src[last:]
    for a, b in pairs:
        seg = re.sub(r"\b%s\b" % re.escape(a), b, seg)
    parts.append(seg)
    return "".join(parts)


def blank_literals(src):
    """`src` with every string literal and comment replaced by spaces, so
    line numbers hold but only CODE is searchable."""
    out, last = [], 0
    for m in STR.finditer(src):
        out.append(src[last : m.start()])
        out.append(re.sub(r"[^\n]", " ", m.group(0)))
        last = m.end()
    out.append(src[last:])
    return "".join(out)


def shadows(src, pairs):
    """Pairs whose TARGET is already a live name in this file's code.

    THE ONE COLLISION CLASS THE COMPILER DOES NOT CATCH. Every other
    collision this conversion hit was a build error -- a renamed
    function no longer matching its call site, two enums of one name.
    But if `b` is already bound in the same scope, `a -> b` SHADOWS it:
    the code still compiles, the wrong binding is read, and nothing
    says a word. This check was written for `nr -> number`, on a grep
    that said nine files bind both -- and its FIRST RUN disproved
    that: the grep had read comments, and code-only it is five files
    of disjoint `for` loops.

    THEN IT FIRED FOR REAL, and this check had named the file before
    the run: `tests/grupper/migrering.rs` held a CSV body in `apne`
    and the open items read back in `open`, and `apne -> open` made
    the third call site pass the Vec where the &str belonged. The
    compiler caught that one only because the types differ -- two
    &str would have compiled and posted the wrong body. So the class
    is not theoretical and the reports are not noise.

    Reported, never refused: most are benign -- struct fields of the
    same name in different structs, a target in another function, a
    module path (`crate::email::`) -- and only a human can tell
    which. Read every line of the report.
    """
    code = blank_literals(src)
    found = []
    for a, b in pairs:
        if not re.search(r"\b%s\b" % re.escape(a), code):
            continue
        at = [
            code[: m.start()].count("\n") + 1
            for m in re.finditer(r"\b%s\b" % re.escape(b), code)
        ]
        if at:
            found.append((a, b, at[:4]))
    return found


def main():
    pairs = []
    for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            a, b = line.split("\t")
            pairs.append((a.strip(), b.strip()))
    wire, stale, shade = {}, {}, {}
    changed = 0
    for f in sys.argv[2:]:
        p = pathlib.Path(f)
        src = p.read_text()
        fields = serde_fields(src)
        hits = sorted(fields & {a for a, _ in pairs})
        if hits:
            wire[f] = hits
        risky = shadows(src, pairs)
        if risky:
            shade[f] = risky
        out = rename(src, pairs)
        words = re.compile(r"\b(?:%s)\b" % "|".join(re.escape(a) for a, _ in pairs))
        mentions = [
            (n, line)
            for n, line in enumerate(src.splitlines(), 1)
            if COMMENT.match(line.strip()) and words.search(line)
        ]
        if mentions:
            stale[f] = mentions
        toks = [m.group(0) for m in STR.finditer(src)]
        before = [t if COMMENT.match(t) else captures(t, pairs) for t in toks]
        after = [m.group(0) for m in STR.finditer(out)]
        if before != after:
            bad = [(x, y) for x, y in zip(before, after) if x != y][:3]
            print(f"REFUSED {f}: string literals changed, e.g. {bad}", file=sys.stderr)
            return 1
        if out != src:
            p.write_text(out)
            changed += 1
    print(f"rewrote {changed} file(s); strings and comments byte-identical")
    if stale:
        print("\nCOMMENTS -- these still use an old name, fix the real ones by hand:")
        for f, lines in stale.items():
            for n, text in lines:
                print(f"  {f}:{n}: {text.strip()[:96]}")
    if shade:
        print("\nSHADOWS -- the new name is ALREADY a name in this file's code.")
        print("The compiler cannot see this one. Check the scopes by hand:")
        for f, items in shade.items():
            for a, b, at in items:
                where = ", ".join(str(n) for n in at)
                print(f"  {f}: {a} -> {b}, which already appears at line {where}")

    if wire:
        print("\nWIRE CONTRACT -- these renamed names are serde fields:")
        for f, hits in wire.items():
            print(f"  {f}: {', '.join(hits)}")
        print("Each needs #[serde(rename)] or a matching client change.")
    return 0


sys.exit(main())
