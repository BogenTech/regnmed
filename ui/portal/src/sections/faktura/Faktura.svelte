<script>
  // Faktura-seksjonen: fakturalisten er forsiden; en faktura, ny faktura
  // og fanene har hver sin adresse (#/c/{id}/faktura/{…}), så tilbake-
  // knappen, bokmerker og delte lenker virker. Hver fane henter bare
  // det den selv viser.
  import { api } from "../../lib/api.js";
  import InvoiceList from "./InvoiceList.svelte";
  import InvoiceDetail from "./InvoiceDetail.svelte";
  import NewInvoice from "./NewInvoice.svelte";
  import Salgsdok from "./Salgsdok.svelte";
  import Repeterende from "./Repeterende.svelte";
  import UfakturerteTimer from "./UfakturerteTimer.svelte";

  let { companyId, extra, query } = $props();

  const TABS = [
    ["", "Fakturaer"],
    ["tilbud", "Tilbud og ordre"],
    ["repeterende", "Repeterende"],
    ["timer", "Timer"],
  ];
  const isInvoiceId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");

  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);

  // Data for the secondary tabs, loaded when the tab opens. It carries
  // the tab it belongs to: the template re-renders on a tab switch
  // BEFORE any effect runs, and one tab's data read with another tab's
  // markup throws and freezes the component (docs/portal.md, #68).
  let loaded = $state(null); // { tab, ...data }
  async function loadTab(id, which) {
    const c = "/companies/" + id;
    if (which === "tilbud") {
      const [quotes, orders, parties, products] = await Promise.all([
        api(c + "/quotes"),
        api(c + "/orders"),
        api(c + "/parties?kind=kunde"),
        api(c + "/products").catch(() => ({ products: [] })),
      ]);
      return {
        quotes: quotes.documents,
        orders: orders.documents,
        parties: parties.parties,
        products: products.products.filter((p) => p.aktiv),
      };
    }
    if (which === "repeterende") {
      return { templates: (await api(c + "/invoice-templates")).templates };
    }
    if (which === "timer") {
      const [unbilled, parties] = await Promise.all([
        api(c + "/timesheet/unbilled"),
        api(c + "/parties?kind=kunde"),
      ]);
      return { unbilled: unbilled.groups, parties: parties.parties };
    }
    return null;
  }
  function reload() {
    const which = tab;
    const company = companyId;
    loadTab(companyId, which)
      .then((d) => tab === which && (loaded = { tab: which, company, ...d }))
      .catch((e) => tab === which && (loaded = { tab: which, company, error: e.message }));
  }
  $effect(() => {
    if (tab) reload();
  });
  let data = $derived(loaded?.tab === tab && loaded.company === companyId ? loaded : null);
</script>

{#if extra === "ny"}
  <NewInvoice {companyId} {query} />
{:else if isInvoiceId(extra)}
  {#key extra}
    <InvoiceDetail {companyId} invoiceId={extra} />
  {/key}
{:else if tab === null}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  <nav class="tabs tabs-box mb-4 w-fit max-w-full" aria-label="Faktura">
    {#each TABS as [key, label] (key)}
      <a
        class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
        href={"#/c/" + companyId + "/faktura" + (key ? "/" + key : "")}
        aria-current={tab === key ? "page" : undefined}
      >
        {label}
      </a>
    {/each}
  </nav>

  {#if tab === ""}
    <!-- A link to another filter (Oversikt's "forfalte fakturaer") arrives
         as a new query while the list may already be open. -->
    {#key companyId + "?" + query}
      <InvoiceList {companyId} {query} />
    {/key}
  {:else if !data}
    <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
  {:else if data.error}
    <div class="alert alert-warning" role="alert"><span>{data.error}</span></div>
  {:else if tab === "tilbud"}
    <Salgsdok {companyId} {...data} onDone={reload} />
  {:else if tab === "repeterende"}
    {#if data.templates.length}
      <Repeterende {companyId} templates={data.templates} onDone={reload} />
    {:else}
      <p class="opacity-70">
        Ingen repeterende fakturaer. Åpne en faktura og velg <b>Gjenta</b>, så lages den automatisk
        hver måned, kvartal eller år.
      </p>
    {/if}
  {:else if tab === "timer"}
    {#if data.unbilled.length}
      <UfakturerteTimer {companyId} unbilled={data.unbilled} kunder={data.parties} onDone={reload} />
    {:else}
      <p class="opacity-70">Ingen ufakturerte timer. Timer som føres som fakturerbare dukker opp her.</p>
    {/if}
  {/if}
{/if}
