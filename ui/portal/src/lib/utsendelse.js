// Utsendelse: POST til send-endepunktet; mangler parten e-postadresse,
// spør vi om en og prøver igjen med den. Sending er alltid en eksplisitt
// menneskelig handling (docs/faktura.md).

import { post } from "./api.js";
import { toast } from "./toast.svelte.js";
import { bekreft, sporsmal } from "./dialog.svelte.js";
import { resultMessage } from "./ehf.js";
import { waitForJob } from "./jobs.js";

export async function sendDocument(path) {
  const attempt = (email) => post(path, email ? { email } : {});
  try {
    const sent = await attempt(null);
    toast("Sendt til " + sent.to, true);
    return sent;
  } catch (error) {
    if (String(error.message).includes("e-postadresse")) {
      const email = await sporsmal("Kundens e-postadresse:", { type: "email", ok: "Send" });
      if (!email) return;
      try {
        const retried = await attempt(email);
        toast("Sendt til " + retried.to, true);
        return retried;
      } catch (retryError) {
        toast(retryError.message, false);
      }
    } else {
      toast(error.message, false);
    }
  }
}

// A worker sends the invoice and chooses the channel (docs/efaktura.md §3.1).
// The user sees the channel before the click that actually sends.
export async function sendInvoice(companyId, base, { resend = false } = {}) {
  const run = async (body) => {
    const started = await post(base + "/send", body);
    return waitForJob(companyId, started.job_id);
  };
  const report = (result) => {
    const { text, ok } = resultMessage(result);
    toast(text, ok);
    return result;
  };
  try {
    const check = await post(base + "/delivery-check", {});
    const route = await waitForJob(companyId, check.job_id);
    if (route.kanal === "ukjent") {
      toast(route.grunn, false);
      return;
    }
    const melding =
      route.kanal === "peppol"
        ? "Fakturaen sendes som EHF til " + route.mottaker + "."
        : route.grunn + " Fakturaen sendes som PDF på e-post.";
    const warning = resend ? " Det er en liten risiko for at kjøperen får den to ganger." : "";
    const reference = route.advarsel ? " " + route.advarsel : "";
    const ok = await bekreft(melding + reference + warning, {
      tittel: resend ? "Send på nytt" : "Send faktura",
      ok: "Send",
    });
    if (!ok) return;
    const body = resend ? { resend_unresolved: true } : {};
    try {
      return report(await run(body));
    } catch (error) {
      if (!String(error.message).includes("e-postadresse")) throw error;
      const email = await sporsmal("Kundens e-postadresse:", { type: "email", ok: "Send" });
      if (!email) return;
      return report(await run({ ...body, email }));
    }
  } catch (error) {
    toast(error.message, false);
  }
}
