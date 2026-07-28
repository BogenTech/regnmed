<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Produktregister from "./Produktregister.svelte";
  import Varelager from "./Varelager.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [products, inventory] = await Promise.all([
      api("/companies/" + id + "/products"),
      api("/companies/" + id + "/inventory"),
    ]);
    data = {
      products: products.products,
      inventory: inventory.inventory,
      verdiOre: inventory.verdi_ore,
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
  <Produktregister {companyId} products={data.products} onDone={reload} />

  <!-- Varelageret vises bare når noe faktisk er lagerført. -->
  {#if data.products.some((p) => p.lagerfort)}
    <Varelager
      {companyId}
      products={data.products}
      inventory={data.inventory}
      verdiOre={data.verdiOre}
      onDone={reload}
    />
  {/if}
{/if}
