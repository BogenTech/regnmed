<script>
  // Anleggsregister og avskrivninger (#40). Saldorapporten er
  // skattemessig og kan mangle satsdekning for året — da svarer
  // endepunktet med feil, og registeret vises likevel.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Anleggsregister from "./Anleggsregister.svelte";
  import Saldo from "./Saldo.svelte";

  let { companyId, extra } = $props();

  let data = $state(null);

  async function load(id) {
    const year = new Date().getFullYear();
    const [assets, saldo] = await Promise.all([
      api("/companies/" + id + "/assets"),
      api("/companies/" + id + "/assets/saldo?year=" + year).catch(() => null),
    ]);
    data = { assets: assets.assets, saldo };
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
  <Anleggsregister {companyId} assets={data.assets} onDone={reload} />
  {#if data.saldo && data.saldo.grupper.length}
    <Saldo saldo={data.saldo} />
  {/if}
{/if}
