#!/usr/bin/env python3
"""Registrerer regnmed i Altinns systemregister (docs/gov.md, #180).

    scripts/altinn-system.py show       # slik systemet står registrert
    scripts/altinn-system.py register   # første gang
    scripts/altinn-system.py update     # etter endring i SYSTEM under

ÉTT system, ikke to: en Maskinporten-klient kan bare knyttes til ett
system (AUTH.VLD-00004), og systembrukertokenet finner systemet gjennom
klient-id-en. Systemet bærer derfor begge kundeformene, og hver
systembrukerforespørsel velger sin del:

  - egen virksomhet:  standard systembruker med `rights` (Skatteetatens
                      ressurser)
  - regnskapsfører:   klient-systembruker (agent) med ÉN tilgangspakke —
                      pakkene på en systembruker er OG (docs/gov.md), så
                      forespørselen ber om én, ikke alle tre.

Bare testmiljøet (TT02) foreløpig; produksjon får egen klient og egen
env-fil når testintegrasjonen er verifisert. Leser
~/.config/regnmed/maskinporten-test.env (docs/secrets.md).
"""
import base64, json, os, subprocess, sys, time, urllib.error, urllib.parse, urllib.request, uuid

PLATFORM = "https://platform.tt02.altinn.no/authentication/api/v1/systemregister/vendor"
VENDOR = "935115086"
SYSTEM_ID = f"{VENDOR}_regnmed"


def b64(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


env = {}
for line in open(os.path.expanduser("~/.config/regnmed/maskinporten-test.env")):
    line = line.strip()
    if line and not line.startswith("#") and "=" in line:
        k, v = line.split("=", 1)
        env[k] = v

SYSTEM = {
    "id": SYSTEM_ID,
    "vendor": {"authority": "iso6523-actorid-upis", "ID": f"0192:{VENDOR}"},
    "name": {"nb": "RegnMed", "nn": "RegnMed", "en": "RegnMed"},
    "description": {
        "nb": "Regnskapssystem for små og mellomstore virksomheter og deres regnskapsførere. "
        "Leverer a-melding, henter skattekort og leverer aksjonærregisteroppgaven.",
        "nn": "Rekneskapssystem for små og mellomstore verksemder og rekneskapsførarane deira. "
        "Leverer a-melding, hentar skattekort og leverer aksjonærregisteroppgåva.",
        "en": "Accounting system for small and medium-sized businesses and their accountants. "
        "Files the a-melding, fetches tax cards and files the shareholder register return.",
    },
    # Egen virksomhet: Skatteetatens ressurser (API-dokumentasjonen, ikke gjettet).
    "rights": [
        {"resource": [{"id": "urn:altinn:resource", "value": r}]}
        for r in (
            "ske-innrapportering-amelding",
            "ske-skattekort-til-arbeidsgiver",
            "ske-innrapportering-aksjonaerregisteroppgave",
        )
    ],
    # Regnskapsfører: de tre pakkene ER-rollen gir, alle fra REGN (regel 2).
    "accessPackages": [
        {"urn": f"urn:altinn:accesspackage:{p}"}
        for p in (
            "regnskapsforer-med-signeringsrettighet",
            "regnskapsforer-uten-signeringsrettighet",
            "regnskapsforer-lonn",
        )
    ],
    "clientId": [env["MASKINPORTEN_CLIENT_ID"]],
    "allowedredirecturls": ["https://test.regnmed.no", "https://regnmed.no"],
    # Must be false with client packages (AUTH.VLD-00018). The system is
    # then not listed in Altinn for a customer to pick — every systembruker
    # starts as a request regnmed sends, and the customer approves it via
    # the confirmUrl. That was the plan anyway.
    "isVisible": False,
}


def token(scope):
    now = int(time.time())
    header = {"alg": "RS256", "typ": "JWT"}
    if env.get("MASKINPORTEN_KID"):
        header["kid"] = env["MASKINPORTEN_KID"]
    claims = {
        "aud": env["MASKINPORTEN_AUDIENCE"],
        "iss": env["MASKINPORTEN_CLIENT_ID"],
        "scope": scope,
        "iat": now,
        "exp": now + 60,
        "jti": str(uuid.uuid4()),
    }
    signing_input = (
        f"{b64(json.dumps(header, separators=(',', ':')).encode())}."
        f"{b64(json.dumps(claims, separators=(',', ':')).encode())}"
    )
    sig = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", env["MASKINPORTEN_KEY_FILE"]],
        input=signing_input.encode(), capture_output=True, check=True,
    ).stdout
    data = urllib.parse.urlencode({
        "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
        "assertion": f"{signing_input}.{b64(sig)}",
    }).encode()
    with urllib.request.urlopen(env["MASKINPORTEN_TOKEN_ENDPOINT"], data=data) as r:
        return json.load(r)["access_token"]


def call(method, url, body=None):
    req = urllib.request.Request(
        url, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={
            "Authorization": f"Bearer {token('altinn:authentication/systemregister.write')}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req) as r:
            text = r.read().decode()
            print(r.status)
    except urllib.error.HTTPError as e:
        text = e.read().decode()
        print(e.code)
    try:
        print(json.dumps(json.loads(text), indent=2, ensure_ascii=False))
    except ValueError:
        print(text)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "show"
    if cmd == "show":
        call("GET", f"{PLATFORM}/{SYSTEM_ID}")
    elif cmd == "register":
        call("POST", PLATFORM, SYSTEM)
    elif cmd == "update":
        call("PUT", f"{PLATFORM}/{SYSTEM_ID}", SYSTEM)
    else:
        sys.exit(__doc__)
