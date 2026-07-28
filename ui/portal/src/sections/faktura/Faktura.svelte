<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Forfalte from "./Forfalte.svelte";
  import Salgsdok from "./Salgsdok.svelte";
  import Repeterende from "./Repeterende.svelte";
  import NyFaktura from "./NyFaktura.svelte";
  import FakturaListe from "./FakturaListe.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [invoices, parties, overdue, dims, templates, quotes, orders, products, rates] =
      await Promise.all([
        api("/companies/" + id + "/invoices"),
        api("/companies/" + id + "/parties?kind=kunde"),
        api("/companies/" + id + "/invoices/overdue").catch(() => ({ invoices: [], buckets: {} })),
        api("/companies/" + id + "/dimensions").catch(() => ({ dimensions: [] })),
        api("/companies/" + id + "/invoice-templates").catch(() => ({ templates: [] })),
        api("/companies/" + id + "/quotes").catch(() => ({ documents: [] })),
        api("/companies/" + id + "/orders").catch(() => ({ documents: [] })),
        api("/companies/" + id + "/products").catch(() => ({ products: [] })),
        api("/companies/" + id + "/currency/rates").catch(() => ({ rates: [] })),
      ]);
    data = {
      invoices: invoices.invoices,
      parties: parties.parties,
      overdue,
      dims: dims.dimensions,
      templates: templates.templates,
      quotes: quotes.documents,
      orders: orders.documents,
      products: products.products.filter((p) => p.aktiv),
      currencies: rates.rates.map((r) => r.valuta),
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  {#if data.overdue.invoices.length}
    <Forfalte {companyId} overdue={data.overdue} onDone={reload} />
  {/if}

  {#if data.quotes.length || data.orders.length || data.parties.length}
    <Salgsdok
      {companyId}
      quotes={data.quotes}
      orders={data.orders}
      parties={data.parties}
      products={data.products}
      onDone={reload}
    />
  {/if}

  {#if data.templates.length}
    <Repeterende {companyId} templates={data.templates} onDone={reload} />
  {/if}

  <NyFaktura
    {companyId}
    parties={data.parties}
    products={data.products}
    dims={data.dims}
    currencies={data.currencies}
    onDone={reload}
  />

  <FakturaListe {companyId} invoices={data.invoices} onDone={reload} />
{/if}
