<script>
  // One asset's depreciation months, newest first: the voucher it was
  // posted on, or why it was not. A month caught up after a period lock
  // shows the date it was actually posted.
  import { onMount } from "svelte";
  import { api } from "../../lib/api.js";
  import { formatDate, kr } from "../../lib/format.js";
  import LoadError from "../../components/LoadError.svelte";
  import { maned } from "./anlegg.js";

  let { companyId, asset, onClose } = $props();

  let runs = $state(null); // null = loading, { error } = failed
  let panel;

  async function last() {
    runs = null;
    try {
      runs = (await api("/companies/" + companyId + "/assets/" + asset.asset_id + "/runs")).runs;
    } catch (error) {
      runs = { error: error.message };
    }
  }

  onMount(() => {
    last();
    panel?.focus();
  });

  // Month-end of the period: when the voucher date differs, it was
  // caught up.
  const innhentet = (r) => {
    const [y, m] = r.period.split("-").map(Number);
    const slutt = new Date(Date.UTC(y, m, 0)).toISOString().slice(0, 10);
    return r.bilagsdato && r.bilagsdato !== slutt;
  };
</script>

<!-- Esc closes the panel from any field inside it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={panel}
  tabindex="-1"
  role="region"
  aria-label={"Avskrivninger for " + asset.navn}
  class="mt-2 p-3 rounded-box bg-base-200 text-sm"
  onkeydown={(e) => e.key === "Escape" && onClose()}
>
  {#if !runs}
    <span class="loading loading-spinner loading-sm" aria-label="Laster avskrivningene"></span>
  {:else if runs.error}
    <LoadError error={runs.error} retry={last} />
  {:else if !runs.length}
    <p class="opacity-70">Ingen avskrivninger bokført ennå.</p>
  {:else}
    <ul class="divide-y divide-base-300">
      {#each runs as r (r.period)}
        <li class="py-1 flex flex-wrap gap-x-3">
          <span class="w-16 tabular-nums">{maned(r.period)}</span>
          <span class="w-28 text-right tabular-nums">{kr(r.amount_ore)} kr</span>
          {#if r.voucher}
            <span class="opacity-70">
              bilag {r.voucher}{#if innhentet(r)}, bokført {formatDate(r.bilagsdato)} (perioden var låst){/if}
            </span>
          {:else}
            <span class="text-error basis-full sm:basis-auto">Ikke bokført: {r.detail}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <button class="btn btn-xs btn-ghost mt-2" onclick={onClose}>Lukk</button>
</div>
