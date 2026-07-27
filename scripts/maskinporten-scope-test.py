#!/usr/bin/env python3
"""Spør Maskinporten om et scope faktisk er tilgjengelig for klienten.

    scripts/maskinporten-scope-test.py skatteetaten:innrapporteringamelding

Svaret skiller mellom tilstandene som ellers blandes sammen:

  GRANTED           scopet virker
  IKKE TILDELT      «Consumer has not been granted access» — Skatteetaten
                    har ikke gitt virksomheten tilgang. Å huke det av i
                    Samarbeidsportalen hjelper ikke; det må BESTILLES.
  IKKE PÅ KLIENTEN  «invalid scopes for client» — scopet er ukjent for
                    klienten
  OPPSETTFEIL       invalid_grant: nøkkel, kid eller klient-id

Skillet mellom «ikke tildelt» og «ikke på klienten» er hele grunnen til
at skriptet finnes: det ene løses i portalen, det andre bare av
Skatteetaten.

Leser ~/.config/regnmed/maskinporten-test.env (docs/secrets.md). Ingen
avhengigheter utover openssl.
"""
import base64, json, subprocess, sys, time, urllib.request, urllib.parse, uuid, os

def b64(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()

env = {}
for line in open(os.path.expanduser("~/.config/regnmed/maskinporten-test.env")):
    line = line.strip()
    if line and not line.startswith("#") and "=" in line:
        k, v = line.split("=", 1); env[k] = v

scope = sys.argv[1]
now = int(time.time())
header = {"alg": "RS256", "typ": "JWT"}
if env.get("MASKINPORTEN_KID"): header["kid"] = env["MASKINPORTEN_KID"]
claims = {
    "aud": env["MASKINPORTEN_AUDIENCE"],
    "iss": env["MASKINPORTEN_CLIENT_ID"],
    "scope": scope,
    "iat": now, "exp": now + 60,
    "jti": str(uuid.uuid4()),
}
signing_input = f"{b64(json.dumps(header,separators=(',',':')).encode())}.{b64(json.dumps(claims,separators=(',',':')).encode())}"
sig = subprocess.run(
    ["openssl", "dgst", "-sha256", "-sign", env["MASKINPORTEN_KEY_FILE"]],
    input=signing_input.encode(), capture_output=True, check=True).stdout
assertion = f"{signing_input}.{b64(sig)}"

data = urllib.parse.urlencode({
    "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
    "assertion": assertion,
}).encode()
try:
    with urllib.request.urlopen(urllib.request.Request(
            env["MASKINPORTEN_TOKEN_ENDPOINT"], data=data,
            headers={"Content-Type": "application/x-www-form-urlencoded"})) as r:
        body = json.load(r)
        print(f"  {scope}\n    GRANTED (token, expires_in={body.get('expires_in')})")
except urllib.error.HTTPError as e:
    detail = e.read().decode()
    if "has not been granted access" in detail:
        verdict = "IKKE TILDELT — Skatteetaten må gi virksomheten tilgang (bestilles)"
    elif "invalid scopes for client" in detail:
        verdict = "IKKE PÅ KLIENTEN — legg scopet til på klienten i Samarbeidsportalen"
    elif "invalid_grant" in detail:
        verdict = "OPPSETTFEIL — sjekk nøkkel, MASKINPORTEN_KID og klient-id"
    else:
        verdict = detail[:200]
    print(f"  {scope}\n    {verdict}")
except Exception as e:
    print(f"  {scope}\n    ERROR: {e}")
