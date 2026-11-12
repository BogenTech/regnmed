// The arithmetic behind the simple booking form, mirrored from
// regnmed-core so the form can show the lines it will post. The server
// validates and posts; nothing here decides anything it cannot check.

// Input VAT (fradrag) codes the simple form offers, with today's rates.
export const INPUT_VAT = {
  1: { bp: 2500, label: "25 %" },
  11: { bp: 1500, label: "15 %" },
  13: { bp: 1200, label: "12 %" },
  "": { bp: 0, label: "Uten mva-fradrag" },
};

// regnmed-core::vat::split_gross: base rounds half away from zero, the
// VAT is the exact remainder, so base + vat == gross always.
export function splitGross(grossOre, rateBp) {
  const denominator = 10000 + rateBp;
  const base =
    Math.sign(grossOre) * Math.floor((Math.abs(grossOre) * 10000 + Math.floor(denominator / 2)) / denominator);
  return { base, vat: grossOre - base };
}

// Where the money goes for each way a document was paid. A supplier
// invoice is owed to the supplier (2400, with the supplier as party);
// a receipt was paid already, from the bank or in cash.
export const PAYMENT = {
  faktura: { account: "2400", label: "Faktura som skal betales" },
  bank: { account: "1920", label: "Betalt fra bankkonto" },
  kontant: { account: "1900", label: "Betalt kontant" },
};

/// The voucher lines the simple form posts: cost (net, with the input
/// VAT code), the VAT itself on 2710, and the gross against the payment
/// account. Amounts in øre; debit positive, credit negative.
export function simpleLines({ grossOre, vatCode, costAccount, payment, partyNo, avdeling, prosjekt }) {
  const rate = INPUT_VAT[vatCode]?.bp ?? 0;
  const { base, vat } = splitGross(grossOre, rate);
  const lines = [
    {
      account: costAccount,
      amount_ore: base,
      vat_code: vatCode || null,
      party_no: null,
      avdeling: avdeling || null,
      prosjekt: prosjekt || null,
    },
  ];
  if (vat) lines.push({ account: "2710", amount_ore: vat, vat_code: null, party_no: null });
  lines.push({
    account: PAYMENT[payment].account,
    amount_ore: -grossOre,
    vat_code: null,
    party_no: payment === "faktura" ? partyNo : null,
  });
  return lines;
}

// The VAT code whose rate best explains an amount pair from a suggestion
// (the suggestion reads the VAT amount, not the code).
export function guessVatCode(netOre, vatOre) {
  if (!vatOre || !netOre) return "";
  const bp = Math.round((vatOre * 10000) / netOre);
  return Object.entries(INPUT_VAT)
    .filter(([code]) => code)
    .reduce((best, [code, v]) => (Math.abs(v.bp - bp) < Math.abs(INPUT_VAT[best].bp - bp) ? code : best), "1");
}
