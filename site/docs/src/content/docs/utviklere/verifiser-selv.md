---
title: Verifiser selv
description: Ikke stol på oss, kontroller. Slik etterprøver du at en hovedbok ikke er endret, uten å stole på API-et.
sidebar:
  order: 9
---

regnmed lover ikke at hovedboken er uendret. Den lar deg **kontrollere** det. Garantien står på tre lag, og det siste laget ligger utenfor vår kontroll:

1. **Domenet**: rettelser er reverserende bilag, aldri redigering.
2. **Databasen**: triggere avviser `UPDATE`, `DELETE` og `TRUNCATE` på bilag og posteringer, og applikasjonsrollen har bare `INSERT` og `SELECT`.
3. **Kryptografi**: hvert bilag lagrer `SHA-256(forrige_hash ‖ kanonisk innhold)`. Hvert døgn fryses alle selskapenes kjedehoder under én Merkle-rot som publiseres åpent.

Detaljene står i [Hovedboken](/teknisk/ledger/) og [Forankring](/teknisk/anchoring/). Her er det praktiske.

## Hurtigsjekk: la regnmed kjøre kontrollen

```sh
curl -s https://api.regnmed.no/companies/$COMPANY/anchors/verify -H "Authorization: Bearer $TOKEN"
```

```json
{ "ok": true, "vouchers_checked": 1523, "attachments_checked": 311, "anchors_checked": 42, "problems": [] }
```

Kallet krever rettigheten `FORANKRING_LES` og gjør tre ting: går kjeden fra genesis og regner om hvert bilags hash, sjekker vedleggenes innholdshasher, og sammenligner hvert forankret kjedehode med den levende kjeden. Du får `ok: false` og en liste `problems` med sekvensnummer ved første avvik. Samme sjekk finnes i portalen (Verifiser kjeden mot forankringen) og som revisjonsrapporten (`GET …/reports/revisjon`) med åtte kontroller.

Dette beviser noe bare hvis du stoler på at serveren kjører sin egen kode ærlig. Det neste avsnittet fjerner den forutsetningen.

## Uavhengig sjekk: Merkle-beviset

Den åpne feeden `GET /anchors` krever ingen innlogging og viser rot-hasher. Hver uavhengig kopi av en rot (din egen cron, en revisors notatbok) er ett vitne til, som en omskriving av databasen ikke kan nå.

```sh
curl -s https://api.regnmed.no/anchors | jq '.snapshots[0]'
curl -s https://api.regnmed.no/companies/$COMPANY/anchors?limit=1 -H "Authorization: Bearer $TOKEN" | jq '.anchors[0]'
```

Selskapssvaret har `last_seq`, `last_hash` (kjedehodet som ble forankret), `root_hash` og `proof`, en liste av `{side, sibling}`. Alt du trenger for å regne roten selv:

- **Blad** (format v1, fryst): `SHA-256(0x00 ‖ netstring("regnmed-anchor-v1") ‖ netstring(selskaps-id som 16 råbytes) ‖ netstring(last_seq som desimaltekst) ‖ netstring(last_hash som 32 råbytes))`, der `netstring(x)` er `<lengde>:<x>;`.
- **Node**: `SHA-256(0x01 ‖ venstre ‖ høyre)`.
- **Bevis**: for hvert steg i `proof` legges søsken til venstre (`side: "left"`) eller høyre (`side: "right"`) for den løpende hashen. Sluttresultatet skal være lik `root_hash`, og `root_hash` skal være lik roten i den åpne feeden for samme `snapshot_id`.

Som kode (Python 3, ingen avhengigheter). Selvtesten nederst regner roten for tre blad og krever at den er lik den frosne verdien fra `golden_root_never_changes` i `regnmed-core::anchor`:

```python
import hashlib, uuid

def sha(b): return hashlib.sha256(b).digest()
def field(b): return str(len(b)).encode() + b":" + b + b";"

def leaf_hash(company_id: str, last_seq: int, last_hash_hex: str) -> bytes:
    content = (field(b"regnmed-anchor-v1")
               + field(uuid.UUID(company_id).bytes)
               + field(str(last_seq).encode())
               + field(bytes.fromhex(last_hash_hex)))
    return sha(b"\x00" + content)

def verify_inclusion(company_id, last_seq, last_hash_hex, proof, root_hex) -> bool:
    h = leaf_hash(company_id, last_seq, last_hash_hex)
    for step in proof:                       # {"side": "left"|"right", "sibling": hex}
        sib = bytes.fromhex(step["sibling"])
        h = sha(b"\x01" + (sib + h if step["side"] == "left" else h + sib))
    return h.hex() == root_hex

# Selvtest mot den frosne testen i regnmed-core::anchor (tre blad).
leaves = [(str(uuid.UUID(int=i)), i * 10, f"{i:02x}" * 32) for i in (1, 2, 3)]
l = [leaf_hash(*x) for x in leaves]
root = sha(b"\x01" + sha(b"\x01" + l[0] + l[1]) + l[2])
assert root.hex() == "79c9ecf01bda78fd05002e8f715d08e7d39ba1990adc6601ef79d6ac4fb9eec1"
proof = [{"side": "right", "sibling": l[1].hex()}, {"side": "right", "sibling": l[2].hex()}]
assert verify_inclusion(*leaves[0], proof, root.hex())
print("ok")
```

Med et svar fra API-et blir bruken `verify_inclusion(company_id, a["last_seq"], a["last_hash"], a["proof"], a["root_hash"])` for hvert element `a` i `anchors`.

:::note
Bladene sorteres på selskaps-id og en odde node løftes uendret opp (aldri duplisert), men det trenger du ikke selv: beviset beskriver allerede veien. Formatet er **fryst**. Det kan erstattes av en v2, aldri endres.
:::

## Tidsstempel fra en tredjepart

Når driften har satt `ANCHOR_TSA_URL`, sendes roten til en RFC 3161-tidsstempeltjeneste, og det signerte tokenet («denne hashen fantes senest på tidspunkt T») lagres. Det verifiseres offline med standardverktøy:

```sh
openssl ts -reply -in token.tsr -text
openssl ts -verify -digest <rot-hex> -in token.tsr -CAfile <tsa-ca.pem>
```

Hvilke vitner et snapshot har, står i `witnesses` i begge svarene. Uten `ANCHOR_TSA_URL` finnes forankringen og oppdager fortsatt endringer, den er bare ikke tidsstemplet av en tredjepart.

## Hva hver kontroll avslører

| Kontroll | Slår ned på |
| --- | --- |
| Kjeden regnet om fra genesis | Enhver redigering på stedet uten full tilgang til databasen |
| Forankret kjedehode mot levende kjede | Omskriving eller avkorting etter at snapshotet ble tatt |
| Roten regnes om fra bladene | Tukling med selve forankringsradene |
| Eksternt vitne (feedkopier, RFC 3161) | Sletting eller utskifting av alle ankrene |

Beskyttelsen gjelder historikk eldre enn nyeste vitnede rot. Dagens posteringer beskyttes av neste snapshot (nattlig som standard).

## Kjør det selv mot en egen instans

`regnmed verify-ledger` går kjeden for alle selskaper (eller ett med `--company <uuid>`), sjekker vedleggshasher og forankringene, og **feiler** ved avvik:

```sh
regnmed verify-ledger
regnmed verify-ledger --company 0192e8aa-…
```

Bilagshashen selv (`SHA-256(forrige_hash ‖ kanonisk innhold)`, formatversjon 1 til 4) er beskrevet presist i [Hovedboken](/teknisk/ledger/). De frosne digestene i `golden_hashes_never_change` i `regnmed-core::hash` er den normative fasiten, og versjonen står på hvert bilag (`hash_version`).

## Ikke bygget ennå

- **Vedleggssettet i forankringen.** Vedleggenes *innhold* hash-kontrolleres, men forankringen beviser ikke at et vedlegg fantes ved snapshottidspunktet. Planlagt som en v2 av bladformatet (v1-røtter verifiserer for alltid).
- **Signert feed.** Feeden hviler på mange uavhengige kopier, ikke en signatur.
