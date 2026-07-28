<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Betalingsliste from "./Betalingsliste.svelte";
  import Kontoutskrift from "./Kontoutskrift.svelte";
  import Avstemming from "./Avstemming.svelte";

  let { companyId } = $props();

  // Bankkontoen avstemmingen gjelder — én konto, som i dagens portal.
  const ACCOUNT = "1920";

  let data = $state(null);

  async function load(id) {
    const [recon, payable, runs] = await Promise.all([
      // Ingen importerte kontoutskrifter er en normal tilstand, ikke en
      // feil: da vises avstemmingskortet tomt i stedet for å forsvinne.
      api("/companies/" + id + "/bank/reconciliation?account=" + ACCOUNT).catch(() => null),
      api("/companies/" + id + "/payments/payable").catch(() => ({ items: [] })),
      api("/companies/" + id + "/payments/runs").catch(() => ({ runs: [] })),
    ]);
    data = { recon, payable: payable.items, runs: runs.runs };
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
  {#if data.payable.length || data.runs.length}
    <Betalingsliste {companyId} payable={data.payable} runs={data.runs} onDone={reload} />
  {/if}

  <Kontoutskrift {companyId} account={ACCOUNT} onDone={reload} />

  <Avstemming {companyId} recon={data.recon} onDone={reload} />
{/if}
