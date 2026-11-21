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
