// Planer og priser i plattformkonsollen: skjemaverdier -> forespørselskropper.
// Penger er heltall øre hele veien: kroner leses som TEKST (aldri via flyttall),
// og alt som ikke er et rent beløp avvises i stedet for å rundes stille.

// "49", "49,5", "1 299,00" -> øre. null for tomt, negativt, mer enn to desimaler
// eller noe som ikke er et tall.
export function strictOre(text) {
  const t = String(text ?? "").replace(/[\s ]/g, "");
  const m = /^(\d+)(?:[.,](\d{1,2}))?$/.exec(t);
  if (!m) return null;
  const ore = Number(m[1]) * 100 + Number((m[2] ?? "").padEnd(2, "0") || 0);
  return Number.isSafeInteger(ore) ? ore : null;
}

// Serveren leser tallene som 32-bits heltall.
const I32_MAX = 2147483647;

export const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

export function featuresFromText(text) {
  return String(text ?? "")
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

// Tom = ingen grense (null); ellers et heltall >= 0.
function limit(text, navn) {
  const t = String(text ?? "").replace(/[\s ]/g, "");
  if (!t) return null;
  if (!/^\d+$/.test(t)) throw new Error(navn + " må være et helt tall, eller tomt for ingen grense");
  if (Number(t) > I32_MAX) throw new Error(navn + " er for stor");
  return Number(t);
}

export function planBody(v) {
  const name = String(v.name ?? "").trim();
  if (!name) throw new Error("planen trenger et navn");
  const sort = String(v.sort ?? "").trim() === "" ? 0 : Number(v.sort);
  if (!Number.isInteger(sort) || Math.abs(sort) > I32_MAX) throw new Error("rekkefølge må være et helt tall");
  // Tilleggstjenester kan merkes «ikke tilgjengelig» (vises som «kommer», kan ikke kjøpes);
  // en pakke er alltid tilgjengelig.
  const kind = v.kind === "addon" ? "addon" : "plan";
  return {
    kind,
    available: kind === "addon" ? !!v.available : true,
    name,
    audience: String(v.audience ?? "").trim(),
    featured: !!v.featured,
    listed: !!v.listed,
    voucher_limit: limit(v.voucher_limit, "grensen for bilag"),
    employee_limit: limit(v.employee_limit, "grensen for ansatte"),
    features: featuresFromText(v.features),
    sort,
  };
}

// Prisen gjelder fra i dag eller senere — aldri bakover, så en faktura som alt er
// utstedt aldri kan ha hatt en annen pris enn den prislisten viste.
export function priceBody(v, todayIso) {
  const price_ore = strictOre(v.price);
  if (price_ore === null) throw new Error("prisen må være et beløp i kroner, med høyst to desimaler");
  if (!/^\d{4}-\d{2}-\d{2}$/.test(v.valid_from ?? "")) throw new Error("velg en gyldig-fra-dato");
  if (v.valid_from < todayIso) throw new Error("prisen kan ikke gjelde bakover i tid");
  const source = String(v.source ?? "").trim();
  if (!source) throw new Error("oppgi kilden (vedtak eller sak)");
  return { interval: v.interval === "year" ? "year" : "month", price_ore, valid_from: v.valid_from, source };
}

// Prisen som gjelder nå og den neste som er vedtatt, for ett intervall.
// `prices` er nyeste først, slik API-et sender dem.
export function currentAndNext(prices, interval, todayIso) {
  const rows = (prices ?? []).filter((p) => p.interval === interval);
  const current = rows.find((p) => p.valid_from <= todayIso) ?? null;
  const future = rows.filter((p) => p.valid_from > todayIso);
  return { current, next: future.length ? future[future.length - 1] : null };
}
