// Nedlasting av dokumenter (PDF/EHF/tekst) via API-klienten — samme
// blob-og-lenke-triks som app.js, feil ender i toast.

import { api } from "./api.js";
import { toast } from "./toast.svelte.js";

/// Lagrer innhold vi allerede har (f.eks. XML fra et JSON-svar, eller
/// bytene i et zip-arkiv) som fil.
export function saveText(text, filename, type = "application/xml") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

export async function download(path, filename) {
  try {
    const response = await api(path);
    const blob = await response.blob();
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    URL.revokeObjectURL(a.href);
  } catch (error) {
    toast(error.message, false);
  }
}
