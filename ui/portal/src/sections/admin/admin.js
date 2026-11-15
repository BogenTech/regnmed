// Pure helpers for the Administrasjon console, tested in admin.test.js.

// Where a registration status came from, in words a user reads.
export const KILDE = {
  brreg: "Enhetsregisteret",
  manuell: "Registrert her",
  migrert: "Utledet, ikke bekreftet",
};

// What is missing before the company can invoice and file, each with the
// tab that fixes it. Only facts that were fetched count: a null input
// (no access, or not loaded) adds nothing rather than a false alarm.
export function gjoremal({ settings, terms, abo }) {
  const ut = [];
  if (settings) {
    if (!settings.address?.trim())
      ut.push({ tekst: "Fyll inn adressen. Fakturaer kan ikke utstedes uten den.", fane: "firma" });
    if (!settings.bank_account?.trim())
      ut.push({ tekst: "Fyll inn kontonummeret som kundene skal betale til.", fane: "firma" });
    if (!settings.email?.trim())
      ut.push({ tekst: "Fyll inn e-postadressen kundene svarer til.", fane: "firma" });
    if (settings.registrering_historikk?.[0]?.kilde === "migrert")
      ut.push({ tekst: "Bekreft registreringen i Mva- og Foretaksregisteret.", fane: "firma" });
  }
  if (terms && !terms.godtatt)
    ut.push({
      tekst: "Godta kundevilkårene, så regnmed kan sende a-melding og andre oppgaver for selskapet.",
      fane: "firma",
    });
  if (abo?.status === "frist")
    ut.push({ tekst: "Abonnementet er ikke betalt. Betal før selskapet sperres.", fane: "abonnement" });
  if (abo?.status === "sperret")
    ut.push({ tekst: "Abonnementet er sperret. Start det igjen for å føre nye bilag.", fane: "abonnement" });
  return ut;
}
