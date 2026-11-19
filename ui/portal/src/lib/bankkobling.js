// Bankkobling (direkte bankforbindelse): rene hjelpere for kortet i Bank-seksjonen.
// Serveren eier statusen; her avgjøres bare hva som vises og når det skal mases.

const DAG = 86_400_000;

// Hele dager til samtykket utløper (0 = i dag, negativt = utløpt). null uten dato.
export function dagerIgjen(gyldigTil, idagIso) {
  if (!/^\d{4}-\d{2}-\d{2}/.test(gyldigTil ?? "")) return null;
  return Math.round((Date.parse(gyldigTil.slice(0, 10)) - Date.parse(idagIso.slice(0, 10))) / DAG);
}

// Merkelapp og om samtykket bør fornyes (utløpt, eller 14 dager eller mindre igjen).
export function koblingsstatus(k, idagIso) {
  const dager = dagerIgjen(k.gyldig_til, idagIso);
  if (k.status === "avsluttet") return { tekst: "frakoblet", klasse: "badge-ghost", forny: false };
  if (k.status === "venter") return { tekst: "venter på banken", klasse: "badge-info", forny: true };
  if (k.status === "utlopt" || (dager !== null && dager < 0))
    return { tekst: "samtykket er utløpt", klasse: "badge-warning", forny: true };
  if (dager !== null && dager <= 14)
    return {
      tekst: dager === 0 ? "utløper i dag" : "utløper om " + dager + (dager === 1 ? " dag" : " dager"),
      klasse: "badge-warning",
      forny: true,
    };
  return { tekst: "aktiv", klasse: "badge-success", forny: false };
}

// «1234 56 78903» -> «•••• •• 78903»: nok til å kjenne igjen kontoen, ikke mer.
export function maskert(kontonummer) {
  const siffer = String(kontonummer ?? "").replace(/\D/g, "");
  return siffer.length >= 5 ? "•••• •• " + siffer.slice(-5) : "";
}

// Det banken sender nettleseren tilbake med (#/c/{id}/bank?bankkode=&banktilstand= | &bankfeil=).
export function retur(query) {
  const p = new URLSearchParams(query ?? "");
  const kode = p.get("bankkode"), tilstand = p.get("banktilstand"), feil = p.get("bankfeil");
  if (feil) return { feil };
  return kode && tilstand ? { kode, tilstand } : null;
}

// Bankvalget i skjemaet: verdien er plassen i listen (så leverandøren følger med),
// og leverandøren vises bare når to oppføringer har samme banknavn.
export function bankvalg(banker) {
  const antall = {};
  for (const b of banker) antall[b.name] = (antall[b.name] ?? 0) + 1;
  return banker.map((b, i) => [String(i), antall[b.name] > 1 && b.provider ? b.name + " (" + b.provider + ")" : b.name]);
}

// Er adressen bankens retur (#/c/{id}/bank?bankkode=… eller ?bankfeil=…)? Innloggingen
// tar vare på akkurat den, så en utlogget bruker ikke mister koden fra banken.
export function erBankretur(hash) {
  const i = String(hash ?? "").indexOf("?");
  return i >= 0 && retur(hash.slice(i + 1)) !== null;
}
