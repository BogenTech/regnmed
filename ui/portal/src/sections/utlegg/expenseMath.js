// Utlegg i ord og tall. Statusene sies slik den som venter på pengene
// leser dem, og kjøregodtgjørelsen regnes som regnmed-core::expenses
// gjør det (km × sats, den trekkfrie delen kan ikke overstige beløpet)
// — så skjemaet viser det samme beløpet serveren vil lagre.

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

// Hele kilometer, ingenting annet: «12,5» og «12 km» er ikke tall vi
// gjetter på.
export function parseKm(text) {
  const t = String(text ?? "").trim();
  return /^\d{1,5}$/.test(t) && Number(t) > 0 ? Number(t) : null;
}

// Representasjon gir aldri fradrag for inngående mva (mval. § 8-3 første
// ledd bokstav d). Varsel, ikke sperre: avgjørelsen er regnskapsførerens.
export const REPRESENTATION_ACCOUNTS = ["7350", "7370"];
