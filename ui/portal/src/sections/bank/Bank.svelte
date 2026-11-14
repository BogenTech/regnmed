<script>
  // Bank: reconciliation is the front page — what the bank shows that
  // the books do not, and the other way round. Payments (the payment
  // list and its pain.001 file) and exchange rates have their own tabs
  // and addresses (#/c/{id}/bank/{tab}).
  import Reconcile from "./Reconcile.svelte";
  import Payments from "./Payments.svelte";
  import Rates from "./Rates.svelte";
  import Cards from "./Cards.svelte";

  let { companyId, extra } = $props();

  const TABS = [
    ["", "Avstemming"],
    ["betalinger", "Betalinger"],
    ["valuta", "Valutakurser"],
    ["kort", "Firmakort"],
  ];
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
</script>

<h1 class="text-2xl font-bold mb-4">Bank</h1>
{#if tab === null}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  <nav class="tabs tabs-box mb-4 w-fit max-w-full" aria-label="Bank">
    {#each TABS as [key, label] (key)}
      <a
        class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
        href={"#/c/" + companyId + "/bank" + (key ? "/" + key : "")}
        aria-current={tab === key ? "page" : undefined}
      >
        {label}
      </a>
    {/each}
  </nav>
  {#key companyId + tab}
    {#if tab === ""}
      <Reconcile {companyId} />
    {:else if tab === "betalinger"}
      <Payments {companyId} />
    {:else if tab === "kort"}
      <Cards {companyId} />
    {:else}
      <Rates {companyId} />
    {/if}
  {/key}
{/if}
