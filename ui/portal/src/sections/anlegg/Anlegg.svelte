<script>
  // Anleggsregister og avskrivninger (#40). The register and the tax
  // depreciation load on their own, so a year the satsregister does not
  // cover never hides the register — and it says so instead of the card
  // quietly vanishing.
  import { api } from "../../lib/api.js";
  import { harRett } from "../../lib/me.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Anleggsregister from "./Anleggsregister.svelte";
  import Saldo from "./Saldo.svelte";

  let { companyId } = $props();

  let kanSkrive = $derived(harRett(companyId, "ANLEGG_SKRIV"));
  // null = loading, { error } = failed, otherwise the list.
  let assets = $state(null);
  let kontoer = $state({ kontoer: [], standard: [] });
  // Bumped after any change, so the tax depreciation follows.
  let versjon = $state(0);

  function lastAssets() {
    const id = companyId;
    return api("/companies/" + id + "/assets")
      .then((s) => id === companyId && (assets = s.assets))
      .catch((error) => id === companyId && (assets = { error: error.message }));
  }

  $effect(() => {
    const id = companyId;
    assets = null;
    lastAssets();
    kontoer = { kontoer: [], standard: [] };
    if (kanSkrive && harRett(id, "BILAG_LES"))
      api("/companies/" + id + "/accounts")
        .then(
          (s) =>
            id === companyId && (kontoer = { kontoer: s.kontoer || [], standard: s.standard || [] }),
        )
        .catch(() => {});
  });

  async function endret() {
    await lastAssets();
    versjon++;
  }
</script>

{#if Array.isArray(assets)}
  <Anleggsregister {companyId} {assets} {kanSkrive} {kontoer} onChanged={endret} />
{:else if assets}
  <LoadError error={assets.error} retry={() => ((assets = null), lastAssets())} />
{:else}
  <span class="loading loading-spinner" aria-label="Laster anleggsregisteret"></span>
{/if}

<Saldo {companyId} {versjon} />
