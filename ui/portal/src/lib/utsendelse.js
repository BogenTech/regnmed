// Utsendelse: POST til send-endepunktet; mangler parten e-postadresse,
// spør vi om en og prøver igjen med den. Sending er alltid en eksplisitt
// menneskelig handling (docs/faktura.md).

import { post } from "./api.js";
import { toast } from "./toast.svelte.js";
import { sporsmal } from "./dialog.svelte.js";

export async function sendDocument(path) {
  const attempt = (email) => post(path, email ? { email } : {});
  try {
    const sent = await attempt(null);
    toast("Sendt til " + sent.to, true);
  } catch (error) {
    if (String(error.message).includes("e-postadresse")) {
      const email = await sporsmal("Kundens e-postadresse:", { type: "email", ok: "Send" });
      if (!email) return;
      try {
        const retried = await attempt(email);
        toast("Sendt til " + retried.to, true);
      } catch (retryError) {
        toast(retryError.message, false);
      }
    } else {
      toast(error.message, false);
    }
  }
}
