// Pakkegrensene i portalen. Serveren håndhever (ansatte og integrasjoner nektes over
// grensen, bokføring stoppes aldri); her vises bare hvor selskapet står.

const n = (x) => new Intl.NumberFormat("nb-NO").format(x);

// null-grense = uten grense.
export const naadd = (antall, grense) => grense != null && antall >= grense;

// «312 av 150 bilag i år», eller «312 bilag i år» uten grense.
export const bruktekst = (antall, grense, enhet) =>
  grense == null ? n(antall) + " " + enhet : n(antall) + " av " + n(grense) + " " + enhet;

// Den billigste pakken der dagens bruk får plass under alle grensene.
export function minsteSomPasser(planer, bruk) {
  if (!bruk) return null;
  const passer = (p) =>
    (p.bilag_grense == null || bruk.bilag <= p.bilag_grense) &&
    (p.ansatte_grense == null || bruk.ansatte <= p.ansatte_grense) &&
    (p.integrasjoner_grense == null || bruk.integrasjoner <= p.integrasjoner_grense);
  return (planer ?? []).filter(passer).sort((a, b) => a.pris_ore_per_mnd - b.pris_ore_per_mnd)[0] ?? null;
}
