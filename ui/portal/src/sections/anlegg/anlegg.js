// Pure helpers for the Anlegg section, kept out of the components so
// they can be tested without a browser.
import { kr, oreOrNull } from "../../lib/format.js";

// Saldogruppene i sktl. §14-43. The rate in the label is a hint for
// choosing; the rate used is looked up per year in the satsregister.
export const SALDOGRUPPER = [
  ["a", "Kontormaskiner o.l.", 30],
  ["b", "Forretningsverdi", 20],
  ["c", "Vogntog, lastebiler, busser", 24],
  ["d", "Personbiler, maskiner, inventar", 20],
  ["e", "Skip, fartøyer", 14],
  ["f", "Fly, helikopter", 12],
  ["g", "Kraftanlegg", 5],
  ["h", "Bygg og anlegg", 4],
  ["i", "Forretningsbygg", 2],
  ["j", "Teknisk installasjon i bygg", 10],
];

export const gruppeNavn = (g) => SALDOGRUPPER.find((s) => s[0] === g)?.[1] ?? g;

// "2025-03-01" → "03.2025": a depreciation period is a month.
export const maned = (period) => period.slice(5, 7) + "." + period.slice(0, 4);

// The register form as typed → the request body, or the first thing
// that is wrong, said the way the person can fix it.
export function lesDriftsmiddel(f) {
  if (!f.navn.trim()) return { feil: "Gi driftsmidlet et navn" };
  if (!f.dato) return { feil: "Velg anskaffelsesdato" };
  const kostpris = oreOrNull(f.kostpris);
  if (kostpris == null || kostpris <= 0)
    return {
      feil: "Kostprisen må være et beløp i kroner, for eksempel 45 000",
    };
  let restverdi = 0;
  if (String(f.restverdi ?? "").trim()) {
    restverdi = oreOrNull(f.restverdi);
    if (restverdi == null || restverdi < 0)
      return { feil: "Restverdien må være et beløp i kroner, eller tom" };
    if (restverdi >= kostpris)
      return { feil: "Restverdien må være lavere enn kostprisen" };
  }
  const t = String(f.levetid ?? "").trim();
  const levetid = /^\d+$/.test(t) ? Number(t) : NaN;
  if (!(levetid > 0))
    return {
      feil: "Levetiden må være et helt antall måneder, for eksempel 60",
    };
  if (!f.balanse.trim() || !f.avskr.trim())
    return { feil: "Velg balansekonto og avskrivningskonto" };
  return {
    body: {
      navn: f.navn.trim(),
      anskaffelsesdato: f.dato,
      kostpris_ore: kostpris,
      restverdi_ore: restverdi,
      levetid_maneder: levetid,
      balansekonto: f.balanse.trim(),
      avskrivningskonto: f.avskr.trim(),
      saldogruppe: f.gruppe,
    },
  };
}

// The result of a depreciation run in one sentence, with the failures
// listed apart so each can be read and acted on.
export function kjoringTekst(svar) {
  const n = svar.generated;
  const bokfort =
    n === 0
      ? "Ingen avskrivninger var forfalt."
      : n === 1
        ? "1 avskrivning bokført."
        : n + " avskrivninger bokført.";
  const feil = svar.outcomes
    .filter((o) => !o.voucher)
    .map((o) => o.navn + " " + maned(o.period) + ": " + (o.detail || "feilet"));
  return { bokfort, feil };
}

// Gain or loss in words.
export function gevinstTekst(g) {
  return g > 0
    ? "Gevinst " + kr(g) + " kr"
    : g < 0
      ? "Tap " + kr(-g) + " kr"
      : "Verken gevinst eller tap";
}
