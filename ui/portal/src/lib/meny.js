// Seksjonsmenyen — én liste, brukt av skallet og ruteren.

export const SEKSJONER = [
  ["oversikt", "Oversikt"], ["faktura", "Faktura"], ["produkter", "Produkter"],
  ["timer", "Timer"], ["lonn", "Lønn"], ["utlegg", "Utlegg"], ["anlegg", "Anlegg"],
  ["aksjonarer", "Aksjonærer"],
  ["reskontro", "Reskontro"],
  ["mva", "Mva"], ["rapporter", "Rapporter"], ["bank", "Bank"], ["bilag", "Bilag"],
  ["periode", "Periode"], ["oppdrag", "Oppdrag"],
];

// Det en ansatt får se (#54). Portalen SKJULER bare — serveren nekter,
// og det er der sannheten ligger. Menyen finnes for at man ikke skal
// klikke seg inn i en feilmelding, ikke som en sperre.
export const ANSATT_MENY = ["timer", "utlegg", "bilag"];

export function seksjonNavn(slug) {
  const hit = SEKSJONER.find(([s]) => s === slug);
  return hit ? hit[1] : slug;
}
