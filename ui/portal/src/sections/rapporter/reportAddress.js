// The report page's address: #/c/{id}/rapporter/{tab}?year=2026&side=2.
// Tab and year are the page; every other key is the open tab's own
// state (account filter, page, customer/supplier), so a link, a reload
// or the back button from a voucher lands where the user was.

export const TABS = [
  ["saldobalanse", "Saldobalanse"],
  ["resultat", "Resultat"],
  ["balanse", "Balanse"],
  ["budsjett", "Budsjett"],
  ["prosjekt", "Prosjekt"],
  ["kontospesifikasjon", "Kontospesifikasjon"],
  ["reskontrospesifikasjon", "Reskontro"],
  ["bokforingsspesifikasjon", "Bokføringsspesifikasjon"],
  ["balansedokumentasjon", "Balansedokumentasjon"],
  ["revisjon", "Revisjon"],
  ["saft", "SAF-T"],
];

export function parseAddress(extra, query, thisYear) {
  const q = new URLSearchParams(query || "");
  const tab = TABS.some(([key]) => key === extra) ? extra : "saldobalanse";
  const y = Number(q.get("year"));
  const year = Number.isInteger(y) && y >= 1900 && y <= 2999 ? y : thisYear;
  q.delete("year");
  return { tab, year, params: Object.fromEntries(q) };
}

export function buildAddress(companyId, tab, year, params) {
  const q = new URLSearchParams({ year: String(year) });
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== "") q.set(key, String(value));
  }
  return "#/c/" + companyId + "/rapporter/" + tab + "?" + q;
}
