// Expense claims in words and numbers. Statuses are said the way the
// person waiting for the money reads them, and mileage is computed as
// regnmed-core::expenses does (km × rate, the tax-free part never above
// the amount) — so the form shows the amount the server will store.

export const STATUS = {
  utkast: { text: "Utkast", badge: "badge-ghost" },
  innsendt: { text: "Venter på godkjenning", badge: "badge-warning" },
  godkjent: { text: "Godkjent, venter på utbetaling", badge: "badge-info" },
  utbetalt: { text: "Utbetalt", badge: "badge-success" },
  avvist: { text: "Avvist", badge: "badge-error" },
};

export function mileage(km, rateOrePerKm, taxFreeOrePerKm) {
  const amount = km * rateOrePerKm;
  const taxFree = Math.min(km * taxFreeOrePerKm, amount);
  return { amount, taxFree, taxable: amount - taxFree };
}

// Whole kilometres only: "12,5" and "12 km" are not numbers we guess at.
export function parseKm(text) {
  const t = String(text ?? "").trim();
  return /^\d{1,5}$/.test(t) && Number(t) > 0 ? Number(t) : null;
}

// Business entertainment never gives input VAT deduction (mval. § 8-3
// first paragraph letter d). A warning, not a block: the decision is the
// accountant's.
export const REPRESENTATION_ACCOUNTS = ["7350", "7370"];

// The vehicles the rate register pays (særavtale reise i Norge § 3). A
// car is paid the same whatever it runs on; zero-emission is kept because
// tolls are not.
export const VEHICLES = [
  ["bil", "Bil (bensin, diesel, hybrid)"],
  ["bil_utslippsfri", "Bil, utslippsfri (el, hydrogen)"],
  ["motorsykkel", "Motorsykkel over 125 ccm"],
  ["moped", "Moped eller motorsykkel til og med 125 ccm"],
  ["snoscooter", "Snøscooter eller ATV"],
  ["bat", "Båt med motor"],
];

export const vehicleName = (key) => VEHICLES.find(([k]) => k === key)?.[1] || key;

// Costs of a trip, reimbursed at their actual amount with a receipt.
export const COST_KINDS = ["Bompenger", "Rushtidsavgift", "Ferje", "Parkering", "Annet"];
