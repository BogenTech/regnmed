#!/usr/bin/env python3
"""Generates src/content/docs/utviklere/api/*.md from the route table in
crates/regnmed-api/src/lib.rs. The required right per endpoint is read from the
`Right::X` the handler passes to `require(...)`; handlers that check in a helper
show «—» (see the page footnote). Run from anywhere: python3 scripts/gen-api-reference.py"""
import glob, json, os, re

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
SRC = os.path.join(ROOT, "crates/regnmed-api/src")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src/content/docs/utviklere/api")

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

PUBLIC = {"/terms", "/terms/versions", "/vilkar", "/vilkar/{version}", "/anchors", "/health"}
SKIP = ("/", "/callback", "/assets/{*path}", "/manifest.webmanifest", "/sw.js", "/icon-192.png", "/icon-512.png", "/ny", "/portal-config")

def group(path):
    p = path
    if p.startswith("/platform"): return "plattform"
    c = re.match(r"/companies/\{company_id\}/([^/]+)", p)
    s = c.group(1) if c else None
    if s == "subscription": return "identitet"
    if s in ("members","access","invitations","roles","platform-access","engagements","engagement-requests","attestering","terms","authority-calls"):
        return "identitet"
    if s in ("integrations","agents","agent"): return "agenter"
    if p.startswith(("/chat",)): return "agenter"
    if p.startswith(("/me","/terms","/vilkar","/health","/firms","/directory","/registry")): return "identitet"
    if s in ("invoices","invoice-templates","quotes","orders","products","inventory","parties","reskontro"): return "salg"
    if s == "reports": return "aksjer" if "aksjonaer" in p else "rapporter"
    if s in ("budgets","mva","period-lock"): return "rapporter" if s != "period-lock" else "hovedbok"
    if s in ("bank","ocr","payments","cards","currency"): return "bank"
    if s in ("employees","payroll","tax-cards","timesheet","expenses"): return "personal"
    if s in ("inbox","import","jobs","opening-balance"): return "innboks"
    if s in ("shareholders","share-events","dividends","assets"): return "aksjer"
    if p == "/anchors" or s == "anchors": return "verifisering"
    return "hovedbok"  # companies, accounts, vouchers, attachments, dimensions, settings

PAGES = {
    "identitet": ("Identitet og tilgang", 1, "Hvem er jeg, hvem har tilgang til hva: `/me`, medlemmer, roller, invitasjoner, byrå, oppdrag, abonnement og de åpne vilkårssidene.", ["auth", "integrations", "marketplace", "abonnement", "vilkar-teknisk"]),
    "hovedbok": ("Selskap og hovedbok", 2, "Onboarding av selskap, kontoplan, bilag, vedlegg, periodelås, dimensjoner, periodisering, årsavslutning, balansedokumentasjon og kassaoppgjør.", ["hovedbok", "ledger", "perioder", "dimensjoner", "periodisering", "arsavslutning", "balansedokumentasjon", "kontantsalg"]),
    "salg": ("Salg og reskontro", 3, "Faktura, tilbud og ordre, produkter og lager, kunder og leverandører, åpne poster og avstemming.", ["faktura", "purring", "produkter", "reskontro", "ehf"]),
    "rapporter": ("Regnskap og rapporter", 4, "Lovpålagte spesifikasjoner, resultat og balanse, mva og mva-melding, SAF-T, nøkkeltall, budsjett og revisjonsrapporten.", ["rapporter", "mva", "saft/README", "budsjett", "revisjon"]),
    "bank": ("Bank og betaling", 5, "Bankavstemming, OCR-giro, betalingslister, kort og valuta.", ["bank", "betaling", "valuta"]),
    "personal": ("Lønn, timer og utlegg", 6, "Ansatte, lønnskjøring og a-melding, skattekort, timeføring og utlegg.", ["lonn", "timer", "utlegg"]),
    "innboks": ("Innboks, import og jobber", 7, "Bilagsinnboksen, forslag, e-post-inn, SAF-T- og CSV-import og bakgrunnsjobber.", ["bilagsinnboks", "bilagstolkning", "epost-inn", "migration", "jobs"]),
    "aksjer": ("Aksjeeierbok og anlegg", 8, "Aksjonærer, aksjehendelser, utbytte, RF-1086 og anleggsregisteret.", ["aksjonaer", "anlegg"]),
    "agenter": ("Integrasjoner, agenter og meldinger", 9, "Maskintilgang, agenter og oppgaver, samtaler.", ["integrations", "agent", "meldinger"]),
    "verifisering": ("Forankring og verifisering", 10, "Den åpne forankringsfeeden og verifisering per selskap.", ["anchoring", "ledger"]),
    "plattform": ("Plattform (leverandørens konsoll)", 11, "Krever aktiv plattformrolle (`support` leser og tildeler nye medlemskap, `systemadmin` endrer); hvert kall logges, og ingen av endepunktene når en hovedbok. Ikke en del av integrasjons-API-et.", ["auth"]),
}


EXTRA = {
    "verifisering": """## Former

`GET /anchors` er åpen (ingen token) og viser de 20 siste snapshotene:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` gir per snapshot selskapets forankrede kjedehode og Merkle-beviset (`proof`: liste av `{side, sibling}`). `GET /companies/{id}/anchors/verify` kjører hele kontrollen og svarer `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. Se [Verifiser selv](/utviklere/verifiser-selv/).
""",
    "agenter": """## Former

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

Se [MCP og agenter](/utviklere/mcp-og-agenter/).
""",
    "innboks": """## Opplasting

`POST /companies/{id}/inbox?filename=<navn>&sha256=<hex>` med filen som rå kropp og riktig `Content-Type`. `sha256` er valgfri, men gjør et nytt forsøk trygt. Svar: `{ "document_id": "…", "existing": false }`. Se [Konvensjoner](/utviklere/konvensjoner/#idempotens).
""",
}

by = {k: [] for k in PAGES}
seen = set()
for path, meth, h in routes:
    if path in SKIP or path.startswith(("/auth/", "/webhooks/")) or (path, meth) in seen: continue
    seen.add((path, meth))
    by[group(path)].append((path, meth, h))


EN = {
    "identitet": ("Identity and access", "Who am I and who may do what: `/me`, members, roles, invitations, firm, engagement, subscription and the public terms pages."),
    "hovedbok": ("Company and general ledger", "Company onboarding, chart of accounts, vouchers, attachments, period lock, dimensions, accruals, year-end close, balance documentation and cash-register settlement."),
    "salg": ("Sales and subledger", "Invoices, quotes and orders, products and inventory, customers and suppliers, open items and matching."),
    "rapporter": ("Accounting and reports", "Statutory specifications, income statement and balance sheet, VAT and the VAT return, SAF-T, key figures, budget and the audit report."),
    "bank": ("Bank and payments", "Bank reconciliation, OCR giro, payment lists, cards and currency."),
    "personal": ("Payroll, time and expenses", "Employees, payroll runs and the a-melding, tax cards, timesheets and expenses."),
    "innboks": ("Inbox, import and jobs", "The voucher inbox, suggestions, inbound e-mail, SAF-T and CSV import and background jobs."),
    "aksjer": ("Share register and assets", "Shareholders, share events, dividends, RF-1086 and the fixed-asset register."),
    "agenter": ("Integrations, agents and messages", "Machine access, agents and tasks, conversations."),
    "verifisering": ("Anchoring and verification", "The public anchoring feed and per-company verification."),
    "plattform": ("Platform (the vendor console)", "Requires an active platform role (`support` reads and grants new memberships, `systemadmin` changes). Every call is logged and none of the endpoints reach a ledger. Not part of the integration API."),
}
EN_EXTRA = {
    "verifisering": """## Shapes

`GET /anchors` is public (no token) and lists the 20 latest snapshots:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` returns, per snapshot, the company's anchored chain head and the Merkle proof (`proof`: a list of `{side, sibling}`). `GET /companies/{id}/anchors/verify` runs the full check and answers `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. See [Verify it yourself](/en/utviklere/verifiser-selv/).
""",
    "agenter": """## Shapes

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

See [MCP and agents](/en/utviklere/mcp-og-agenter/).
""",
    "innboks": """## Upload

`POST /companies/{id}/inbox?filename=<name>&sha256=<hex>` with the file as the raw body and the right `Content-Type`. `sha256` is optional but makes a retry safe. Response: `{ "document_id": "…", "existing": false }`. See [Conventions](/en/utviklere/konvensjoner/#idempotens).
""",
}

def emit(locale):
    en = locale == "en"
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src/content/docs", "en/utviklere/api" if en else "utviklere/api")
    base = "/en/utviklere" if en else "/utviklere"
    os.makedirs(out, exist_ok=True)
    for key, (title, order, desc, docs) in PAGES.items():
        if en: title, desc = EN[key]
        extra = EN_EXTRA if en else EXTRA
        rows = sorted(by[key], key=lambda r: (r[0], r[1]))
        lines = [f"---\ntitle: {json.dumps(title, ensure_ascii=False)}\ndescription: {json.dumps(desc, ensure_ascii=False)}\neditUrl: false\nsidebar:\n  order: {order}\n---\n", f"{desc}\n"]
        if en:
            lines.append(f"The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles]({base}/tilgang-og-roller/)); \"public\" means no token, \"—\" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.\n")
            hdr, openw, plat = "| Method | Path | Right |", "public", "platform role"
        else:
            lines.append(f"Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller]({base}/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.\n")
            hdr, openw, plat = "| Metode | Sti | Rett |", "åpen", "plattformrolle"
        lines.append(hdr + "\n| --- | --- | --- |")
        for path, meth, h in rows:
            r = plat if key == "plattform" else (openw if path in PUBLIC else (right_of(h) or "—"))
            if r not in (openw, "—", plat):
                r = ", ".join(f"`{x.strip()}`" if x.strip() else x for x in r.split(", "))
            lines.append(f"| {meth} | `{path}` | {r} |")
        if key in extra:
            lines.append("\n" + extra[key])
        lines.append("\n## Further reading (Norwegian)\n" if en else "\n## Les mer\n")
        lines += [f"- [{d.split('/')[0]}](/teknisk/{d.replace('/README','').split('/')[0]}/)" for d in docs if not d.startswith("saft/")]
        if key == "rapporter":
            lines.append("- [SAF-T](https://github.com/BogenTech/regnmed/blob/main/docs/saft/README.md)")
        open(f"{out}/{key}.md", "w").write("\n".join(lines) + "\n")

emit("nb")
emit("en")
print({k: len(v) for k, v in by.items()})
