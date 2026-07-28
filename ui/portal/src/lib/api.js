// API-klienten — portert fra app.js. Ingen tilgangslogikk her: serveren
// nekter, klienten viser. 401 tømmer økten; App.svelte reagerer på
// session.authed og viser innloggingen.

import { tokens, clearTokens } from "./auth.svelte.js";

export async function api(path, options = {}) {
  const t = tokens();
  if (!t) {
    clearTokens();
    throw new Error("ikke innlogget");
  }
  options.headers = Object.assign(
    { authorization: "Bearer " + t.access_token },
    options.headers || {},
  );
  const response = await fetch(path, options);
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
