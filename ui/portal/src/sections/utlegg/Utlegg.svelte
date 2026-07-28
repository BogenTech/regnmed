<script>
  // Utlegg og kjøregodtgjørelse (#42): refusjonskrav med innboksens
  // disiplin — kvitteringen er uforanderlig fra innsending, og statusen
  // går bare én vei: innsendt → godkjent/avvist → utbetalt.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import NyttUtlegg from "./NyttUtlegg.svelte";
  import Kjoregodtgjorelse from "./Kjoregodtgjorelse.svelte";
  import KravListe from "./KravListe.svelte";

  let { companyId, extra } = $props();

  let data = $state(null);

  async function load(id) {
    const expenses = await api("/companies/" + id + "/expenses");
    data = { expenses: expenses.expenses };
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
  <NyttUtlegg {companyId} onDone={reload} />
  <Kjoregodtgjorelse {companyId} onDone={reload} />
  <KravListe {companyId} expenses={data.expenses} onDone={reload} />
{/if}
