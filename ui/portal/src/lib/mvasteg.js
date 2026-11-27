// What a person can do next with an mva-filing. The server's flags decide,
// so a job that died can never leave the page waiting for ever.

const I_ARBEID = ["godkjent", "oppretter_instans", "instans_opprettet"];
const NY_FRA_START = ["avvik", "avvist", "avbrutt", "lukket_etter_kontroll"];

export function nesteSteg(filing) {
  if (!filing) return { handling: "godkjenn", poll: null, stoppet: false };
  if (filing.jobb_aktiv) return { handling: null, poll: 2_000, stoppet: false };
  if (filing.status === "sendt" && filing.henter_svar) {
    return { handling: null, poll: 15_000, stoppet: false };
  }
  // Delivered: a correction is a new complete melding, and the last one counts.
  if (filing.status === "kvittert" || filing.lukking?.funn === "levert") {
    return { handling: "korriger", poll: null, stoppet: false };
  }
  const stoppet = I_ARBEID.includes(filing.status);
  if (NY_FRA_START.includes(filing.status)) return { handling: "godkjenn", poll: null, stoppet };
  if (!filing.kan_godkjennes) return { handling: null, poll: null, stoppet };
  if (filing.status === "opprettet") return { handling: "godkjenn", poll: null, stoppet };
  return { handling: filing.status === "sendt" ? "hent" : "fortsett", poll: null, stoppet };
}

// A run that stopped. With an instance on record nobody can say how far it
// got, so the text must not suggest that nothing was sent.
export function stoppTekst(filing) {
  return filing?.kan_vaere_sendt
    ? "Innsendingen stoppet, og vi vet ikke hvor langt den kom. Sjekk Altinn før du gjør noe annet."
    : "Innsendingen stoppet før den var ferdig. Logg inn, så fortsetter vi der den stoppet.";
}

// A person's finding from Altinn. Never worded like a receipt regnmed fetched.
export function lukkeTekst(lukking) {
  return lukking.funn === "levert"
    ? "Levert ifølge kontroll i Altinn. regnmed har ikke hentet kvittering for denne innsendingen."
    : "Ikke levert ifølge kontroll i Altinn. Du kan starte en ny innsending.";
}
