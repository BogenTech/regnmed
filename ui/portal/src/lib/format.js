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

export function today() {
  return new Date().toISOString().slice(0, 10);
}

export function antallStr(milli) {
  return String(milli / 1000).replace(".", ",");
}

export function parseAntall(text) {
  const value = Number(String(text).replace(/\s/g, "").replace(",", "."));
  if (!isFinite(value)) throw new Error("ugyldig antall: " + text);
  return Math.round(value * 1000);
}
