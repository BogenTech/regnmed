<script>
  // Purring (#29): alltid en eksplisitt menneskelig handling —
  // forhåndsvis kravet (gebyrtak og rente hentes fra satsregisteret),
  // så registrer. Enveis trapp; serveren håndhever reglene.
  import { untrack } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import { sendDocument } from "../../lib/utsendelse.js";

  let { companyId, invoiceId, suggestedSteg, onDone } = $props();

  let base = $derived("/companies/" + companyId + "/invoices/" + invoiceId + "/reminders");

  let history = $state(null);
  let steg = $state(untrack(() => suggestedSteg));
  let frist = $state(new Date(Date.now() + 14 * 86400000).toISOString().slice(0, 10));
  let gebyr = $state(false);
  let rente = $state(false);
  let naering = $state(false);
  let previewed = $state(null);

  $effect(() => {
    api(base)
      .then((svar) => (history = svar.reminders))
      .catch(() => (history = []));
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
        created.steg + " registrert" + (created.voucher ? " (bilag " + created.voucher + ")" : ""),
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<div class="card border border-base-300 card-sm mt-3">
  <div class="card-body">
    {#if history?.length}
      <p class="text-sm font-semibold mb-1">Purrehistorikk</p>
      <table class="table table-xs mb-3">
        <thead>
          <tr>
            <th>Skritt</th><th>Sendt</th><th>Frist</th>
            <th class="text-right">Gebyr+rente</th><th>Bilag</th><th></th>
          </tr>
        </thead>
        <tbody>
          {#each history as r (r.reminder_id)}
            <tr>
              <td>{r.steg}</td>
              <td>{r.sent_date}</td>
              <td>{r.frist_date}</td>
              <td class="text-right">{kr(r.gebyr_ore + r.rente_ore)}</td>
              <td>{r.voucher || "–"}</td>
              <td>
                <button
                  class="link text-xs"
                  onclick={() => download(base + "/" + r.reminder_id + "?format=tekst", "purring.txt")}
                >
                  tekst
                </button>
                <button
                  class="link text-xs"
                  onclick={() => download(base + "/" + r.reminder_id + "?format=pdf", "purring.pdf")}
                >
                  pdf
                </button>
                <button
                  class="link text-xs"
                  onclick={() => sendDocument(base + "/" + r.reminder_id + "/send")}
                >
                  send
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <div class="grid gap-2 max-w-md">
      <label class="fieldset">
        <span class="fieldset-legend">Skritt</span>
        <select class="select select-sm" bind:value={steg}>
          <option value="paminnelse">Betalingspåminnelse (gebyrfri)</option>
          <option value="purring">Purring</option>
          <option value="inkassovarsel">Inkassovarsel (14 dagers frist)</option>
        </select>
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">Betalingsfrist</span>
        <input type="date" class="input input-sm" bind:value={frist} />
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
          Registrer
        </button>
      </div>
      {#if previewed}
        <div>
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
          <pre class="bg-base-200 rounded p-2 text-xs overflow-x-auto mt-2">{previewed.document}</pre>
        </div>
      {/if}
    </div>
  </div>
</div>
