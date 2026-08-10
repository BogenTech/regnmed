// OIDC authorization code + PKCE mot regnid. Token-utvekslingen proxyes
// via /auth/token (samme origin), så IdP-en trenger ingen browser-CORS,
// og regnmed ser aldri et passord — proxyen videresender bare
// engangskoden. Tokenene ligger i sessionStorage; 401 sender brukeren
// tilbake til innlogging.

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
    encodeURIComponent(location.origin + "/callback") +
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
      redirect_uri: location.origin + "/callback",
    }),
  });
  if (!response.ok) throw new Error("innlogging feilet (" + response.status + ")");
  saveTokens(await response.json());
  history.replaceState(null, "", "/");
}

export function logout() {
  const t = tokens();
  clearTokens();
  // Send the user BACK here after the issuer has ended its session.
  // Without post_logout_redirect_uri they are left standing on the
  // issuer's own "logget ut" page, whose only way onward is a bare
  // /login -- which, having no pending authorize request, finishes at
  // the issuer's account page instead of this application. That reads
  // as "logging in again didn't bring me back".
  //
  // The URI must be pre-registered on the client (it is, for this
  // origin); client_id goes along so the issuer can validate it even
  // when there is no id_token to take it from.
  const params = new URLSearchParams();
  if (t && t.id_token) params.set("id_token_hint", t.id_token);
  params.set("client_id", session.config.client_id);
  params.set("post_logout_redirect_uri", location.origin + "/");
  const end = session.config.issuer.replace(/\/$/, "") + "/end_session?" + params;
  location.assign(end);
}
