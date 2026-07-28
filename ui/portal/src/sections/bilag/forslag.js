// Bilagstolkning (#34, docs/bilagstolkning.md): serveren svarer med et
// FORSLAG — aldri en bokføring. Her oversettes svaret til skjemaets
// linjer, med kilden og begrunnelsene intakt så brukeren ser hva
// forslaget bygger på.

import { kr, today } from "../../lib/format.js";

const KILDE_TEKST = {
  ehf: "lest fra EHF",
  "pdf-tekst": "tolket fra PDF-teksten",
  tekst: "tolket fra teksten",
};

// kr() bruker typografisk minus; skjemaets parser vil ha ASCII.
function belop(ore) {
  return kr(ore).replace("−", "-");
}

export function forslagTilSkjema(f) {
  const netto = f.netto_ore !== null ? f.netto_ore : f.brutto_ore;
  const lines = [
    {
      account: f.konto || "4300",
      amount: belop(netto),
      vat: f.mva_ore ? "1" : "",
      party_no: null,
      avdeling: "",
      prosjekt: "",
    },
  ];
  if (f.mva_ore) {
    lines.push({
      account: "2710",
      amount: belop(f.mva_ore),
      vat: "",
      party_no: null,
      avdeling: "",
      prosjekt: "",
    });
  }
  lines.push({
    account: "2400",
    amount: belop(-f.brutto_ore),
    vat: "",
    party_no: f.leverandor_no || null,
    avdeling: "",
    prosjekt: "",
  });

  return {
    date: f.dato || today(),
    description:
      (f.selger_navn || f.leverandor_navn || "Bilag") +
      (f.fakturanr ? " faktura " + f.fakturanr : ""),
    lines,
    kildeTekst: KILDE_TEKST[f.kilde] || f.kilde,
    hvorfor: (f.begrunnelser || [])
      .map((b) => b.felt + ": " + b.hvorfor)
      .concat(f.konto_begrunnelse ? ["konto: " + f.konto_begrunnelse] : [])
      .concat(f.warnings || []),
  };
}
