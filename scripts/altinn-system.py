#!/usr/bin/env python3
"""Registrerer regnmed i Altinns systemregister (docs/gov.md, #180).

    scripts/altinn-system.py show       # slik systemet står registrert
    scripts/altinn-system.py register   # første gang
    scripts/altinn-system.py update     # etter endring i SYSTEM under
    scripts/altinn-system.py request <orgnr>   # standard systembruker for et selskap
    scripts/altinn-system.py status <id>       # en forespørsels status og confirmUrl
    scripts/altinn-system.py change <orgnr>    # ask an EXISTING system user for the
                                               # rights added to SYSTEM since
    scripts/altinn-system.py change-status <id>  # a change request's status and confirmUrl

A new resource in `rights` only reaches new system users. One that is
already approved gets it through a change request (`change`), which the
company approves the same way as the first request. The request is
idempotent: resources the system user already has are left alone.

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

AUTH = "https://platform.tt02.altinn.no/authentication/api/v1"
PLATFORM = f"{AUTH}/systemregister/vendor"
REQUESTS = f"{AUTH}/systemuser/request/vendor"
CHANGES = f"{AUTH}/systemuser/changerequest/vendor"
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
        "Leverer a-melding, aksjonærregisteroppgaven, skattemeldingen og SAF-T ved kontroll, "
        "og henter skattekort og trekkpålegg.",
        "nn": "Rekneskapssystem for små og mellomstore verksemder og rekneskapsførarane deira. "
        "Leverer a-melding, aksjonærregisteroppgåva, skattemeldinga og SAF-T ved kontroll, "
        "og hentar skattekort og trekkpålegg.",
        "en": "Accounting system for small and medium-sized businesses and their accountants. "
        "Files the a-melding, fetches tax cards and garnishment orders, and files the "
        "shareholder register return, the tax return and SAF-T for tax audits.",
    },
    # Egen virksomhet: Skatteetatens ressurser (API-dokumentasjonen, ikke gjettet).
    "rights": [
        {"resource": [{"id": "urn:altinn:resource", "value": r}]}
        for r in (
            "ske-innrapportering-amelding",
            "ske-skattekort-til-arbeidsgiver",
            "ske-innrapportering-aksjonaerregisteroppgave",
            # Garnishment orders (#334, docs/trekkpalegg.md); scope ordered in SSV-6690.
            "ske-informasjon-om-trekkpaalegg",
            # SAF-T delivered in a control (#183, docs/saft/README.md); scope ordered in SSV-6691.
            "ske-kontroll",
            # Skattemelding with næringsspesifikasjon (#11): Skatteetaten's Altinn 3 app,
            # needed for both their API and Altinn's (onboarding guide, docs/gov.md); scope in SSV-6691.
            "app_skd_formueinntekt-skattemelding-v2",
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


def call(method, url, body=None, scope="altinn:authentication/systemregister.write"):
    req = urllib.request.Request(
        url, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={
            "Authorization": f"Bearer {token(scope)}",
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
        parsed = json.loads(text)
    except ValueError:
        print(text)
        return None
    print(json.dumps(parsed, indent=2, ensure_ascii=False))
    return parsed


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "show"
    if cmd == "show":
        call("GET", f"{PLATFORM}/{SYSTEM_ID}")
    elif cmd == "register":
        call("POST", PLATFORM, SYSTEM)
    elif cmd == "update":
        call("PUT", f"{PLATFORM}/{SYSTEM_ID}", SYSTEM)
    elif cmd == "request" and len(sys.argv) == 3:
        # Egen virksomhet: alle ressursene i `rights`. Svaret har confirmUrl, som
        # daglig leder (eller en med tilgangsstyring) åpner og godkjenner.
        call("POST", REQUESTS, {
            "systemId": SYSTEM_ID,
            "partyOrgNo": sys.argv[2],
            "rights": SYSTEM["rights"],
        }, scope="altinn:authentication/systemuser.request.write")
    elif cmd == "status" and len(sys.argv) == 3:
        call("GET", f"{REQUESTS}/{sys.argv[2]}",
             scope="altinn:authentication/systemuser.request.read")
    elif cmd == "change" and len(sys.argv) == 3:
        # Look up the system user's id, then ask for every right in SYSTEM.
        query = urllib.parse.urlencode({"system-id": SYSTEM_ID, "orgno": sys.argv[2]})
        found = call("GET", f"{AUTH}/systemuser/vendor/byquery?{query}",
                     scope="altinn:authentication/systemuser.request.write")
        if not isinstance(found, dict) or not found.get("id"):
            sys.exit(f"no system user for {SYSTEM_ID} at {sys.argv[2]}")
        params = urllib.parse.urlencode(
            {"correlation-id": str(uuid.uuid4()), "system-user-id": found["id"]})
        call("POST", f"{CHANGES}?{params}", {"requiredRights": SYSTEM["rights"]},
             scope="altinn:authentication/systemuser.request.write")
    elif cmd == "change-status" and len(sys.argv) == 3:
        call("GET", f"{CHANGES}/{sys.argv[2]}",
             scope="altinn:authentication/systemuser.request.read")
    else:
        sys.exit(__doc__)
