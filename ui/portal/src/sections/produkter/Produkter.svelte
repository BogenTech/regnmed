<script>
  // Produkter og varelager (#39). The register and the stock are two
  // rights (PRODUKT_* and LAGER_*), so they load apart: someone who may
  // see products but not stock still gets the register, and a failed
  // load says so in place with a retry instead of spinning.
  import { api } from "../../lib/api.js";
  import { harRett } from "../../lib/me.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Produktregister from "./Produktregister.svelte";
  import Varelager from "./Varelager.svelte";

  let { companyId } = $props();

  let kanSkrive = $derived(harRett(companyId, "PRODUKT_SKRIV"));
  let serLager = $derived(harRett(companyId, "LAGER_LES"));
  let kanLager = $derived(harRett(companyId, "LAGER_SKRIV"));

  // null = loading, { error } = failed, otherwise the data.
  let products = $state(null);
  let lager = $state(null);
  // The account picker's lists; only for someone who may read the
  // chart of accounts (BILAG_LES) — without them the field is plain.
  let kontoer = $state({ kontoer: [], standard: [] });

  async function lastProdukter(id = companyId) {
    try {
      const svar = await api("/companies/" + id + "/products");
      if (id === companyId) products = svar.products;
    } catch (error) {
      if (id === companyId) products = { error: error.message };
    }
  }

  async function lastLager(id = companyId) {
    try {
      const svar = await api("/companies/" + id + "/inventory");
      if (id === companyId) lager = { rader: svar.inventory, verdiOre: svar.verdi_ore };
    } catch (error) {
      if (id === companyId) lager = { error: error.message };
    }
  }

  $effect(() => {
    const id = companyId;
    products = lager = null;
    lastProdukter(id);
    if (serLager) lastLager(id);
    if (kanSkrive && harRett(id, "BILAG_LES"))
      api("/companies/" + id + "/accounts")
        .then((s) => {
          if (id !== companyId) return;
          const inntekt = (k) => /^3\d{3}$/.test(k.number);
          kontoer = {
            kontoer: (s.kontoer || []).filter(inntekt),
            standard: (s.standard || []).filter(inntekt),
          };
        })
        .catch(() => {});
  });

  // A product edit can change what is stocked, so the stock follows.
  // Returned, so callers can move focus once the list has its new shape.
  function produktEndret() {
    return Promise.all([lastProdukter(), serLager && lastLager()]);
  }

  let stocked = $derived(Array.isArray(products) && products.some((p) => p.lagerfort));
</script>

{#if !products}
  <span class="loading loading-spinner loading-lg" aria-label="Laster"></span>
{:else if products.error}
  <LoadError error={products.error} retry={() => ((products = null), lastProdukter())} />
{:else}
  <Produktregister {companyId} {products} {kanSkrive} {kontoer} onChanged={produktEndret} />

  <!-- Varelageret vises bare når noe faktisk er lagerført. -->
  {#if serLager && stocked}
    {#if !lager}
      <span class="loading loading-spinner" aria-label="Laster varelageret"></span>
    {:else if lager.error}
      <LoadError error={lager.error} retry={() => ((lager = null), lastLager())} />
    {:else}
      <Varelager
        {companyId}
        {products}
        inventory={lager.rader}
        verdiOre={lager.verdiOre}
        kanSkrive={kanLager}
        onChanged={() => lastLager()}
      />
    {/if}
  {/if}
{/if}
