// Hash-ruting — samme adresser som dagens portal (#/c/{id}/{seksjon}),
// så en lenke kan flyttes mellom / og /ny uendret.
//
// Spørringen (#/c/{id}/mva?year=&termin=) hører med til adressen: den
// gjør en bestemt termin delbar. Den leses HER, ett sted, så ingen
// seksjon trenger sin egen hashchange-lytter.

function parse() {
  const hash = location.hash.replace(/^#\/?/, "");
  const [sti, query = ""] = hash.split("?");
  const parts = sti.split("/");
  if (parts[0] === "byra" && parts[1]) {
    return { view: "byra", firmId: parts[1], query };
  }
  if (parts[0] === "c" && parts[1]) {
    return {
      view: "company",
      companyId: parts[1],
      section: parts[2] || "oversikt",
      extra: parts[3] || null,
      query,
    };
  }
  return { view: "companies", query };
}

export const route = $state(parse());

window.addEventListener("hashchange", () => {
  Object.assign(
    route,
    { firmId: null, companyId: null, section: null, extra: null, query: "" },
    parse(),
  );
});
