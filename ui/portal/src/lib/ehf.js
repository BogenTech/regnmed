// What an EHF dispatch means to the user (docs/efaktura.md §3.3). The server
// computes the status; this only turns it into text.

const STATUS = {
  sendeforsok: { text: "Sendes nå", tone: "info" },
  ikke_sendt: { text: "Ikke sendt", tone: "error" },
  uavklart: { text: "Ukjent om den kom fram", tone: "warning" },
  akseptert: { text: "Sendt, venter på kvittering", tone: "info" },
  leveringsfeil: { text: "Levering feilet", tone: "error" },
  avvist: { text: "Avvist", tone: "error" },
  levert: { text: "Levert", tone: "success" },
};

export function dispatchStatus(status) {
  return STATUS[status] ?? { text: "Ukjent status", tone: "warning" };
}

// The toast after a finished send job. Only what actually went out is ok.
export function resultMessage(result) {
  if (result.kanal === "epost") {
    return { text: result.to ? "Sendt til " + result.to : "Sendt på e-post", ok: true };
  }
  const to = result.mottaker;
  const detail = result.detalj ? ": " + result.detalj : "";
  switch (result.status) {
    case "levert":
      return { text: "Levert som EHF til " + to, ok: true };
    case "akseptert":
      return { text: "Sendt som EHF til " + to, ok: true };
    case "uavklart":
    case "sendeforsok":
      return {
        text: "Vi vet ikke ennå om EHF-en kom fram. Ikke send på nytt, vi sjekker det automatisk.",
        ok: false,
      };
    case "leveringsfeil":
      return { text: "Leveringen feilet" + detail, ok: false };
    case "avvist":
      return { text: "EHF-en ble avvist" + detail, ok: false };
    default:
      return { text: "EHF-en ble ikke sendt" + detail, ok: false };
  }
}

// Only a plain https address may become a link in the portal.
export function safeLink(url) {
  try {
    return new URL(url).protocol === "https:" ? url : null;
  } catch {
    return null;
  }
}

// Keeps a channel check that started before the click, so the dialog can open
// at once. It is display only: the worker looks the buyer up again at send.
export function createRouteCache({ ttlMs = 30_000, now = Date.now } = {}) {
  const entries = new Map();
  return {
    get(key, load) {
      const hit = entries.get(key);
      if (hit && now() - hit.at < ttlMs) return hit.route;
      const entry = { at: now(), route: null };
      entry.route = load().catch((error) => {
        if (entries.get(key) === entry) entries.delete(key);
        throw error;
      });
      for (const [k, old] of entries) {
        if (now() - old.at >= ttlMs) entries.delete(k);
      }
      entries.set(key, entry);
      return entry.route;
    },
    clear(key) {
      entries.delete(key);
    },
  };
}
