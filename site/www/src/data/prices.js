// Single source for the price list shown on www. Not yet in force: billing still runs
// Basis 49 / Standard 99 (docs/abonnement.md). Shape mirrors what the admin-managed
// plan list will serve, so the pages can switch to the API without markup changes.
export const IN_FORCE = false;
export const PLANS = [
  { slug: "liten", name: "Liten", price: 49, vouchers: "ca. 150", audience: "For holdingselskap, enkeltpersonforetak og selskap med lite aktivitet", features: ["Alle funksjoner", "Lønn for 1 ansatt", "Dokumentasjon som support"] },
  { slug: "standard", name: "Standard", price: 129, vouchers: "ca. 1 500", featured: true, audience: "For aktive små og mellomstore bedrifter", features: ["Alle funksjoner", "Lønn for opptil 5 ansatte", "Support på e-post"] },
  { slug: "stor", name: "Stor", price: 299, vouchers: "ca. 10 000", audience: "For selskap med mye aktivitet", features: ["Alle funksjoner", "Lønn for opptil ca. 25 ansatte", "Prioritert support", "Høyere kapasitet for API og agenter (planlagt)"] },
];
export const FROM = Math.min(...PLANS.map((p) => p.price));
