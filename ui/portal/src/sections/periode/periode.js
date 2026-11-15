// Pure helpers for the Periode section, tested in periode.test.js.
import { oreOrNull } from "../../lib/format.js";

// The last day of the month before `iso` (YYYY-MM-DD): the lock most
// companies set next, once a month is done.
export function forrigeManedsslutt(iso) {
  const [y, m] = iso.split("-").map(Number);
  // Day 0 of month m (1-based) is the last day of month m-1.
  const d = new Date(Date.UTC(y, m - 1, 0));
  return d.toISOString().slice(0, 10);
}

// The suggested next lock, or null when the lock is already there.
export function laseForslag(laastTil, iDag) {
  const forslag = forrigeManedsslutt(iDag);
  return !laastTil || laastTil < forslag ? forslag : null;
}

// The tax expense as typed. Empty is not zero: the closing needs the
// number stated, and 0 is a valid answer someone has to give.
export function lesSkatt(tekst) {
  if (!String(tekst ?? "").trim()) return { feil: "Oppgi skattekostnaden. 0 er et gyldig svar." };
  const ore = oreOrNull(tekst);
  if (ore === null) return { feil: "Skattekostnaden må være et beløp, for eksempel 13 200,00." };
  if (ore < 0) return { feil: "Skattekostnaden kan ikke være negativ." };
  return { ore };
}

// Why a year cannot be closed here, or null when it can.
// rader: the closed years; laastTil: the current lock (ISO) or null.
export function kanIkkeAvslutte(ar, iDag, rader, laastTil, erAdmin) {
  const naa = Number(iDag.slice(0, 4));
  if (ar >= naa) return `${ar} er ikke over ennå. Året kan avsluttes fra 1. januar ${ar + 1}.`;
  const avsluttet = rader?.find((r) => r.ar === ar);
  if (avsluttet) return `${ar} er avsluttet med bilag ${avsluttet.bilag}.`;
  if (laastTil && laastTil >= `${ar}-12-31` && !erAdmin)
    return `Perioden er låst forbi 31.12.${ar}. Bare en administrator kan avslutte året, fordi låsen må flyttes tilbake mens bilaget føres.`;
  return null;
}
