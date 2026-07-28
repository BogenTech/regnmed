// Nedlasting av dokumenter (PDF/EHF/tekst) via API-klienten — samme
// blob-og-lenke-triks som app.js, feil ender i toast.

import { api } from "./api.js";
import { toast } from "./toast.svelte.js";

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
