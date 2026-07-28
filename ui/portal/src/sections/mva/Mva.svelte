<script>
  // Mva-seksjonen følger selskapets TERMINORDNING (#51): ordningen
  // Skatteetaten har innvilget avgjør hvilke perioder som finnes,
  // hvilken spesifikasjon som vises og hvilken leveringsfrist som
  // gjelder. Perioder utenfor ordningen avvises av serveren — velgeren
  // tilbyr dem derfor ikke. Systemet vurderer aldri berettigelse.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Spesifikasjon from "./Spesifikasjon.svelte";
  import Eksport from "./Eksport.svelte";
  import Terminordning from "./Terminordning.svelte";

  // År og termin ligger i hash-spørringen (#/c/{id}/mva?year=&termin=),
  // slik at en lenke til en bestemt termin kan deles. Ruteren leser den.
  let { companyId, query = "" } = $props();

  let data = $state(null);

  // Sekvensielt, ikke Promise.all: hvilken termin som spørres etter
  // avhenger av ordningen.
  async function load(id, q) {
    const ordningInfo = await api("/companies/" + id + "/mva/terminordning").catch(() => ({
      ordning: "to-maneder",
      antall_perioder: 6,
      perioder: [],
      history: [],
    }));
    const yearly = ordningInfo.antall_perioder === 1;
    let year = new Date().getFullYear();
    let termin = yearly ? 1 : Math.floor(new Date().getMonth() / 2) + 1;
    if (q) {
      const params = new URLSearchParams(q);
      year = Number(params.get("year") || year);
      termin = yearly ? 1 : Number(params.get("termin") || termin);
    }
    let report = null;
    try {
      report = await api("/companies/" + id + "/reports/mva?year=" + year + "&termin=" + termin);
    } catch (e) {
      /* none */
    }
    data = { ordningInfo, yearly, year, termin, report };
  }

  function reload() {
    load(companyId, query).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    const id = companyId;
    const q = query;
    data = null;
    load(id, q).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Spesifikasjon
    {companyId}
    yearly={data.yearly}
    year={data.year}
    termin={data.termin}
    report={data.report}
  />

  <Eksport {companyId} year={data.year} termin={data.termin} />

  <Terminordning {companyId} ordningInfo={data.ordningInfo} year={data.year} onDone={reload} />
{/if}
