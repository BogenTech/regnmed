// Hash-ruting — samme adresser som dagens portal (#/c/{id}/{seksjon}),
// så en lenke kan flyttes mellom / og /ny uendret.

function parse() {
  const parts = location.hash.replace(/^#\/?/, "").split("?")[0].split("/");
  if (parts[0] === "byra" && parts[1]) {
    return { view: "byra", firmId: parts[1] };
  }
  if (parts[0] === "c" && parts[1]) {
    return {
      view: "company",
      companyId: parts[1],
      section: parts[2] || "oversikt",
      extra: parts[3] || null,
    };
  }
  return { view: "companies" };
}

export const route = $state(parse());

window.addEventListener("hashchange", () => {
  Object.assign(route, { firmId: null, companyId: null, section: null, extra: null }, parse());
});
