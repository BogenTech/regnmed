// Pure helpers for the Byrå view, tested in byra.test.js.

// The engagement kinds as stored, in words a user reads.
const OPPDRAGSTYPE = {
  regnskap: "Regnskapsføring",
  revisjon: "Revisjon",
};

export function oppdragstype(kind) {
  return OPPDRAGSTYPE[kind] || kind;
}

// Active clients first (they are what the page is for), each list sorted
// by name; ended engagements are history and listed apart. `valid_to` is
// exclusive: an engagement that ends today has already ended.
export function delKlienter(clients, idag) {
  const etterNavn = (a, b) => a.company.localeCompare(b.company, "nb");
  const aktiv = (e) => !e.valid_to || e.valid_to > idag;
  return {
    aktive: clients.filter(aktiv).sort(etterNavn),
    avsluttede: clients.filter((e) => !aktiv(e)).sort(etterNavn),
  };
}
