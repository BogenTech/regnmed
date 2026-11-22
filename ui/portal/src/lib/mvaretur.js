// Coming back from ID-porten after approving an mva-melding. The answer is
// kept until the Mva page can post it with the person's own token, so it
// survives a login in between. The company and filing ride in `state`.

const KEY = "regnmed-mva-retur";
const TERM = "regnmed-mva-termin";

export function lesRetur(search) {
  const q = new URLSearchParams(search);
  const state = q.get("state") ?? "";
  const [companyId, filingId, nonce] = state.split(".");
  if (!companyId || !filingId || !nonce) return null;
  const code = q.get("code");
  const error = q.get("error");
  if (!code && !error) return null;
  return { companyId, filingId, state, code, error };
}

// The address to land on, with the term the person was looking at.
export function returAdresse(retur, termin) {
  return "#/c/" + encodeURIComponent(retur.companyId) + "/mva" + (termin ? "?" + termin : "");
}

export function husk(retur) {
  sessionStorage.setItem(KEY, JSON.stringify(retur));
}

export function huskTermin(year, termin) {
  sessionStorage.setItem(TERM, new URLSearchParams({ year, termin }).toString());
}

export function tattTermin() {
  const value = sessionStorage.getItem(TERM);
  sessionStorage.removeItem(TERM);
  return value;
}

export const harRetur = () => sessionStorage.getItem(KEY) !== null;

// Handed out once: a code that is posted twice would only be refused.
export function taRetur(companyId) {
  let retur = null;
  try {
    retur = JSON.parse(sessionStorage.getItem(KEY));
  } catch {
    sessionStorage.removeItem(KEY);
  }
  if (retur?.companyId !== companyId) return null;
  sessionStorage.removeItem(KEY);
  return retur;
}
