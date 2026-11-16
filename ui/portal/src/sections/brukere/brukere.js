// Pure helpers for the Brukere tab, tested in brukere.test.js.

// The built-in roles are stored by key; a user reads these names. A
// custom role is named by the company itself and passes through.
const ROLLENAVN = {
  ansatt: "Ansatt",
  les: "Lesetilgang",
  revisor: "Revisor",
  bokforing: "Bokføring",
  admin: "Administrator",
};

export function rolleNavn(rolle) {
  return ROLLENAVN[rolle] || rolle;
}

// What changing a member's role costs, in words, or null when the change
// is routine. Taking away admin, or changing your own role, can lock
// someone (possibly you) out of this very page, so those are confirmed.
export function rolleendringAdvarsel(medlem, ny, megSelv) {
  if (medlem.rolle === ny) return null;
  if (megSelv && medlem.rolle === "admin")
    return "Du gir deg selv rollen «" + rolleNavn(ny) + "» og mister administratortilgangen. Du kan ikke ta den tilbake selv.";
  if (megSelv) return "Du endrer din egen rolle til «" + rolleNavn(ny) + "».";
  if (medlem.rolle === "admin")
    return medlem.navn + " mister administratortilgangen og får rollen «" + rolleNavn(ny) + "».";
  return null;
}

// How many of a group's rights are ticked — shown on the closed group so
// the choice is visible without opening it.
export function antallValgt(retter, valgte) {
  return retter.filter((v) => valgte.includes(v.rett)).length;
}

// Same set of rights, order ignored: «Lagre» is offered only on a change.
export function sammeSett(a, b) {
  return a.length === b.length && a.every((x) => b.includes(x));
}
