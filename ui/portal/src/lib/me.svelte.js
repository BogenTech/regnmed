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
});

export async function loadMe(force = false) {
  if (me.loaded && !force) return;
  const svar = await api("/me");
  me.name = svar.name || "";
  me.email = svar.email || "";
  me.companies = svar.companies || [];
  // Invitations redeemed by this very call — the invited user's first
  // login should say so instead of silently listing the companies.
  me.nyeTilganger = svar.nye_tilganger || 0;
  me.loaded = true;
}

export function company(companyId) {
  return me.companies.find((c) => c.company_id === companyId) || null;
}
