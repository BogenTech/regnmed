// The invoice arithmetic of regnmed-core, mirrored so the new-invoice
// form can show the exact sum while the user types. The server stays
// authoritative: it recomputes on issue with the rate valid on the
// invoice date. Rates here are today's (the same table VatSelect shows).

export const VAT_CODES = {
  3: { bp: 2500, label: "25 %" },
  31: { bp: 1500, label: "15 %" },
  33: { bp: 1200, label: "12 %" },
  5: { bp: 0, label: "0 % (fritatt)" },
  6: { bp: 0, label: "Utenfor mva" },
};

const roundHalfAway = (numerator, denominator) =>
  Math.sign(numerator) * Math.floor((Math.abs(numerator) + denominator / 2) / denominator);

// regnmed-core::invoice::line_net_ore: quantity in thousandths × øre.
export function lineNetOre(quantityMilli, unitPriceOre) {
  return roundHalfAway(quantityMilli * unitPriceOre, 1000);
}

// regnmed-core::vat::vat_of_base: basis points, half away from zero.
export function vatOfBase(baseOre, rateBp) {
  return roundHalfAway(baseOre * rateBp, 10000);
}

// regnmed-db::timesheet::milli_hours: minutes to thousandths of an hour.
export function milliHours(minutes) {
  return Math.floor((minutes * 1000 + 30) / 60);
}
