<script>
  // Oppdrag: det som følger AVTALER — oppdrag til byråer, og jobbene
  // som gis til en agent. Agentene SELV står i Brukere og roller, for
  // en agent er en bruker (docs/agent.md §8.6); det var feil sted å
  // sette og endre rollen som er sikkerhetsgrensen.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Oppdragsliste from "./Oppdragsliste.svelte";
  import Byraliste from "./Byraliste.svelte";
  import Agentoppgaver from "./Agentoppgaver.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [engagements, firms, integrasjoner, oppgaver] = await Promise.all([
      api("/companies/" + id + "/engagements"),
      api("/directory/firms"),
      api("/companies/" + id + "/integrations").catch(() => ({ integrasjoner: [] })),
      api("/companies/" + id + "/agent/oppgaver?apne=false").catch(() => ({ oppgaver: [] })),
    ]);
    data = {
      engagements: engagements.engagements,
      firms: firms.firms,
      // Agentoppgaver trenger listen for å vite hvem en oppgave kan
      // stiles til; selve agentkortet bor i Brukere og roller.
      integrasjoner: integrasjoner.integrasjoner,
      oppgaver: oppgaver.oppgaver,
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
  <Oppdragsliste {companyId} engagements={data.engagements} onDone={reload} />

  <Byraliste {companyId} firms={data.firms} engagements={data.engagements} onDone={reload} />

  <Agentoppgaver
    {companyId}
    integrasjoner={data.integrasjoner}
    oppgaver={data.oppgaver}
    onDone={reload}
  />
{/if}
