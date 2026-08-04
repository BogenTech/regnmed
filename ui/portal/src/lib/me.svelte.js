// /me-svaret: «selskapene jeg kan opptre for, og som hva». Lastes én
// gang per økt og deles av skallet og seksjonene; reload() etter
// endringer som gir nye tilganger (onboarding, oppdrag).

import { api } from "./api.js";

export const me = $state({
  loaded: false,
  name: "",
  email: "",
  companies: [],
  nyeTilganger: 0,
  // Aktiv plattformrolle (docs/auth.md §8): {rolle, valid_to} eller null.
  // Gir INGEN selskabstilgang — bare Plattform-visningen i menyen.
  plattform: null,
});

export async function loadMe(force = false) {
  if (me.loaded && !force) return;
  const svar = await api("/me");
  me.name = svar.name || "";
  me.email = svar.email || "";
  me.companies = svar.companies || [];
  me.plattform = svar.plattform || null;
  // Invitations redeemed by this very call — the invited user's first
  // login should say so instead of silently listing the companies.
  me.nyeTilganger = svar.nye_tilganger || 0;
  me.loaded = true;
}

export function company(companyId) {
  return me.companies.find((c) => c.company_id === companyId) || null;
}

// Rettighetene fra /me — BARE visning: seksjonene skjuler knapper som
// ville fått 403, serveren nekter uansett (docs/auth.md). Mangler
// listen (gammelt svar), vises ingenting skrivbart — fail-closed.
export function harRett(companyId, rett) {
  const c = company(companyId);
  return !!c && Array.isArray(c.rettigheter) && c.rettigheter.includes(rett);
}
