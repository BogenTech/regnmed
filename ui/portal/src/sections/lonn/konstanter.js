// Lønnsseksjonens faste lister — portert uendret fra app.js.
//
// Sonelisten er bevisst UTEN sone Ia: fribeløpet på 850 000 er
// bagatellmessig støtte som også forbrukes utenfor regnmed, så
// serveren avviser sonen (docs/lonn.md). Den skal derfor heller ikke
// kunne velges her.
export const AGA_SONER = [
  ["I", "I — 14,1 %"],
  ["II", "II — 10,6 %"],
  ["III", "III — 6,4 %"],
  ["IV", "IV — 5,1 %"],
  ["IVa", "IVa — 7,9 %"],
  ["V", "V — 0 %"],
];

export const MANEDER = [
  "januar",
  "februar",
  "mars",
  "april",
  "mai",
  "juni",
  "juli",
  "august",
  "september",
  "oktober",
  "november",
  "desember",
];
