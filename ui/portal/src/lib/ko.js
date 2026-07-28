// Kvitteringsfoto og offline-kø (#48) — portert fra app.js.
//
// Bare OPPLASTINGER køes. Hovedboken lagres aldri lokalt: et regnskap
// som viser gamle tall uten dekning er verre enn et som sier fra.
// Bildet hashes i telefonen, så serveren kan kjenne igjen det samme
// bildet sendt to ganger — nettverk gjentar seg, og et bilag skal
// ikke bli to.

import { api } from "./api.js";
import { toast } from "./toast.svelte.js";

const KO_DB = "regnmed-ko";

function koApne() {
  return new Promise((ok, feil) => {
    const req = indexedDB.open(KO_DB, 1);
    req.onupgradeneeded = () => {
      req.result.createObjectStore("bilder", { keyPath: "id", autoIncrement: true });
    };
    req.onsuccess = () => ok(req.result);
    req.onerror = () => feil(req.error);
  });
}

function koLegg(rad) {
  return koApne().then(
    (db) =>
      new Promise((ok, feil) => {
        const tx = db.transaction("bilder", "readwrite");
        tx.objectStore("bilder").add(rad);
        tx.oncomplete = () => ok();
        tx.onerror = () => feil(tx.error);
      }),
  );
}

export function koLes() {
  return koApne().then(
    (db) =>
      new Promise((ok, feil) => {
        const req = db.transaction("bilder").objectStore("bilder").getAll();
        req.onsuccess = () => ok(req.result);
        req.onerror = () => feil(req.error);
      }),
  );
}

function koFjern(id) {
  return koApne().then(
    (db) =>
      new Promise((ok) => {
        const tx = db.transaction("bilder", "readwrite");
        tx.objectStore("bilder").delete(id);
        tx.oncomplete = () => ok();
      }),
  );
}

async function filHash(buffer) {
  if (!crypto.subtle) return null;
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/// Sender ett bilde. Returnerer false når det er nettet som svikter —
/// da hører bildet hjemme i køen, ikke i en feilmelding.
async function sendBilde(companyId, filnavn, type, buffer, sha256) {
  try {
    await api(
      "/companies/" +
        companyId +
        "/inbox?filename=" +
        encodeURIComponent(filnavn) +
        (sha256 ? "&sha256=" + sha256 : ""),
      {
        method: "POST",
        headers: { "content-type": type || "application/octet-stream" },
        body: buffer,
      },
    );
    return true;
  } catch (error) {
    if (!navigator.onLine || /nettverk|network|failed|fetch/i.test(error.message)) return false;
    throw error;
  }
}

/// Tømmer køen når nettet er tilbake. Kalles ved oppstart, når
/// browseren sier «online», og etter hver ny opplasting.
export async function koSend() {
  if (!navigator.onLine) return 0;
  let rader;
  try {
    rader = await koLes();
  } catch (e) {
    return 0;
  }
  let sendt = 0;
  for (const rad of rader) {
    try {
      if (await sendBilde(rad.company, rad.filnavn, rad.type, rad.buffer, rad.sha256)) {
        await koFjern(rad.id);
        sendt++;
      } else {
        break; // fortsatt uten dekning; resten venter
      }
    } catch (error) {
      // Serveren avviste bildet (f.eks. duplikat). Da hører det ikke
      // hjemme i køen lenger — det blir aldri bedre av å prøve igjen.
      await koFjern(rad.id);
    }
  }
  if (sendt) toast(sendt + " kvittering(er) sendt fra køen", true);
  return sendt;
}

/// Kvitteringsfoto: tar bildet, sender det, eller legger det i kø.
export async function lastOppKvittering(companyId, file) {
  const buffer = await file.arrayBuffer();
  const sha256 = await filHash(buffer);
  const filnavn =
    file.name && file.name !== "image.jpg"
      ? file.name
      : "kvittering-" + new Date().toISOString().slice(0, 19).replace(/[:T]/g, "") + ".jpg";
  const sendtNa = await sendBilde(companyId, filnavn, file.type, buffer, sha256);
  if (sendtNa) return "sendt";
  await koLegg({ company: companyId, filnavn, type: file.type, buffer, sha256 });
  return "kø";
}
