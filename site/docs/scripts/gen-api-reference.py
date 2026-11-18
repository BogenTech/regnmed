#!/usr/bin/env python3
"""Generates src/content/docs/developers/api/*.md from the route table in
crates/regnmed-api/src/lib.rs. The required right per endpoint is read from the
`Right::X` the handler passes to `require(...)`; handlers that check in a helper
show «—» (see the page footnote). Run from anywhere: python3 scripts/gen-api-reference.py"""
import glob, json, os, re

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
SRC = os.path.join(ROOT, "crates/regnmed-api/src")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src/content/docs/developers/api")

ROUTE_RE = re.compile(r'\.route\(\s*"([^"]+)"\s*,\s*((?:axum::routing::)?(?:get|post|put|delete)\([^)]*\)(?:\s*\.\s*(?:get|post|put|delete)\([^)]*\))*)\s*,?\s*\)', re.S)
routes = []
for fname, default_mod in (("lib.rs", "lib"), ("platform.rs", "platform")):
    text = open(f"{SRC}/{fname}").read()
    for m in ROUTE_RE.finditer(text):
        for mm in re.finditer(r'(get|post|put|delete)\(([\w:]+)\)', m.group(2)):
            h = mm.group(2)
            routes.append((m.group(1), mm.group(1).upper(), h if "::" in h else f"{default_mod}::{h}"))

files = {os.path.basename(f)[:-3]: open(f).read() for f in glob.glob(f"{SRC}/*.rs")}
acc = open(f"{SRC}/access.rs").read()
slug = dict(re.findall(r'(\w+) => "([A-Z_]+)",', acc[acc.index("pub fn slug(self) -> &'static str"):]))

def right_of(handler):
    mod, _, fn = handler.rpartition("::")
    body = files.get(mod or "lib", "")
    i = re.search(r"fn " + fn + r"\b", body)
    if not i:
        return ""
    seg = body[i.start(): i.start() + 3000]
    nxt = re.search(r"\n(pub(\([a-z]+\))? )?(async )?fn ", seg[10:])
    if nxt:
        seg = seg[: nxt.start() + 10]
    return ", ".join(dict.fromkeys(slug.get(x, x) for x in re.findall(r"Right::(\w+)", seg)))

PUBLIC = {"/pricing", "/terms", "/terms/versions", "/vilkar", "/vilkar/{version}", "/anchors", "/health"}
SKIP = ("/", "/callback", "/assets/{*path}", "/manifest.webmanifest", "/sw.js", "/icon-192.png", "/icon-512.png", "/ny", "/portal-config")

def group(path):
    p = path
    if p.startswith("/platform"): return "platform"
    c = re.match(r"/companies/\{company_id\}/([^/]+)", p)
    s = c.group(1) if c else None
    if s == "subscription": return "identity"
    if s in ("members","access","invitations","roles","platform-access","engagements","engagement-requests","attestering","terms","authority-calls"):
        return "identity"
    if s in ("integrations","agents","agent"): return "agents"
    if p.startswith(("/chat",)): return "agents"
    if p.startswith(("/me","/pricing","/terms","/vilkar","/health","/firms","/directory","/registry")): return "identity"
    if s in ("invoices","invoice-templates","quotes","orders","products","inventory","parties","reskontro"): return "sales"
    if s == "reports": return "shares" if "aksjonaer" in p else "reports"
    if s in ("budgets","mva","period-lock"): return "reports" if s != "period-lock" else "ledger"
    if s in ("bank","ocr","payments","cards","currency"): return "bank"
    if s in ("employees","payroll","tax-cards","timesheet","expenses"): return "payroll"
    if s in ("inbox","import","jobs","opening-balance"): return "inbox"
    if s in ("shareholders","share-events","dividends","assets"): return "shares"
    if p == "/anchors" or s == "anchors": return "verification"
    return "ledger"  # companies, accounts, vouchers, attachments, dimensions, settings

# Prose per area and locale, side by side so a change is made once and cannot drift.
AREAS = {
    "identity": {
        "order": 1, "docs": ['auth', 'integrations', 'marketplace', 'abonnement', 'vilkar-teknisk'],
        "nb": ('Identitet og tilgang', 'Hvem er jeg, hvem har tilgang til hva: `/me`, medlemmer, roller, invitasjoner, byrå, oppdrag, abonnement og de åpne vilkårssidene.'),
        "en": ('Identity and access', 'Who am I and who may do what: `/me`, members, roles, invitations, firm, engagement, subscription and the public terms pages.'),
    },
    "ledger": {
        "order": 2, "docs": ['hovedbok', 'ledger', 'perioder', 'dimensjoner', 'periodisering', 'arsavslutning', 'balansedokumentasjon', 'kontantsalg'],
        "nb": ('Selskap og hovedbok', 'Onboarding av selskap, kontoplan, bilag, vedlegg, periodelås, dimensjoner, periodisering, årsavslutning, balansedokumentasjon og kassaoppgjør.'),
        "en": ('Company and general ledger', 'Company onboarding, chart of accounts, vouchers, attachments, period lock, dimensions, accruals, year-end close, balance documentation and cash-register settlement.'),
    },
    "sales": {
        "order": 3, "docs": ['faktura', 'purring', 'produkter', 'reskontro', 'ehf'],
        "nb": ('Salg og reskontro', 'Faktura, tilbud og ordre, produkter og lager, kunder og leverandører, åpne poster og avstemming.'),
        "en": ('Sales and subledger', 'Invoices, quotes and orders, products and inventory, customers and suppliers, open items and matching.'),
    },
    "reports": {
        "order": 4, "docs": ['rapporter', 'mva', 'saft/README', 'budsjett', 'revisjon'],
        "nb": ('Regnskap og rapporter', 'Lovpålagte spesifikasjoner, resultat og balanse, mva og mva-melding, SAF-T, nøkkeltall, budsjett og revisjonsrapporten.'),
        "en": ('Accounting and reports', 'Statutory specifications, income statement and balance sheet, VAT and the VAT return, SAF-T, key figures, budget and the audit report.'),
    },
    "bank": {
        "order": 5, "docs": ['bank', 'betaling', 'valuta'],
        "nb": ('Bank og betaling', 'Bankavstemming, OCR-giro, betalingslister, kort og valuta.'),
        "en": ('Bank and payments', 'Bank reconciliation, OCR giro, payment lists, cards and currency.'),
    },
    "payroll": {
        "order": 6, "docs": ['lonn', 'timer', 'utlegg'],
        "nb": ('Lønn, timer og utlegg', 'Ansatte, lønnskjøring og a-melding, skattekort, timeføring og utlegg.'),
        "en": ('Payroll, time and expenses', 'Employees, payroll runs and the a-melding, tax cards, timesheets and expenses.'),
    },
    "inbox": {
        "order": 7, "docs": ['bilagsinnboks', 'bilagstolkning', 'epost-inn', 'migration', 'jobs'],
        "nb": ('Innboks, import og jobber', 'Bilagsinnboksen, forslag, e-post-inn, SAF-T- og CSV-import og bakgrunnsjobber.'),
        "en": ('Inbox, import and jobs', 'The voucher inbox, suggestions, inbound e-mail, SAF-T and CSV import and background jobs.'),
        "extra": {
            "nb": """## Opplasting

`POST /companies/{id}/inbox?filename=<navn>&sha256=<hex>` med filen som rå kropp og riktig `Content-Type`. `sha256` er valgfri, men gjør et nytt forsøk trygt. Svar: `{ "document_id": "…", "existing": false }`. Se [Konvensjoner](/developers/conventions/#idempotens).
""",
            "en": """## Upload

`POST /companies/{id}/inbox?filename=<name>&sha256=<hex>` with the file as the raw body and the right `Content-Type`. `sha256` is optional but makes a retry safe. Response: `{ "document_id": "…", "existing": false }`. See [Conventions](/en/developers/conventions/#idempotency).
""",
        },
    },
    "shares": {
        "order": 8, "docs": ['aksjonaer', 'anlegg'],
        "nb": ('Aksjeeierbok og anlegg', 'Aksjonærer, aksjehendelser, utbytte, RF-1086 og anleggsregisteret.'),
        "en": ('Share register and assets', 'Shareholders, share events, dividends, RF-1086 and the fixed-asset register.'),
    },
    "agents": {
        "order": 9, "docs": ['integrations', 'agent', 'meldinger'],
        "nb": ('Integrasjoner, agenter og meldinger', 'Maskintilgang, agenter og oppgaver, samtaler.'),
        "en": ('Integrations, agents and messages', 'Machine access, agents and tasks, conversations.'),
        "extra": {
            "nb": """## Former

Gi en integrasjon tilgang (`INTEGRASJON_ADMIN`):

```json
POST /companies/{id}/integrations
{ "client_id": "nettbutikk", "navn": "Nettbutikken", "kontakt": "it@butikk.example", "access": "bokforing" }
```

Bytt rolle: `PUT …/integrations/{iid}/rolle` med `{ "access": "les" }`. Trekk tilbake: `POST …/integrations/{iid}/revoke`. Endrende kall integrasjonen har gjort: `GET …/integrations/log`.

Oppgaver til en agent:

```json
POST /companies/{id}/agent/oppgaver
{ "melding": "Bokfør kvitteringene i innboksen", "integration_id": "…", "frist": "2026-12-01" }

POST /companies/{id}/agent/oppgaver/{oppgave_id}/svar     (bare agenten oppgaven er stilt til)
{ "status": "pagar" }   // pagar | sporsmal | utfort | avvist, med "notat" ved sporsmal og avvist
```

Se [MCP og agenter](/developers/mcp-and-agents/).
""",
            "en": """## Shapes

Grant an integration access (`INTEGRASJON_ADMIN`):

```json
POST /companies/{id}/integrations
{ "client_id": "webshop", "navn": "The webshop", "kontakt": "it@shop.example", "access": "bokforing" }
```

Change the role: `PUT …/integrations/{iid}/rolle` with `{ "access": "les" }`. Revoke: `POST …/integrations/{iid}/revoke`. Mutating calls the integration has made: `GET …/integrations/log`.

Tasks for an agent:

```json
POST /companies/{id}/agent/oppgaver
{ "melding": "Post the receipts in the inbox", "integration_id": "…", "frist": "2026-12-01" }

POST /companies/{id}/agent/oppgaver/{oppgave_id}/svar     (only the agent the task is assigned to)
{ "status": "pagar" }   // pagar | sporsmal | utfort | avvist, with "notat" for sporsmal and avvist
```

See [MCP and agents](/en/developers/mcp-and-agents/).
""",
        },
    },
    "verification": {
        "order": 10, "docs": ['anchoring', 'ledger'],
        "nb": ('Forankring og verifisering', 'Den åpne forankringsfeeden og verifisering per selskap.'),
        "en": ('Anchoring and verification', 'The public anchoring feed and per-company verification.'),
        "extra": {
            "nb": """## Former

`GET /anchors` er åpen (ingen token) og viser de 20 siste snapshotene:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` gir per snapshot selskapets forankrede kjedehode og Merkle-beviset (`proof`: liste av `{side, sibling}`). `GET /companies/{id}/anchors/verify` kjører hele kontrollen og svarer `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. Se [Verifiser selv](/developers/verify-it-yourself/).
""",
            "en": """## Shapes

`GET /anchors` is public (no token) and lists the 20 latest snapshots:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` returns, per snapshot, the company's anchored chain head and the Merkle proof (`proof`: a list of `{side, sibling}`). `GET /companies/{id}/anchors/verify` runs the full check and answers `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. See [Verify it yourself](/en/developers/verify-it-yourself/).
""",
        },
    },
    "platform": {
        "order": 11, "docs": ['auth'],
        "nb": ('Plattform (leverandørens konsoll)', 'Krever aktiv plattformrolle (`support` leser og tildeler nye medlemskap, `systemadmin` endrer); hvert kall logges, og ingen av endepunktene når en hovedbok. Ikke en del av integrasjons-API-et.'),
        "en": ('Platform (the vendor console)', 'Requires an active platform role (`support` reads and grants new memberships, `systemadmin` changes). Every call is logged and none of the endpoints reach a ledger. Not part of the integration API.'),
    },
}

by = {k: [] for k in AREAS}
seen = set()
for path, meth, h in routes:
    if path in SKIP or path.startswith(("/auth/", "/webhooks/")) or (path, meth) in seen: continue
    seen.add((path, meth))
    by[group(path)].append((path, meth, h))

T = {  # the strings that differ per locale
    "nb": dict(base="/developers", out="developers/api", more="## Les mer\n", hdr="| Metode | Sti | Rett |", public="åpen", platform="plattformrolle",
               intro="Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller]({base}/access-and-roles/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.\n"),
    "en": dict(base="/en/developers", out="en/developers/api", more="## Further reading (Norwegian)\n", hdr="| Method | Path | Right |", public="public", platform="platform role",
               intro="The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles]({base}/access-and-roles/)); \"public\" means no token, \"—\" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.\n"),
}

def emit(loc):
    t = T[loc]
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src/content/docs", t["out"])
    os.makedirs(out, exist_ok=True)
    for key, a in AREAS.items():
        title, desc = a[loc]
        rows = sorted(by[key], key=lambda r: (r[0], r[1]))
        lines = [f"---\ntitle: {json.dumps(title, ensure_ascii=False)}\ndescription: {json.dumps(desc, ensure_ascii=False)}\neditUrl: false\nsidebar:\n  order: {a['order']}\n---\n", f"{desc}\n", t["intro"].format(base=t["base"]), t["hdr"] + "\n| --- | --- | --- |"]
        for path, meth, h in rows:
            r = t["platform"] if key == "platform" else (t["public"] if path in PUBLIC else (right_of(h) or "—"))
            if r not in (t["public"], "—", t["platform"]):
                r = ", ".join(f"`{x.strip()}`" if x.strip() else x for x in r.split(", "))
            lines.append(f"| {meth} | `{path}` | {r} |")
        if "extra" in a:
            lines.append("\n" + a["extra"][loc])
        lines.append("\n" + t["more"])
        lines += [f"- [{d.split('/')[0]}](/teknisk/{d.replace('/README','').split('/')[0]}/)" for d in a["docs"] if not d.startswith("saft/")]
        if key == "reports":
            lines.append("- [SAF-T](https://github.com/BogenTech/regnmed/blob/main/docs/saft/README.md)")
        open(f"{out}/{key}.md", "w").write("\n".join(lines) + "\n")

emit("nb")
emit("en")
print({k: len(v) for k, v in by.items()})
