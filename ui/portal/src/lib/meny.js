// Seksjonsmenyen — én liste, brukt av skallet og ruteren.

export const SEKSJONER = [
  ["oversikt", "Oversikt"], ["faktura", "Faktura"], ["kunder", "Kunder"],
  ["produkter", "Produkter"],
  ["prosjekter", "Prosjekter"],
  ["timer", "Timer"], ["lonn", "Lønn"], ["utlegg", "Utlegg"], ["anlegg", "Anlegg"],
  ["aksjonarer", "Aksjonærer"],
  ["reskontro", "Reskontro"],
  ["mva", "Mva"], ["rapporter", "Rapporter"], ["bank", "Bank"], ["bilag", "Bilag"],
  ["admin", "Administrasjon"],
];

// Brukere, Oppdrag og Periode gikk inn i Administrasjon-konsollen som
// faner. Adressene består (gamle bokmerker og lenker virker — App.svelte
// ruter dem fortsatt), de står bare ikke i menyen lenger.
export const FLYTTET_TIL_ADMIN = { brukere: "brukere", oppdrag: "oppdrag", periode: "periode" };

// Det en ansatt får se (#54). Portalen SKJULER bare — serveren nekter,
// og det er der sannheten ligger. Menyen finnes for at man ikke skal
// klikke seg inn i en feilmelding, ikke som en sperre. Prosjekter er
// med: registeret er lesbart for ansatte (DIMENSJON_LES i
// ansattbunten — det må finnes noe å føre timene på), skriveknappene
// styres av DIMENSJON_SKRIV inne i seksjonen.
export const ANSATT_MENY = ["prosjekter", "timer", "utlegg", "bilag"];

export function seksjonNavn(slug) {
  const hit = SEKSJONER.find(([s]) => s === slug);
  return hit ? hit[1] : slug;
}
