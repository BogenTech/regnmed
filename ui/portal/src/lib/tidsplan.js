// Tidsplaner (den generelle planleggeren): rene hjelpere for det portalen viser og sender.
// Tidene er norsk lokal tid som «HH:MM», både inn og ut — serveren eier tidssonen.

// «3:15, 12.00» -> ["03:15", "12:00"]: sortert, uten duplikater. Kaster med en forklaring.
export function parseTider(tekst, maksPerDag) {
  const deler = String(tekst ?? "")
    .split(/[,;\s]+/)
    .filter(Boolean);
  if (deler.length === 0) throw new Error("oppgi minst ett klokkeslett, for eksempel 03:15");
  const tider = deler.map((d) => {
    const m = /^(\d{1,2})[:.](\d{2})$/.exec(d);
    if (!m || Number(m[1]) > 23 || Number(m[2]) > 59) throw new Error("«" + d + "» er ikke et klokkeslett (skriv som 03:15)");
    return m[1].padStart(2, "0") + ":" + m[2];
  });
  const unike = [...new Set(tider)].sort();
  if (maksPerDag != null && unike.length > maksPerDag)
    throw new Error("høyst " + maksPerDag + " klokkeslett per dag");
  return unike;
}

// ["03:15","12:00","18:00"] -> «03:15, 12:00 og 18:00»
export function tiderTekst(tider) {
  const t = tider ?? [];
  return t.length <= 1 ? (t[0] ?? "") : t.slice(0, -1).join(", ") + " og " + t[t.length - 1];
}

// Linjen i Bankkobling-kortet.
export function planTekst(plan) {
  if (!plan.aktiv) return "Automatisk henting er satt på pause";
  const tider = tiderTekst(plan.tider);
  return tider ? "Henter automatisk hver dag kl. " + tider : "Automatisk henting har ingen klokkeslett";
}

// Utfallet av siste kjøring: { tekst, feil } eller null når det ikke har vært noen.
// `naar` er tidspunktet ferdig formatert (kalleren eier datoformatet).
export function sistTekst(sist, naar) {
  if (!sist) return null;
  if (sist.status === "queued" || sist.status === "running") return { tekst: "Henter nå …", feil: false };
  const foran = "Sist" + (naar ? " " + naar : "") + ": ";
  // The job's own error text is for the operator (English, from the worker): never shown here.
  if (sist.status === "failed") return { tekst: foran + "hentingen feilet. Prøv «Hent nå», eller ta kontakt hvis det gjentar seg.", feil: true };
  const r = sist.resultat ?? {};
  if (r.feil?.length) return { tekst: foran + r.feil[0], feil: true };
  const nye = r.nye ?? 0;
  return {
    tekst: foran + (nye === 0 ? "ingen nye bankposter" : nye + (nye === 1 ? " ny bankpost" : " nye bankposter")),
    feil: false,
  };
}

// Status i plattformkonsollens tabell.
export const JOBBSTATUS = { queued: "i kø", running: "kjører", done: "ferdig", failed: "feilet" };
