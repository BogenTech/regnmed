<script>
  // Company-card purchases still waiting for their receipt: the lines of
  // the card's statement that nothing in the books explains yet. Pick the
  // photo straight from a line and it becomes a draft with the line's
  // date, amount and card — the cardholder only checks it and sends it in.
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");

  let lines = $state(null);
  let busy = $state(null);

  $effect(() => {
    api(base + "/card-lines")
      .then((r) => (lines = r.lines))
      .catch((err) => (lines = { error: err.message }));
  });

  async function picked(line, event) {
    const file = event.target.files[0];
    event.target.value = "";
    if (!file || busy) return;
    busy = line.bank_transaction_id;
    try {
      const r = await api(
        base + "/receipts?filename=" + encodeURIComponent(file.name) + "&bank_transaction_id=" + line.bank_transaction_id,
        { method: "POST", headers: { "content-type": file.type || "application/octet-stream" }, body: file },
      );
      location.hash = list + "/" + r.expense_id;
    } catch (err) {
      toast(err.message, false);
      busy = null;
    }
  }
</script>

<nav class="mb-3"><a class="link link-hover text-sm" href={list}>← Alle utlegg</a></nav>
<h1 class="text-2xl font-bold mb-2">Kortkjøp uten kvittering</h1>
<p class="opacity-70 mb-4 max-w-2xl">
  Disse kjøpene står på kontoutskriften for firmakortet, men ingen kvittering er levert. Ta bilde av kvitteringen
  ved kjøpet, så fylles dato, beløp og kort inn fra kontoutskriften.
</p>

{#if lines === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
{:else if lines.error}
  <div class="alert alert-error" role="alert"><span>{lines.error}</span></div>
{:else if !lines.length}
  <div class="alert alert-success alert-soft" role="status"><span>Alle kortkjøp har kvittering.</span></div>
{:else}
  <ul class="grid gap-2 max-w-3xl">
    {#each lines as l (l.bank_transaction_id)}
      <li class="card border border-base-300 bg-base-100">
        <div class="card-body p-4 flex-row flex-wrap items-center gap-3">
          <div class="mr-auto min-w-0">
            <p class="font-medium break-words">{l.beskrivelse || "Kortkjøp"}</p>
            <p class="text-sm opacity-70">{formatDate(l.dato)} · {l.card}</p>
          </div>
          <span class="tabular-nums font-semibold whitespace-nowrap">{kr(l.belop_ore)} kr</span>
          <label class="btn btn-sm btn-primary focus-within:outline-2 focus-within:outline-offset-2">
            {#if busy === l.bank_transaction_id}<span class="loading loading-spinner loading-sm"></span>{/if}
            Legg ved kvittering
            <span class="sr-only">for {l.beskrivelse} {formatDate(l.dato)}, {kr(l.belop_ore)} kr</span>
            <input type="file" class="sr-only" accept="image/*,application/pdf" disabled={!!busy} onchange={(ev) => picked(l, ev)} />
          </label>
        </div>
      </li>
    {/each}
  </ul>
{/if}
