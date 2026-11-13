// The arithmetic behind the manual voucher form, mirrored from
// regnmed-core so the form shows the lines it will post. A line's amount
// is GROSS when it carries a VAT code: the VAT is split out
// (split_gross, the same rounding as the core) onto 2710 for input VAT
// or 2700 for output VAT, with the code on the net line — the same shape
// the Bilag form posts. The server validates and posts; nothing here
// decides anything it cannot check.
import { INPUT_VAT, splitGross } from "../bilag/bookingMath.js";
import { oreOrNull } from "../../lib/format.js";

// Input VAT comes from the Bilag form's own table, so a rate is changed
// in one place; output VAT (sales) is only posted by hand here.
export const VAT = {
  ...Object.fromEntries(
    Object.entries(INPUT_VAT)
      .filter(([code]) => code)
      .map(([code, v]) => [code, { bp: v.bp, account: "2710", label: "Inngående " + v.label + " (" + code + ")" }]),
  ),
  3: { bp: 2500, account: "2700", label: "Utgående 25 % (3)" },
  31: { bp: 1500, account: "2700", label: "Utgående 15 % (31)" },
  33: { bp: 1200, account: "2700", label: "Utgående 12 % (33)" },
};

// Signed øre of one form line: debit positive, credit negative; null
// when neither column holds a number, NaN when both or either is invalid.
export function lineOre(line) {
  const d = String(line.debit ?? "").trim();
  const c = String(line.credit ?? "").trim();
  if (!d && !c) return null;
  if (d && c) return NaN;
  const v = oreOrNull(d || c);
  if (v === null || v <= 0) return NaN;
  return d ? v : -v;
}

// The lines that will be posted, VAT split out.
export function postingLines(lines) {
  const out = [];
  for (const l of lines) {
    const ore = lineOre(l);
    if (!l.account?.trim() || ore === null || Number.isNaN(ore)) continue;
    const vat = VAT[l.vat];
    const dims = { avdeling: l.avdeling || null, prosjekt: l.prosjekt || null };
    if (!vat) {
      out.push({ account: l.account.trim(), amount_ore: ore, vat_code: null, ...dims });
      continue;
    }
    const { base, vat: vatOre } = splitGross(ore, vat.bp);
    out.push({ account: l.account.trim(), amount_ore: base, vat_code: l.vat, ...dims });
    if (vatOre) out.push({ account: vat.account, amount_ore: vatOre, vat_code: null, avdeling: null, prosjekt: null });
  }
  return out;
}
