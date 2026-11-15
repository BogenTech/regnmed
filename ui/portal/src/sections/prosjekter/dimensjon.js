// Pure helpers for the Prosjekter section, tested in dimensjon.test.js.
import { oreOrNull } from "../../lib/format.js";

// One kind's register: open ones first, closed ones kept apart — a
// closed project is history the list should not have to wade through.
export function delOpp(dims, kind) {
  const egne = dims.filter((d) => d.kind === kind);
  return {
    aktive: egne.filter((d) => d.active),
    avsluttede: egne.filter((d) => !d.active),
  };
}

// An hourly rate as a person types it. An empty field is NOT zero: a
// rate of 0 would bill every hour from that date at nothing.
export function lesSats(text) {
  if (!String(text ?? "").trim()) return { feil: "Skriv inn timesatsen i kroner" };
  const ore = oreOrNull(text);
  if (ore == null) return { feil: "Timesatsen må være et beløp, for eksempel 950 eller 1 250,50" };
  if (ore < 0) return { feil: "Timesatsen kan ikke være negativ" };
  return { ore };
}
