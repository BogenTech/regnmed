// Expense claims in words and numbers. Statuses are said the way the
// person waiting for the money reads them, and mileage is computed as
// regnmed-core::expenses does (km × rate, the tax-free part never above
// the amount) — so the form shows the amount the server will store.

export const STATUS = {
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
