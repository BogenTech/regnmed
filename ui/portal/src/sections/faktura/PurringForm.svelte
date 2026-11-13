<script>
  // Purring (#29): alltid en eksplisitt menneskelig handling —
  // forhåndsvis kravet (gebyrtak og rente hentes fra satsregisteret),
  // så registrer. Enveis trapp; serveren håndhever reglene.
  import { untrack } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import { sendDocument } from "../../lib/utsendelse.js";

  let { companyId, invoiceId, suggestedSteg = null, onDone } = $props();

  // Without a suggestion, the next step after the last one sent.
  const NEXT_STEP = { paminnelse: "purring", purring: "inkassovarsel", inkassovarsel: "inkassovarsel" };
  const STEP_LABEL = { paminnelse: "Betalingspåminnelse", purring: "Purring", inkassovarsel: "Inkassovarsel" };

  let base = $derived("/companies/" + companyId + "/invoices/" + invoiceId + "/reminders");

  let history = $state(null);
  let steg = $state(untrack(() => suggestedSteg || "paminnelse"));
  let frist = $state(new Date(Date.now() + 14 * 86400000).toISOString().slice(0, 10));
  let gebyr = $state(false);
  let rente = $state(false);
  let naering = $state(false);
  let previewed = $state(null);
  // The reminder just registered, offered for sending as the next step.
  let registered = $state(null);

  function loadHistory() {
    api(base)
      .then((svar) => {
        history = svar.reminders;
        const last = history.at(-1)?.steg;
        if (!untrack(() => suggestedSteg) && last) steg = NEXT_STEP[last] || steg;
      })
      .catch(() => (history = []));
  }
  $effect(loadHistory);

  // What is registered must be what was previewed: any change to the
  // choices throws the preview away.
  $effect(() => {
    void [steg, frist, gebyr, rente, naering];
    untrack(() => (previewed = null));
  });

  function body(gebyrOre) {
    return {
      steg,
      frist_date: frist,
      gebyr_ore: gebyr ? gebyrOre : 0,
      med_rente: rente,
      naeringsdrivende: naering,
    };
  }

  async function forhandsvis() {
    try {
      // Første pass henter gjeldende maks-sats, andre forhåndsviser med den.
      const probe = await post(base + "?preview=true", body(0));
      previewed = await post(base + "?preview=true", body(probe.maks_gebyr_ore));
    } catch (error) {
      previewed = null;
      toast(error.message, false);
    }
  }

  async function registrer() {
    if (!previewed) return;
    try {
      const created = await post(base, body(previewed.gebyr_ore));
      toast(
        (STEP_LABEL[created.steg] || created.steg) + " registrert" + (created.voucher ? " (bilag " + created.voucher + ")" : ""),
        true,
      );
      previewed = null;
      registered = created;
      loadHistory();
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function sendRegistered() {
    if (await sendDocument(base + "/" + registered.reminder_id + "/send")) {
      registered = null;
      onDone();
    }
  }
</script>

<div class="card border border-base-300 card-sm mt-3 min-w-0">
  <div class="card-body min-w-0">
    {#if registered}
      <div class="alert alert-success alert-soft" role="status">
        <span>{STEP_LABEL[registered.steg] || registered.steg} er registrert. Send den til kunden nå?</span>
        <div class="flex gap-2">
          <button class="btn btn-sm btn-primary" onclick={sendRegistered}>Send til kunde</button>
          <button class="btn btn-sm btn-ghost" onclick={() => (registered = null)}>Senere</button>
        </div>
      </div>
    {/if}
    {#if history?.length}
      <h3 class="text-sm font-semibold mb-1">Purrehistorikk</h3>
      <div class="overflow-x-auto">
        <table class="table table-sm mb-3">
          <thead>
            <tr>
              <th scope="col">Skritt</th><th scope="col">Sendt</th><th scope="col">Frist</th>
              <th scope="col" class="text-right">Gebyr og rente</th><th scope="col">Bilag</th><th scope="col"><span class="sr-only">Handlinger</span></th>
            </tr>
          </thead>
          <tbody>
            {#each history as r (r.reminder_id)}
              {@const label = STEP_LABEL[r.steg] || r.steg}
              <tr>
                <td>{label}</td>
                <td class="whitespace-nowrap">{formatDate(r.sent_date)}</td>
                <td class="whitespace-nowrap">{formatDate(r.frist_date)}</td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(r.gebyr_ore + r.rente_ore)}</td>
                <td>{r.voucher || "–"}</td>
                <td class="whitespace-nowrap">
                  <button
                    class="btn btn-xs btn-ghost"
                    aria-label={"Last ned " + label.toLowerCase() + " " + formatDate(r.sent_date) + " som PDF"}
                    onclick={() => download(base + "/" + r.reminder_id + "?format=pdf", "purring.pdf")}
                  >
                    PDF
                  </button>
                  <button
                    class="btn btn-xs btn-ghost"
                    aria-label={"Last ned " + label.toLowerCase() + " " + formatDate(r.sent_date) + " som tekst"}
                    onclick={() => download(base + "/" + r.reminder_id + "?format=tekst", "purring.txt")}
                  >
                    Tekst
                  </button>
                  <button
                    class="btn btn-xs btn-ghost"
                    aria-label={"Send " + label.toLowerCase() + " " + formatDate(r.sent_date) + " til kunden"}
                    onclick={() => sendDocument(base + "/" + r.reminder_id + "/send")}
                  >
                    Send
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
    <div class="grid gap-2 max-w-md min-w-0">
      <label class="fieldset">
        <span class="fieldset-legend">Skritt</span>
        <select class="select select-sm w-full" bind:value={steg}>
          <option value="paminnelse">Betalingspåminnelse (gebyrfri)</option>
          <option value="purring">Purring</option>
          <option value="inkassovarsel">Inkassovarsel (14 dagers frist)</option>
        </select>
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">Betalingsfrist</span>
        <input type="date" class="input input-sm w-full" bind:value={frist} />
      </label>
      <label class="label cursor-pointer justify-start gap-2">
        <input type="checkbox" class="checkbox checkbox-sm" bind:checked={gebyr} />
        <span>Purregebyr (maks-sats)</span>
      </label>
      <label class="label cursor-pointer justify-start gap-2">
        <input type="checkbox" class="checkbox checkbox-sm" bind:checked={rente} />
        <span>Krev forsinkelsesrente</span>
      </label>
      <label class="label cursor-pointer justify-start gap-2">
        <input type="checkbox" class="checkbox checkbox-sm" bind:checked={naering} />
        <span>Næringsdrivende skyldner (standardkompensasjon)</span>
      </label>
      <div class="flex gap-2">
        <button class="btn btn-sm" onclick={forhandsvis}>Forhåndsvis</button>
        <button class="btn btn-sm btn-primary" disabled={!previewed} onclick={registrer}>
          Registrer {STEP_LABEL[steg]?.toLowerCase() || "purringen"}
        </button>
      </div>
      {#if !previewed}
        <p class="text-xs opacity-60">Forhåndsvis først — det er teksten og beløpet du ser, som registreres.</p>
      {:else}
        <div class="min-w-0" aria-live="polite">
          <p class="text-sm mt-2">
            Å betale: <b>{kr(previewed.total_ore)}</b>
            {#if previewed.gebyr_ore || previewed.rente_ore}
              {" (" +
                [
                  previewed.gebyr_ore ? "gebyr " + kr(previewed.gebyr_ore) : null,
                  previewed.rente_ore ? "rente " + kr(previewed.rente_ore) : null,
                ]
                  .filter(Boolean)
                  .join(", ") +
                ")"}
            {/if}
          </p>
          <pre class="bg-base-200 rounded p-2 text-xs overflow-x-auto max-w-full mt-2">{previewed.document}</pre>
        </div>
      {/if}
    </div>
  </div>
</div>
