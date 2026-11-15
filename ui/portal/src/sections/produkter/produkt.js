// Pure helpers for the Produkter section, tested in produkt.test.js.
import { oreOrNull } from "../../lib/format.js";

// The rate behind each standard VAT code, for a column that would
// otherwise say only "3". Same list as VatSelect.
export const MVA_SATS = { 3: "25 %", 31: "15 %", 33: "12 %", 5: "fritatt", 6: "utenfor" };

export function mvaTekst(code) {
  if (!code) return "uten mva";
  return MVA_SATS[code] ? "mva " + MVA_SATS[code] : "mva-kode " + code;
}

// Active products first, deactivated ones kept apart, and both narrowed
// by a search over number and name.
export function utvalg(products, sok) {
  const q = String(sok ?? "").trim().toLowerCase();
  const treff = q
    ? products.filter((p) => p.nummer.toLowerCase().includes(q) || p.navn.toLowerCase().includes(q))
    : products;
  return { aktive: treff.filter((p) => p.aktiv), deaktiverte: treff.filter((p) => !p.aktiv) };
}

// A price as a person types it. Empty is NOT zero: a product at 0 kr
// would put free lines on every invoice that picks it.
export function lesPris(text, hva = "salgsprisen") {
  const Hva = hva[0].toUpperCase() + hva.slice(1);
  if (!String(text ?? "").trim()) return { feil: "Skriv inn " + hva + " i kroner" };
  const ore = oreOrNull(text);
  if (ore == null) return { feil: Hva + " må være et beløp, for eksempel 250 eller 1 250,50" };
  if (ore < 0) return { feil: Hva + " kan ikke være negativ" };
  return { ore };
}

// The revenue account the server will accept, said before it is asked.
export function lesKonto(text) {
  const k = String(text ?? "").trim();
  if (/^3\d{3}$/.test(k)) return { konto: k };
  return { feil: "Inntektskontoen må være et firesifret nummer i klasse 3 (3000–3999)" };
}

// A quantity in thousandths (2,5 → 2500), or an error to show.
export function lesAntall(text, { tillatNull = false } = {}) {
  const t = String(text ?? "").replace(/\s/g, "").replace(",", ".").replace("−", "-");
  if (!t) return { feil: "Skriv inn antall" };
  const v = Number(t);
  if (!Number.isFinite(v)) return { feil: "Antall må være et tall, for eksempel 12 eller 2,5" };
  const milli = Math.round(v * 1000);
  if (!tillatNull && milli === 0) return { feil: "Antallet kan ikke være null" };
  return { milli };
}

// The count as the server will see it: only the rows someone filled in.
export function tellingLinjer(inventory, talt) {
  const linjer = [];
  for (const r of inventory) {
    const verdi = talt[r.nummer];
    if (verdi == null || String(verdi).trim() === "") continue;
    const a = lesAntall(verdi, { tillatNull: true });
    if (a.feil) return { feil: r.nummer + ": " + a.feil };
    if (a.milli < 0) return { feil: r.nummer + ": talt antall kan ikke være negativt" };
    linjer.push({ produkt: r.nummer, talt_milli: a.milli });
  }
  if (!linjer.length) return { feil: "Fyll inn talt antall for minst ett produkt" };
  return { linjer };
}
