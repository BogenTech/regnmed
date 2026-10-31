// API-klienten — portert fra app.js. Ingen tilgangslogikk her: serveren
// nekter, klienten viser. Et 401 forsøkes vekslet én gang mot
// refresh-tokenet; holder ikke det, tømmes økten og App.svelte reagerer
// på session.authed og viser innloggingen.

import { tokens, clearTokens, refresh } from "./auth.svelte.js";

export async function api(path, options = {}) {
  const t = tokens();
  if (!t) {
    clearTokens();
    throw new Error("ikke innlogget");
  }
  const attempt = (access_token) =>
    fetch(
      path,
      Object.assign({}, options, {
        headers: Object.assign(
          { authorization: "Bearer " + access_token },
          options.headers || {},
        ),
      }),
    );
  let response = await attempt(t.access_token);
  // Tilgangstokenet lever en time, refresh-cookien 30 dager. Ett forsøk
  // bak den delte vekslingen i auth.svelte.js — et 401 etter det er en
  // død økt, ikke et utløpt token.
  if (response.status === 401 && (await refresh())) {
    response = await attempt(tokens().access_token);
  }
  if (response.status === 401) {
    clearTokens();
    throw new Error("utløpt økt");
  }
  if (!response.ok) {
    let detail = "";
    try {
      detail = (await response.json()).error || "";
    } catch (e) {
      /* ikke json */
    }
    throw new Error(detail || "feil (" + response.status + ")");
  }
  const type = response.headers.get("content-type") || "";
  return type.includes("application/json") ? response.json() : response;
}

export function post(path, body) {
  return api(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

// PUT og DELETE — samme som post, men med metoden oppgitt.
export function send(method, path, body) {
  return api(path, {
    method,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body || {}),
  });
}
