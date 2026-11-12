// Tall- og datoformatering — portert uendret fra app.js. Svelte escaper
// selv, så esc() finnes ikke lenger: verdier legges i markup som tekst.

export function kr(ore) {
  const negative = ore < 0;
  const abs = Math.abs(ore);
  const whole = Math.floor(abs / 100)
    .toString()
    .replace(/\B(?=(\d{3})+(?!\d))/g, " ");
  return (negative ? "−" : "") + whole + "," + String(abs % 100).padStart(2, "0");
}

export function parseKr(text) {
  const cleaned = String(text).replace(/[\s ]/g, "").replace(",", ".");
  const value = Number(cleaned);
  if (!isFinite(value)) throw new Error("ugyldig beløp: " + text);
  return Math.round(value * 100);
}

// Today's date where the user is. toISOString() alone is UTC, which in
// Norway is still yesterday until 01:00/02:00.
export function today(now = new Date()) {
  return new Date(now.getTime() - now.getTimezoneOffset() * 60000).toISOString().slice(0, 10);
}

export function antallStr(milli) {
  return String(milli / 1000).replace(".", ",");
}

// Timeføring (docs/timer.md): minutter som heltall, redigerbart til
// måneden låses eller timene faktureres.
export function minutterTilTimer(min) {
  return (min / 60).toFixed(2).replace(".", ",").replace(/,?0+$/, "") || "0";
}

export function parseAntall(text) {
  const value = Number(String(text).replace(/\s/g, "").replace(",", "."));
  if (!isFinite(value)) throw new Error("ugyldig antall: " + text);
  return Math.round(value * 1000);
}

// Whole kroner for summaries (13 750,40 → 13 750). Exact øre stay in
// tables and documents; a dashboard reads faster without them.
export function krWhole(ore) {
  return kr(Math.sign(ore) * Math.round(Math.abs(ore) / 100) * 100).slice(0, -3);
}

// ISO date (2026-06-20, or a timestamp) → 20.06.2026, the form Norwegian
// users read. Anything that is not an ISO date passes through unchanged.
export function formatDate(iso) {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso || "");
  return m ? m[3] + "." + m[2] + "." + m[1] : iso;
}
