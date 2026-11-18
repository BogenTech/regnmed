// Fallback price list, used only when the API cannot be reached at build time (local dev,
// an API blip, or an API that does not serve /pricing yet). It is the prisvedtak IN FORCE
// (docs/abonnement.md), never a proposal: a failed fetch must not publish unapproved prices.
// The live source is the platform console (Planer og priser) via GET /pricing; see
// data/pricing.js. Same shape as the API.
export const FALLBACK = {
  plans: [
    { slug: "basis", name: "Basis", kind: "plan", available: true, audience: "Selvbetjent", featured: false, voucher_limit: null, employee_limit: null, features: ["Alle funksjoner", "Dokumentasjon som support"], price_ore_per_month: 4900, in_force: true, upcoming: null },
    { slug: "standard", name: "Standard", kind: "plan", available: true, audience: "Med e-postsupport", featured: true, voucher_limit: null, employee_limit: null, features: ["Alle funksjoner", "Support på e-post"], price_ore_per_month: 9900, in_force: true, upcoming: null },
  ],
  addons: [
    { slug: "bank", name: "Bankkobling", kind: "addon", available: false, audience: "Henter bankbevegelsene automatisk i stedet for fra fil", featured: false, voucher_limit: null, employee_limit: null, features: [], price_ore_per_month: null, in_force: false, upcoming: null },
  ],
};
