// Modal questions replacing the browser's built-in confirm()/prompt().
// Same pattern as toast.svelte.js: this module owns the state,
// Dialog.svelte (mounted once in App.svelte) renders it as a daisyUI
// modal, and callers await the promise — so a call site reads exactly
// like the old `if (!confirm(...)) return`, just with await. Esc and a
// backdrop click resolve as cancel, matching the native dialogs.

export const dialog = $state({ aktiv: null });

// Yes/no. Resolves true on OK, false on cancel.
// valg: { tittel, ok, avbryt, farlig } — farlig renders OK as btn-error,
// for the one-way actions (revocations, oppsigelse, avhending).
export function bekreft(melding, valg = {}) {
  return new Promise((resolve) => {
    dialog.aktiv = {
      type: "bekreft",
      tittel: valg.tittel ?? "Er du sikker?",
      melding,
      ok: valg.ok ?? "OK",
      avbryt: valg.avbryt ?? "Avbryt",
      farlig: !!valg.farlig,
      resolve,
    };
  });
}

// One or more fields in a single modal. Resolves an object keyed by each
// field's `navn` (string values trimmed), or null on cancel. A field is
// required unless `valgfri`; OK stays disabled until every required
// field has content — validation prompt() never had.
// felt: { navn, etikett, standard, valgfri, hjelp,
//         type: text|date|textarea|select|checkbox,
//         valg: select options as [verdi, etikett] pairs }
export function skjema(tittel, felter, valg = {}) {
  return new Promise((resolve) => {
    dialog.aktiv = {
      type: "skjema",
      tittel,
      melding: valg.melding,
      felter,
      ok: valg.ok ?? "OK",
      avbryt: valg.avbryt ?? "Avbryt",
      farlig: !!valg.farlig,
      resolve,
    };
  });
}

// prompt() equivalent: one field, resolves the (trimmed) string or null.
// valg: { standard, type, valgfri, ok, melding, farlig }
export function sporsmal(melding, valg = {}) {
  return skjema(
    melding,
    [
      {
        navn: "svar",
        standard: valg.standard ?? "",
        type: valg.type ?? "text",
        valgfri: !!valg.valgfri,
      },
    ],
    { ok: valg.ok, melding: valg.melding, farlig: valg.farlig },
  ).then((svar) => (svar ? svar.svar : null));
}
