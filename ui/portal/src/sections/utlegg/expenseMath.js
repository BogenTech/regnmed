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

// Where a traveller slept: it decides the trekkfri kost rate
// (satsforskriften § 2). Same slugs as regnmed-core::travel::Lodging.
export const LODGING = [
  ["hotell", "Hotell"],
  ["pensjonat", "Pensjonat, brakke eller hybel uten kjøkken"],
  ["privat", "Privat, eller hybel med kjøkken"],
];
export const lodgingName = (key) => LODGING.find(([k]) => k === key)?.[1] || key;

export const DAY_KIND = { overnatting: "Med overnatting", rest: "Påbegynt døgn", dagreise: "Dagreise" };

// A <input type="datetime-local"> gives "2026-03-02T08:00"; the server
// reads seconds too.
export const withSeconds = (v) => (v && v.length === 16 ? v + ":00" : v || null);

// Which døgn of a trip have a night in them — the core's rule
// (regnmed-core::travel::allowance): every full døgn, and the only one of
// a short trip with an overnight stay, must say where the traveller slept
// (true); the part døgn at the end MAY hold a night ("optional"); a day
// trip has none (false). Read as UTC so a daylight-saving change does not
// move a døgn.
export function nights(departure, ret, overnight, count) {
  if (!overnight || !departure || !ret) return Array(count).fill(false);
  const minutes = (new Date(withSeconds(ret) + "Z") - new Date(withSeconds(departure) + "Z")) / 60000;
  const full = Math.floor(minutes / 1440);
  return Array.from({ length: count }, (_, i) => (i < full || full === 0 ? true : "optional"));
}
