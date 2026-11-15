// Pure helpers for the Aksjonærer section, tested in aksjonaer.test.js.
import { formatDate } from "../../lib/format.js";

// Ownership in basis points, the Norwegian way: 6000 → "60,00 %".
export function andelTekst(bp) {
  return (bp / 100).toFixed(2).replace(".", ",") + " %";
}

// A share count as a person types it. Shares are whole, "1 000" is a
// thousand (parseInt would read 1), and zero or less is not an event.
export function lesAksjer(text) {
  const t = String(text ?? "").replace(/[\s .]/g, "");
  if (!t) return { feil: "Skriv inn antall aksjer" };
  if (!/^\d+$/.test(t)) return { feil: "Antall aksjer må være et helt tall, for eksempel 1 000" };
  const antall = Number(t);
  if (antall <= 0) return { feil: "Antall aksjer må være minst 1" };
  return { antall };
}

// The 35 types, grouped the way a person thinks about them: shares in
// and shares out. Each disposal named like its acquisition gets
// "(avgang)" so the two cannot be mistaken in the list.
export function grupperTyper(typer) {
  const tilgangNavn = new Set(typer.filter((t) => t.tilgang).map((t) => t.navn));
  const vis = (t) => ({
    ...t,
    visning: t.navn + (!t.tilgang && tilgangNavn.has(t.navn) ? " (avgang)" : "") + (t.leverbar ? "" : " ⚠"),
  });
  return {
    tilganger: typer.filter((t) => t.tilgang).map(vis),
    avganger: typer.filter((t) => !t.tilgang).map(vis),
  };
}

// Aksjeloven §4-5 asks for the birth DATE, never the number.
export function identTekst(a) {
  if (a.orgnr) return "org.nr. " + a.orgnr;
  if (a.utenlandsk_id) return a.utenlandsk_id;
  return a.fodselsdato ? "født " + formatDate(a.fodselsdato) : "";
}

export function adresseTekst(a) {
  return [a.adresse, [a.postnummer, a.poststed].filter(Boolean).join(" "), a.landkode]
    .filter(Boolean)
    .join(", ");
}

// The sentence a person confirms before an event is written for good.
export function hendelseTekst({ type, antall, holder, motpart, dato }) {
  const n = antall.toLocaleString("nb-NO") + (antall === 1 ? " aksje" : " aksjer");
  const retning = !motpart ? "" : type.tilgang ? " fra " + motpart : " til " + motpart;
  return `${type.navn}: ${n} for ${holder}${retning}, ${formatDate(dato)}.`;
}
