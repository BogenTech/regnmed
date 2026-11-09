// API-klienten — portert fra app.js. Ingen tilgangslogikk her: serveren
// nekter, klienten viser. Et 401 forsøkes vekslet én gang mot
// refresh-tokenet; holder ikke det, tømmes økten og App.svelte reagerer
// på session.authed og viser innloggingen.

import { tokens, clearTokens, refresh } from "./auth.svelte.js";

// GET response cache: switching sections back and forth must not wait on
// the network. A response is reused for FRESH_MS, and ANY write (POST/PUT/
// DELETE) empties the whole cache, so your own changes always show at once.
// Someone else's change can be up to FRESH_MS old after a section switch,
// exactly like a page left open. Returning to the tab empties it too. The
// text is stored, not the object, so a section that mutates its response
// can never alter the cache. /me redeems invitations and is never cached,
// nor is a job's status, which is polled until it changes.
const FRESH_MS = 30_000;
const cache = new Map(); // path -> { at, text }
const inflight = new Map(); // path -> Promise<text|Response>
// Bumped on every clear: a GET that started before a write must not store
// what it read once it lands after the write.
let generation = 0;

export function clearCache() {
  generation += 1;
  cache.clear();
  inflight.clear();
}
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") clearCache();
});

export async function api(path, options = {}) {
  const method = (options.method || "GET").toUpperCase();
  if (method !== "GET") {
    clearCache();
    try {
      return await request(path, options);
    } finally {
      clearCache(); // anything read while the write was in flight
    }
  }
  // A job's status is polled until it changes, so it is never cached.
  if (path === "/me" || path.includes("/jobs/")) return request(path, options);
  const hit = cache.get(path);
  if (hit && Date.now() - hit.at < FRESH_MS) return JSON.parse(hit.text);
  // Two identical reads at once (two cards needing the same list) share a call.
  let pending = inflight.get(path);
  if (!pending) {
    const started = generation;
    pending = request(path, options, (text) => {
      if (generation === started) cache.set(path, { at: Date.now(), text });
    });
    inflight.set(path, pending);
    pending.finally(() => inflight.get(path) === pending && inflight.delete(path)).catch(() => {});
  }
  const result = await pending;
  return typeof result === "string" ? JSON.parse(result) : result;
}

// store: given on the GET path, which wants JSON as text for the cache.
async function request(path, options, store = null) {
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
  if (!type.includes("application/json")) return response;
  if (!store) return response.json();
  const text = await response.text();
  store(text);
  return text;
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
