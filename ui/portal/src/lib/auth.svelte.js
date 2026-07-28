// OIDC authorization code + PKCE mot regnid — portert fra app.js.
// Token-utvekslingen proxyes via /auth/token (samme origin), så IdP-en
// trenger ingen browser-CORS. Samme sessionStorage-nøkler som dagens
// portal, så en økt følger med mellom / og /ny i samme fane.
//
// Eneste forskjell: redirect-URI-en er /ny/callback (må være registrert
// på klienten i regnid — scripts/dev-sso.sh gjør det i dev).

export const session = $state({
  config: null, // {issuer, client_id} fra /portal-config
  authed: false,
});

export function tokens() {
  try {
    return JSON.parse(sessionStorage.getItem("regnmed-tokens"));
  } catch (e) {
    return null;
  }
}

export function saveTokens(t) {
  sessionStorage.setItem("regnmed-tokens", JSON.stringify(t));
  session.authed = true;
}

export function clearTokens() {
  sessionStorage.removeItem("regnmed-tokens");
  session.authed = false;
}

function b64url(bytes) {
  return btoa(String.fromCharCode.apply(null, new Uint8Array(bytes)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function randomString() {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return b64url(bytes);
}

export async function login() {
  const verifier = randomString();
  const state = randomString();
  const challenge = b64url(
    await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)),
  );
  sessionStorage.setItem("regnmed-pkce", JSON.stringify({ verifier, state }));
  const url =
    session.config.issuer.replace(/\/$/, "") +
    "/authorize" +
    "?response_type=code&client_id=" +
    encodeURIComponent(session.config.client_id) +
    "&redirect_uri=" +
    encodeURIComponent(location.origin + "/ny/callback") +
    "&scope=" +
    encodeURIComponent("openid profile email") +
    "&state=" +
    state +
    "&nonce=" +
    randomString() +
    "&code_challenge=" +
    challenge +
    "&code_challenge_method=S256";
  location.assign(url);
}

export async function handleCallback() {
  const params = new URLSearchParams(location.search);
  const pkce = JSON.parse(sessionStorage.getItem("regnmed-pkce") || "null");
  sessionStorage.removeItem("regnmed-pkce");
  if (!params.get("code") || !pkce || params.get("state") !== pkce.state) {
    throw new Error("ugyldig innloggingssvar");
  }
  const response = await fetch("/auth/token", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      code: params.get("code"),
      code_verifier: pkce.verifier,
      redirect_uri: location.origin + "/ny/callback",
    }),
  });
  if (!response.ok) throw new Error("innlogging feilet (" + response.status + ")");
  saveTokens(await response.json());
  history.replaceState(null, "", "/ny");
}

export function logout() {
  const t = tokens();
  clearTokens();
  let end = session.config.issuer.replace(/\/$/, "") + "/end_session";
  if (t && t.id_token) end += "?id_token_hint=" + encodeURIComponent(t.id_token);
  location.assign(end);
}
