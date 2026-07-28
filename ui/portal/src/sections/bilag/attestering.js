// Attesteringsmerket på et innboksdokument (#47). Delt mellom køen og
// innboksen, så de aldri kan si forskjellige ting om samme bilag.

export function attesteringMerke(d, policyAktiv) {
  if (d.attestering === "godkjent") {
    return { klasse: "badge-success", tekst: "attestert", av: d.attestert_av || "" };
  }
  if (d.attestering === "avvist") {
    return { klasse: "badge-error", tekst: "avvist", av: d.attestert_av || "" };
  }
  return policyAktiv ? { klasse: "badge-warning", tekst: "venter", av: "" } : null;
}
