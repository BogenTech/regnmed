// Pure helpers for the Oppdrag section: what a firm and an engagement are
// called, and the search over the directory of verified firms.

// The firm's kind decides the oppdrag's kind (regnmed-db::request_engagement).
export const BYRATYPE = { regnskap: "Regnskapsfører", revisjon: "Revisor" };
export const OPPDRAGSTYPE = { regnskap: "Regnskap", revisjon: "Revisjon" };

export const VIS_MAKS = 20;

// Name (any case, any part) or orgnr (spaces ignored). Returns every
// match; the caller shows the first VIS_MAKS and says how many there are.
export function finnByraer(firms, sok, type = "") {
  const q = sok.trim().toLowerCase();
  const sifre = q.replace(/\s/g, "");
  return firms.filter(
    (f) =>
      (!type || f.kind === type) &&
      (!q ||
        f.name.toLowerCase().includes(q) ||
        (/^\d+$/.test(sifre) && f.orgnr.includes(sifre))),
  );
}

// "123456789" → "123 456 789", how Brønnøysund prints it.
export function formatOrgnr(n) {
  return /^\d{9}$/.test(n)
    ? n.slice(0, 3) + " " + n.slice(3, 6) + " " + n.slice(6)
    : n;
}
