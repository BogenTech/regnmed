// /me-svaret: «selskapene jeg kan opptre for, og som hva». Lastes én
// gang per økt og deles av skallet og seksjonene; reload() etter
// endringer som gir nye tilganger (onboarding, oppdrag).

import { api } from "./api.js";

export const me = $state({ loaded: false, name: "", email: "", companies: [] });

export async function loadMe(force = false) {
  if (me.loaded && !force) return;
  const svar = await api("/me");
  me.name = svar.name || "";
  me.email = svar.email || "";
  me.companies = svar.companies || [];
  me.loaded = true;
}

export function company(companyId) {
  return me.companies.find((c) => c.company_id === companyId) || null;
}
