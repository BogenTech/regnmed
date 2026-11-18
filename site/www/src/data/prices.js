// Fallback price list, used only when the API cannot be reached at build time (local dev,
// or an API that does not serve /pricing yet). The live source is the platform console
// (Planer og priser) via GET /pricing; see data/pricing.js. Same shape as the API.
export const FALLBACK = {
  plans: [
    { slug: "liten", name: "Liten", kind: "plan", available: true, audience: "For holdingselskap, enkeltpersonforetak og selskap med lite aktivitet", featured: false, voucher_limit: 150, employee_limit: 1, features: ["Alle funksjoner", "Dokumentasjon som support"], price_ore_per_month: null, in_force: false, upcoming: { price_ore_per_month: 4900, valid_from: null } },
    { slug: "standard", name: "Standard", kind: "plan", available: true, audience: "For aktive små og mellomstore bedrifter", featured: true, voucher_limit: 1500, employee_limit: 5, features: ["Alle funksjoner", "Support på e-post"], price_ore_per_month: null, in_force: false, upcoming: { price_ore_per_month: 12900, valid_from: null } },
    { slug: "stor", name: "Stor", kind: "plan", available: true, audience: "For selskap med mye aktivitet", featured: false, voucher_limit: 10000, employee_limit: 25, features: ["Alle funksjoner", "Prioritert support"], price_ore_per_month: null, in_force: false, upcoming: { price_ore_per_month: 29900, valid_from: null } },
  ],
  addons: [
    { slug: "bank", name: "Bankkobling", kind: "addon", available: false, audience: "Henter bankbevegelsene automatisk i stedet for fra fil", featured: false, voucher_limit: null, employee_limit: null, features: [], price_ore_per_month: null, in_force: false, upcoming: null },
  ],
};
