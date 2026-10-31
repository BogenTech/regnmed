// OIDC authorization code + PKCE mot regnid. Token-utvekslingen proxyes
// via /auth/token (samme origin), så IdP-en trenger ingen browser-CORS,
// og regnmed ser aldri et passord — proxyen videresender bare
// engangskoden.
//
// INGEN TOKENER LAGRES I NETTLESEREN. Tilgangstokenet lever én time og
// ligger bare i minnet her, så det dør med fanen. Refresh-tokenet lever
// 30 dager og ligger i en HttpOnly-cookie proxyen setter: siden kan
// ikke lese det, bare be om en veksling (`POST /auth/refresh`). Kode som
// kjører i siden kan altså bruke den økten som alt er i gang, men aldri
// hente seg en ny — og en omlasting trenger ingen ny innlogging, siden
// cookien fortsatt er der. Begrunnelsen står i docs/portal.md.

export const session = $state({
  config: null, // {issuer, client_id} fra /portal-config
  authed: false,
});

let access = null;

export function tokens() {
  return access;
}

export function saveTokens(t) {
  access = t;
  session.authed = true;
}

export function clearTokens() {
  access = null;
  session.authed = false;
}

// Én veksling i luften om gangen innenfor fanen, delt av alle som ser
// et 401 — ellers ville tre samtidige kall spent cookien tre ganger.
let refreshing = null;

// Veksler cookien inn i et nytt tilgangstoken. Svarer false når økten
// er slutt; da, og bare da, skal brukeren logge inn igjen.
export function refresh() {
  if (refreshing) return refreshing;
  refreshing = (async () => {
    try {
      for (let forsok = 0; forsok < 2; forsok++) {
        const response = await fetch("/auth/refresh", { method: "POST" });
        if (response.ok) {
          // id_token utstedes ikke på nytt av refresh-grantet, så det vi
          // alt har beholdes — utloggingen bruker det som id_token_hint.
          saveTokens(Object.assign({}, access, await response.json()));
          return true;
        }
        // 401 = ingen cookie, altså ingen økt å redde. 400 = utstederen
        // avviste den, og det kan være ROTASJONEN: to faner kan veksle
        // samtidig, og taperen fikk avslag på et token som nettopp ble
        // byttet ut. Vinnerens Set-Cookie havner i den DELTE cookie-
        // burken, så ett forsøk til bærer det nye tokenet av seg selv.
        // (Smett igjen: rekker avslaget fram før vinnerens svar er
        // lagret, blir det likevel en utlogging — som før, aldri verre.)
        if (response.status !== 400) return false;
        await new Promise((r) => setTimeout(r, 400));
      }
      return false;
    } catch (e) {
      return false;
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

// Innlogging fra en annen enhet (RFC 8628): skjermen her viser en kode,
// og en telefon som alt er innlogget godkjenner den. Enhetskoden er en
// legitimasjon og ligger derfor i en HttpOnly-cookie proxyen setter —
// siden viser brukerkoden og holder ingenting.
//
// Økten som kommer ut av dette har INGEN refresh-cookie, med vilje: denne
// skjermen er kanskje ikke brukerens egen, så den varer én time.
export async function startDevice() {
  const response = await fetch("/auth/device", { method: "POST" });
  if (!response.ok) throw new Error("Kunne ikke starte innlogging fra en annen enhet.");
  return await response.json();
}

// true = godkjent og innlogget, false = ingen har svart ennå,
// null = avvist, utløpt eller brukt. Statuskoden bærer forskjellen, så
// siden aldri tolker utstederens ordlyd.
export async function pollDevice() {
  const response = await fetch("/auth/device/poll", { method: "POST" });
  if (response.status === 202) return false;
  if (!response.ok) return null;
  saveTokens(await response.json());
  return true;
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

export async function logout() {
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
  // Tøm cookien før utstederen tar over navigasjonen: uten dette
  // overlever refresh-tokenet utloggingen, og neste besøk logger stille
  // inn igjen — på en delt maskin som forrige bruker.
  try {
    await fetch("/auth/logout", { method: "POST" });
  } catch (e) {
    // Nettet kan svikte; da er det eneste rette å logge ut hos
    // utstederen likevel, så økten ikke ser levende ut.
  }
  const params = new URLSearchParams();
  if (t && t.id_token) params.set("id_token_hint", t.id_token);
  params.set("client_id", session.config.client_id);
  params.set("post_logout_redirect_uri", location.origin + "/");
  const end = session.config.issuer.replace(/\/$/, "") + "/end_session?" + params;
  location.assign(end);
}
