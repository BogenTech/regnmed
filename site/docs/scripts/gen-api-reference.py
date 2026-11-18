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

os.makedirs(OUT, exist_ok=True)
for key, (title, order, desc, docs) in PAGES.items():
    rows = sorted(by[key], key=lambda r: (r[0], r[1]))
    lines = [f"---\ntitle: {json.dumps(title, ensure_ascii=False)}\ndescription: {json.dumps(desc, ensure_ascii=False)}\nsidebar:\n  order: {order}\n---\n", f"{desc}\n"]
    lines.append("Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.\n")
    lines.append("| Metode | Sti | Rett |\n| --- | --- | --- |")
    for path, meth, h in rows:
        r = "plattformrolle" if key == "plattform" else ("åpen" if path in PUBLIC else (right_of(h) or "—"))
        r = ", ".join(f"`{x.strip()}`" if x.strip() and x.strip() not in ("åpen","—") else x for x in r.split(", ")) if r not in ("åpen","—","plattformrolle") else r
        lines.append(f"| {meth} | `{path}` | {r} |")
    if key in EXTRA:
        lines.append("\n" + EXTRA[key])
    lines.append("\n## Les mer\n")
    lines += [f"- [{d.split('/')[0]}](/teknisk/{d.replace('/README','').split('/')[0]}/)" for d in docs if not d.startswith("saft/")]
    if key == "rapporter":
        lines.append("- [SAF-T](https://github.com/BogenTech/regnmed/blob/main/docs/saft/README.md)")
    open(f"{OUT}/{key}.md", "w").write("\n".join(lines) + "\n")
print({k: len(v) for k, v in by.items()})
