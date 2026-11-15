<script>
  // Bevegelseslisten for ett produkt, åpnet under raden — salgslinjene
  // bærer fakturaen de kom fra, så beholdningen alltid kan spores
  // tilbake til et bilag.
  import { onMount } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, antallStr, formatDate } from "../../lib/format.js";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, nummer } = $props();

  const KIND_NAVN = { kjop: "Varekjøp", salg: "Salg", justering: "Justering" };

  let movements = $state(null); // null = loading, { error } = failed

  async function last() {
    try {
      const svar = await api(
        "/companies/" + companyId + "/inventory/movements?produkt=" + encodeURIComponent(nummer),
      );
      movements = svar.movements;
    } catch (error) {
      movements = { error: error.message };
    }
  }
  onMount(last);
</script>

<div class="mt-2 p-3 rounded-box bg-base-200 text-sm">
  {#if !movements}
    <span class="loading loading-spinner loading-sm" aria-label="Laster bevegelser"></span>
  {:else if movements.error}
    <LoadError error={movements.error} retry={() => ((movements = null), last())} />
  {:else if !movements.length}
    <p class="opacity-70">Ingen bevegelser ennå.</p>
  {:else}
    <ul class="divide-y divide-base-300">
      {#each movements as m, i (i)}
        <li class="py-1 flex gap-2">
          <span class="whitespace-nowrap w-20 shrink-0">{formatDate(m.dato)}</span>
          <span class="min-w-0 flex-1">
            {KIND_NAVN[m.kind] || m.kind}
            {#if m.invoice_no}<span class="opacity-60">faktura {m.invoice_no}</span>{/if}
            {#if m.kostpris_ore != null}
              <span class="opacity-60">à {kr(m.kostpris_ore)} kr</span>
            {/if}
            {#if m.note}<span class="block text-xs opacity-60">{m.note}</span>{/if}
          </span>
          <span class="text-right whitespace-nowrap">
            {m.antall_milli > 0 ? "+" : ""}{antallStr(m.antall_milli)}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
